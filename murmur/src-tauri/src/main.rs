// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod audio;
mod commands;
mod db;
mod dictation;
mod dsp;
mod hotkeys;
mod insert;
mod llm;
mod models;
mod platform;
mod settings;
mod state;
mod stt;
mod text;

use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, WindowEvent};

use state::{AppState, Hotkeys};

/// Load the speech model in the background so the first dictation is fast.
pub fn warm_up(app: &AppHandle) {
    let a = app.clone();
    std::thread::spawn(move || {
        let st = a.state::<AppState>();
        let s = st.settings();
        if let Some(path) = models::model_path(&st.data_dir, &s.model_id) {
            if path.exists() && !st.engine.is_loaded(&path) {
                if let Err(e) = st.engine.context(&path) {
                    eprintln!("[murmur] model warm-up failed: {e}");
                }
            }
        }
    });
}

/// Start or stop the local HTTP API to match settings.
pub fn apply_api(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let s = st.settings();
    let mut slot = st.api.lock().map_err(|e| e.to_string())?;
    if let Some(server) = slot.take() {
        server.stop();
    }
    if s.api_enabled {
        *slot = Some(api::start(app.clone(), s.api_port)?);
    }
    Ok(())
}

/// The overlay is a small always-on-top, click-through window that stays
/// "shown" and draws nothing while idle, so it never steals focus.
pub fn show_overlay(app: &AppHandle, visible: bool) {
    if let Some(w) = app.get_webview_window("overlay") {
        if visible {
            let _ = w.set_ignore_cursor_events(true);
            if let Ok(Some(monitor)) = w.primary_monitor() {
                let size = monitor.size();
                let pos = monitor.position();
                let scale = monitor.scale_factor();
                let (ww, wh) = (260.0 * scale, 72.0 * scale);
                let x = pos.x as f64 + (size.width as f64 - ww) / 2.0;
                let y = pos.y as f64 + size.height as f64 - wh - 96.0 * scale;
                let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
            }
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Murmur", true, None::<&str>)?;
    let hands_free = MenuItem::with_id(app, "hands_free", "Start / stop hands-free dictation", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Murmur", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &hands_free, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("murmur-tray")
        .tooltip("Murmur — hold your shortcut and speak")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "hands_free" => {
                // Behaves like a tap: start locked, or stop if already recording.
                dictation::send_event(app, state::KeyEvent::Pressed(state::Mode::Dictate));
                let a = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    dictation::send_event(&a, state::KeyEvent::Released(state::Mode::Dictate));
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(hotkeys::plugin())
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir().expect("no app data directory");
            std::fs::create_dir_all(&data_dir).ok();
            let settings = settings::load(&data_dir);
            let database = db::Db::open(&data_dir).expect("couldn't open the Murmur database");
            let _ = database.prune_history(settings.history_retention_days);
            let onboarded = settings.onboarded;
            let overlay_on = settings.show_overlay;

            app.manage(AppState {
                data_dir,
                settings: Mutex::new(settings),
                db: Mutex::new(database),
                recorder: audio::Recorder::new(),
                engine: stt::Engine::new(),
                session: Mutex::new(None),
                busy: AtomicBool::new(false),
                hotkeys: Mutex::new(Hotkeys::default()),
                key_tx: Mutex::new(None),
                downloading: Mutex::new(vec![]),
                api: Mutex::new(None),
                next_session: Mutex::new(0),
            });

            let tx = dictation::start_worker(handle.clone());
            if let Ok(mut slot) = app.state::<AppState>().key_tx.lock() {
                *slot = Some(tx);
            }

            if let Err(e) = hotkeys::register(&handle) {
                eprintln!("[murmur] {e}");
            }
            if let Err(e) = apply_api(&handle) {
                eprintln!("[murmur] local API: {e}");
            }
            build_tray(&handle)?;
            show_overlay(&handle, overlay_on);
            warm_up(&handle);

            // First run: show the main window for onboarding. Afterwards Murmur
            // starts quietly in the tray / menu bar.
            if !onboarded {
                show_main(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps Murmur running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_history,
            commands::delete_history,
            commands::clear_history,
            commands::update_history_text,
            commands::copy_text,
            commands::get_stats,
            commands::list_dictionary,
            commands::add_dictionary,
            commands::delete_dictionary,
            commands::list_snippets,
            commands::save_snippet,
            commands::delete_snippet,
            commands::list_models,
            commands::download_model,
            commands::delete_model,
            commands::list_input_devices,
            commands::get_permissions,
            commands::open_permission_settings,
            commands::open_data_folder,
            commands::open_url,
            commands::get_app_info,
            commands::llm_status,
            commands::ollama_pull,
            commands::test_cleanup,
            commands::delete_all_data,
            commands::finish_onboarding,
            commands::cancel_recording,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Murmur");
}
