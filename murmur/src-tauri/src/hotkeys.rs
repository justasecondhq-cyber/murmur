//! Global hotkeys (push-to-talk, command mode, Esc to cancel).

use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::dictation::send_event;
use crate::state::{AppState, KeyEvent, Mode};

/// Plugin with a handler that forwards events to the dictation worker.
pub fn plugin() -> tauri::plugin::TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent| on_shortcut(app, shortcut, event))
        .build()
}

fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let state = app.state::<AppState>();
    let hk = match state.hotkeys.lock() {
        Ok(h) => h.clone(),
        Err(_) => return,
    };
    let pressed = event.state() == ShortcutState::Pressed;
    let ev = if hk.escape.as_ref() == Some(shortcut) {
        if !pressed {
            return;
        }
        KeyEvent::Cancel
    } else if hk.dictate.as_ref() == Some(shortcut) {
        if pressed {
            KeyEvent::Pressed(Mode::Dictate)
        } else {
            KeyEvent::Released(Mode::Dictate)
        }
    } else if hk.command.as_ref() == Some(shortcut) {
        if pressed {
            KeyEvent::Pressed(Mode::Command)
        } else {
            KeyEvent::Released(Mode::Command)
        }
    } else {
        return;
    };
    // Hand off to the worker thread; never block the event loop here.
    send_event(app, ev);
}

/// Parse a hotkey string like "Alt+Space" or "CommandOrControl+Shift+D".
pub fn parse(s: &str) -> Result<Shortcut, String> {
    s.trim().parse::<Shortcut>().map_err(|e| format!("\"{s}\" isn't a valid shortcut ({e})"))
}

/// (Re)register the dictation and command hotkeys from settings.
///
/// The hotkeys mutex is never held while calling into the shortcut manager:
/// registration hops to the main thread, which may itself be waiting on that
/// mutex inside `on_shortcut`.
pub fn register(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let gs = app.global_shortcut();

    let old: Vec<Shortcut> = {
        let mut hk = state.hotkeys.lock().map_err(|e| e.to_string())?;
        [hk.dictate.take(), hk.command.take()].into_iter().flatten().collect()
    };
    for sc in old {
        let _ = gs.unregister(sc);
    }

    let mut errors = vec![];
    let mut dictate: Option<Shortcut> = None;
    let mut command: Option<Shortcut> = None;
    match parse(&settings.hotkey_dictate) {
        Ok(sc) => match gs.register(sc) {
            Ok(()) => dictate = Some(sc),
            Err(e) => {
                errors.push(format!("Dictation shortcut {} is taken by another app ({e}).", settings.hotkey_dictate))
            }
        },
        Err(e) => errors.push(e),
    }
    if !settings.hotkey_command.trim().is_empty() {
        match parse(&settings.hotkey_command) {
            Ok(sc) if Some(sc) != dictate => match gs.register(sc) {
                Ok(()) => command = Some(sc),
                Err(e) => {
                    errors.push(format!("Command shortcut {} is taken by another app ({e}).", settings.hotkey_command))
                }
            },
            Ok(_) => errors.push("The command shortcut must be different from the dictation shortcut.".into()),
            Err(e) => errors.push(e),
        }
    }
    if let Ok(mut hk) = state.hotkeys.lock() {
        hk.dictate = dictate;
        hk.command = command;
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" "))
    }
}
