//! Commands the UI calls through `invoke`.

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{DictionaryRow, HistoryItem, SnippetRow, Stats};
use crate::dictation::{self, EV_DATA};
use crate::settings::{self, Settings};
use crate::state::AppState;
use crate::text::{self, Style};
use crate::{audio, hotkeys, llm, models, platform};

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------- settings ----------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> CmdResult<Value> {
    let old = state.settings();
    settings::save(&state.data_dir, &settings).map_err(err)?;
    if let Ok(mut s) = state.settings.lock() {
        *s = settings.clone();
    }
    let mut warnings: Vec<String> = vec![];

    if old.hotkey_dictate != settings.hotkey_dictate || old.hotkey_command != settings.hotkey_command {
        if let Err(e) = hotkeys::register(&app) {
            warnings.push(e);
        }
    }
    if old.show_overlay != settings.show_overlay {
        crate::show_overlay(&app, settings.show_overlay);
    }
    if old.model_id != settings.model_id {
        state.engine.unload();
        crate::warm_up(&app);
    }
    if old.api_enabled != settings.api_enabled || old.api_port != settings.api_port {
        if let Err(e) = crate::apply_api(&app) {
            warnings.push(e);
        }
    }
    if let Ok(db) = state.db.lock() {
        let _ = db.prune_history(settings.history_retention_days);
    }
    Ok(json!({ "warnings": warnings }))
}

// ---------------- history ----------------

#[tauri::command]
pub fn get_history(
    state: State<'_, AppState>,
    query: Option<String>,
    limit: Option<i64>,
) -> CmdResult<Vec<HistoryItem>> {
    let db = state.db.lock().map_err(err)?;
    db.history(query.as_deref().unwrap_or(""), limit.unwrap_or(200)).map_err(err)
}

#[tauri::command]
pub fn delete_history(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.db.lock().map_err(err)?.delete_history(id).map_err(err)?;
    let _ = app.emit(EV_DATA, ());
    Ok(())
}

#[tauri::command]
pub fn clear_history(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    state.db.lock().map_err(err)?.clear_history().map_err(err)?;
    let _ = app.emit(EV_DATA, ());
    Ok(())
}

/// Save a corrected transcript and suggest dictionary words from the edit.
#[tauri::command]
pub fn update_history_text(state: State<'_, AppState>, id: i64, text: String) -> CmdResult<Vec<String>> {
    let db = state.db.lock().map_err(err)?;
    let before = db.history_item(id).map_err(err)?.map(|h| h.text).unwrap_or_default();
    db.update_history_text(id, &text).map_err(err)?;
    Ok(text::suggest_dictionary_words(&before, &text).into_iter().filter(|w| !db.has_word(w)).collect())
}

#[tauri::command]
pub fn copy_text(text: String) -> CmdResult<()> {
    let mut c = arboard::Clipboard::new().map_err(err)?;
    c.set_text(text).map_err(err)
}

#[tauri::command]
pub fn get_stats(state: State<'_, AppState>) -> CmdResult<Stats> {
    let wpm = state.settings().typing_wpm;
    state.db.lock().map_err(err)?.stats(wpm).map_err(err)
}

// ---------------- dictionary & snippets ----------------

#[tauri::command]
pub fn list_dictionary(state: State<'_, AppState>) -> CmdResult<Vec<DictionaryRow>> {
    state.db.lock().map_err(err)?.dictionary().map_err(err)
}

#[tauri::command]
pub fn add_dictionary(state: State<'_, AppState>, word: String, variants: Vec<String>) -> CmdResult<()> {
    if word.trim().is_empty() {
        return Err("Type the word as it should be spelled.".into());
    }
    state.db.lock().map_err(err)?.upsert_dictionary(&word, &variants).map_err(err)
}

#[tauri::command]
pub fn delete_dictionary(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.db.lock().map_err(err)?.delete_dictionary(id).map_err(err)
}

#[tauri::command]
pub fn list_snippets(state: State<'_, AppState>) -> CmdResult<Vec<SnippetRow>> {
    state.db.lock().map_err(err)?.snippets().map_err(err)
}

#[tauri::command]
pub fn save_snippet(state: State<'_, AppState>, id: Option<i64>, trigger: String, expansion: String) -> CmdResult<()> {
    if trigger.trim().split_whitespace().count() < 2 {
        return Err("Use a phrase of at least two words so it isn't triggered by accident.".into());
    }
    if expansion.trim().is_empty() {
        return Err("Add the text this phrase should expand to.".into());
    }
    state.db.lock().map_err(err)?.save_snippet(id, &trigger, &expansion).map_err(err)
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.db.lock().map_err(err)?.delete_snippet(id).map_err(err)
}

// ---------------- speech models ----------------

#[derive(Serialize, Clone)]
struct DownloadProgress {
    id: String,
    downloaded: u64,
    total: u64,
    done: bool,
    error: Option<String>,
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Value {
    let downloading = state.downloading.lock().map(|d| d.clone()).unwrap_or_default();
    json!({ "models": models::list(&state.data_dir), "downloading": downloading })
}

#[tauri::command]
pub fn download_model(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    if models::find(&id).is_none() {
        return Err(format!("Unknown model {id}"));
    }
    {
        let mut d = state.downloading.lock().map_err(err)?;
        if d.contains(&id) {
            return Ok(());
        }
        d.push(id.clone());
    }
    let dir = state.data_dir.clone();
    std::thread::spawn(move || {
        let ev = "murmur://model-progress";
        let a = app.clone();
        let mid = id.clone();
        let result = models::download(&dir, &id, |downloaded, total| {
            let _ = a.emit(ev, DownloadProgress { id: mid.clone(), downloaded, total, done: false, error: None });
        });
        if let Ok(mut d) = app.state::<AppState>().downloading.lock() {
            d.retain(|x| x != &id);
        }
        let error = result.as_ref().err().map(|e| e.to_string());
        let _ =
            app.emit(ev, DownloadProgress { id: id.clone(), downloaded: 0, total: 0, done: error.is_none(), error });
        if app.state::<AppState>().settings().model_id == id {
            crate::warm_up(&app);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn delete_model(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    if state.settings().model_id == id {
        state.engine.unload();
    }
    models::delete(&state.data_dir, &id).map_err(err)
}

// ---------------- devices & permissions ----------------

#[tauri::command]
pub fn list_input_devices() -> Vec<String> {
    audio::list_input_devices()
}

#[tauri::command]
pub fn get_permissions() -> platform::Permissions {
    platform::permissions()
}

#[tauri::command]
pub fn open_permission_settings(kind: String) {
    platform::open_permission_settings(&kind);
}

#[tauri::command]
pub fn open_data_folder(state: State<'_, AppState>) {
    platform::open(&state.data_dir.to_string_lossy());
}

#[tauri::command]
pub fn open_url(url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    platform::open(&url);
    Ok(())
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: State<'_, AppState>) -> Value {
    let api = state.api.lock().ok().and_then(|a| a.as_ref().map(|s| s.port));
    json!({
        "version": app.package_info().version.to_string(),
        "data_dir": state.data_dir.to_string_lossy(),
        "platform": platform::permissions().platform,
        "api_port": api,
        "recording": state.recorder.is_recording(),
    })
}

// ---------------- AI clean-up ----------------

#[tauri::command]
pub async fn llm_status(app: AppHandle) -> CmdResult<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>().settings();
        llm::status(&s)
    })
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn ollama_pull(app: AppHandle, model: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>().settings();
        llm::ollama_pull(&s.ollama_url, &model).map_err(|e| format!("Ollama couldn't download {model}: {e}"))
    })
    .await
    .map_err(err)?
}

/// Run text through the clean-up pipeline without the microphone.
#[tauri::command]
pub async fn test_cleanup(app: AppHandle, text: String, style: String) -> CmdResult<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = Style::parse(&style);
        let r = dictation::clean(&app, &text, st, "");
        json!({ "text": r.text, "provider": r.provider })
    })
    .await
    .map_err(err)
}

// ---------------- privacy ----------------

#[tauri::command]
pub fn delete_all_data(app: AppHandle, state: State<'_, AppState>, include_models: bool) -> CmdResult<()> {
    state.db.lock().map_err(err)?.wipe().map_err(err)?;
    let fresh = Settings { onboarded: true, ..Settings::default() };
    settings::save(&state.data_dir, &fresh).map_err(err)?;
    if let Ok(mut s) = state.settings.lock() {
        *s = fresh;
    }
    if include_models {
        state.engine.unload();
        let _ = std::fs::remove_dir_all(models::models_dir(&state.data_dir));
    }
    let _ = hotkeys::register(&app);
    let _ = app.emit(EV_DATA, ());
    Ok(())
}

#[tauri::command]
pub fn finish_onboarding(state: State<'_, AppState>) -> CmdResult<()> {
    let mut s = state.settings();
    s.onboarded = true;
    settings::save(&state.data_dir, &s).map_err(err)?;
    if let Ok(mut cur) = state.settings.lock() {
        *cur = s;
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_recording(app: AppHandle) {
    dictation::send_event(&app, crate::state::KeyEvent::Cancel);
}
