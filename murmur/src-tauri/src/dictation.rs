//! The dictation flow: hotkey events -> record -> transcribe -> clean -> insert.

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::{AppState, KeyEvent, Mode, Session};
use crate::text::{self, CleanOptions, Style};
use crate::{dsp, insert, llm, models, platform};

pub const EV_STATE: &str = "murmur://state";
pub const EV_LEVEL: &str = "murmur://level";
pub const EV_DICTATION: &str = "murmur://dictation";
pub const EV_DATA: &str = "murmur://data-changed";

#[derive(Debug, Clone, Serialize)]
pub struct StatePayload {
    /// "idle" | "recording" | "processing" | "done" | "error"
    pub phase: &'static str,
    pub mode: &'static str,
    pub locked: bool,
    pub message: Option<String>,
    pub sounds: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictationPayload {
    pub id: Option<i64>,
    pub raw: String,
    pub text: String,
    pub app: String,
    pub style: String,
    pub mode: String,
    pub provider: String,
    pub duration_ms: i64,
    pub process_ms: i64,
    pub words: i64,
}

pub fn emit_state(app: &AppHandle, phase: &'static str, mode: Mode, locked: bool, message: Option<String>) {
    let sounds = app.state::<AppState>().settings().play_sounds;
    let _ = app.emit(EV_STATE, StatePayload { phase, mode: mode.as_str(), locked, message, sounds });
}

/// Spawn the worker that turns hotkey events into recording sessions.
/// All handling happens on this one thread so press/release never race.
pub fn start_worker(app: AppHandle) -> mpsc::Sender<KeyEvent> {
    let (tx, rx) = mpsc::channel::<KeyEvent>();
    std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            handle_event(&app, ev);
        }
    });
    tx
}

pub fn send_event(app: &AppHandle, ev: KeyEvent) {
    if let Ok(tx) = app.state::<AppState>().key_tx.lock() {
        if let Some(tx) = tx.as_ref() {
            let _ = tx.send(ev);
        }
    }
}

fn handle_event(app: &AppHandle, ev: KeyEvent) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let current = state.session.lock().ok().and_then(|s| s.clone());

    match ev {
        KeyEvent::Pressed(mode) => {
            if let Some(sess) = current {
                // Already recording: a press stops hands-free / toggle recordings.
                if sess.locked || settings.hotkey_mode == "toggle" {
                    finish(app);
                }
                // Otherwise it's key-repeat while holding: ignore.
                return;
            }
            if state.busy.load(Ordering::SeqCst) {
                return;
            }
            begin(app, mode);
        }
        KeyEvent::Released(_mode) => {
            let Some(sess) = current else { return };
            if settings.hotkey_mode == "toggle" || sess.locked {
                return;
            }
            if sess.started.elapsed() < Duration::from_millis(300) {
                // Quick tap: lock into hands-free mode.
                if let Ok(mut s) = state.session.lock() {
                    if let Some(s) = s.as_mut() {
                        s.locked = true;
                    }
                }
                emit_state(app, "recording", sess.mode, true, None);
                return;
            }
            finish(app);
        }
        KeyEvent::Cancel => {
            if current.is_some() {
                state.recorder.cancel();
                end_session(app);
                emit_state(
                    app,
                    "idle",
                    current.map(|s| s.mode).unwrap_or(Mode::Dictate),
                    false,
                    Some("Cancelled".into()),
                );
            }
        }
        KeyEvent::Timeout(id) => {
            if current.map(|s| s.id == id).unwrap_or(false) {
                finish(app);
            }
        }
    }
}

fn register_escape(app: &AppHandle, on: bool) {
    // Never hold the hotkeys lock while talking to the shortcut manager (see hotkeys::register).
    let state = app.state::<AppState>();
    let gs = app.global_shortcut();
    if on {
        let already = state.hotkeys.lock().map(|h| h.escape.is_some()).unwrap_or(true);
        if already {
            return;
        }
        if let Ok(sc) = "Escape".parse::<Shortcut>() {
            if gs.register(sc).is_ok() {
                if let Ok(mut hk) = state.hotkeys.lock() {
                    hk.escape = Some(sc);
                }
            }
        }
    } else {
        let taken = state.hotkeys.lock().ok().and_then(|mut h| h.escape.take());
        if let Some(sc) = taken {
            let _ = gs.unregister(sc);
        }
    }
}

fn begin(app: &AppHandle, mode: Mode) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    // Remember where the text should go before anything else can take focus.
    let focused = platform::focused_app();

    if let Err(e) = state.recorder.start(settings.input_device.clone()) {
        emit_state(app, "error", mode, false, Some(e.to_string()));
        return;
    }
    let id = {
        let mut n = state.next_session.lock().unwrap();
        *n += 1;
        *n
    };
    if let Ok(mut s) = state.session.lock() {
        *s =
            Some(Session { id, mode, started: Instant::now(), locked: settings.hotkey_mode == "toggle", app: focused });
    }
    emit_state(app, "recording", mode, settings.hotkey_mode == "toggle", None);
    register_escape(app, true);

    // Stream the input level to the overlay.
    let a = app.clone();
    std::thread::spawn(move || {
        let st = a.state::<AppState>();
        while st.recorder.is_recording() {
            let _ = a.emit(EV_LEVEL, st.recorder.level());
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = a.emit(EV_LEVEL, 0.0f32);
    });

    // Safety net: stop after the maximum recording length.
    let a = app.clone();
    let max = settings.max_recording_secs.max(10) as u64;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(max));
        send_event(&a, KeyEvent::Timeout(id));
    });
}

fn end_session(app: &AppHandle) -> Option<Session> {
    let state = app.state::<AppState>();
    register_escape(app, false);
    state.session.lock().ok().and_then(|mut s| s.take())
}

fn finish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (samples, duration_ms) = state.recorder.stop();
    let Some(sess) = end_session(app) else { return };
    state.busy.store(true, Ordering::SeqCst);
    emit_state(app, "processing", sess.mode, false, None);

    let a = app.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = match sess.mode {
            Mode::Dictate => run_dictation(&a, &sess, samples, duration_ms),
            Mode::Command => run_command(&a, &sess, samples, duration_ms),
        };
        let st = a.state::<AppState>();
        st.busy.store(false, Ordering::SeqCst);
        match result {
            Ok(mut payload) => {
                payload.process_ms = started.elapsed().as_millis() as i64;
                emit_state(&a, "done", sess.mode, false, None);
                let _ = a.emit(EV_DICTATION, payload);
                let _ = a.emit(EV_DATA, ());
            }
            Err(msg) => emit_state(&a, "error", sess.mode, false, Some(msg)),
        }
    });
}

// ---------------------------------------------------------------------------
// Shared steps
// ---------------------------------------------------------------------------

/// Trim, normalize and transcribe 16 kHz audio. Ok(None) means "no speech".
pub fn transcribe(app: &AppHandle, samples: Vec<f32>) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let Some(trimmed) = dsp::trim_silence(&samples, dsp::TARGET_RATE, settings.whisper_mode) else {
        return Ok(None);
    };
    let mut audio = trimmed.samples;
    dsp::normalize(&mut audio, if settings.whisper_mode { 30.0 } else { 8.0 });
    dsp::pad_to(&mut audio, dsp::TARGET_RATE, 1_100);

    let info = models::find(&settings.model_id).ok_or("Pick a speech model in Settings.")?;
    let path = models::model_path(&state.data_dir, info.id).ok_or("Unknown speech model")?;
    let dict = state.db.lock().map(|d| d.dict_entries()).unwrap_or_default();
    let prompt = text::whisper_prompt(&dict);
    let raw = state
        .engine
        .transcribe(&path, &audio, &settings.language, info.english_only, &prompt)
        .map_err(|e| e.to_string())?;
    let stripped = text::strip_whisper_artifacts(&raw);
    if text::is_hallucination(&stripped) && trimmed.speech_ms < 1500 {
        return Ok(None);
    }
    if stripped.is_empty() {
        return Ok(None);
    }
    Ok(Some(stripped))
}

pub struct CleanResult {
    pub text: String,
    pub provider: String,
}

/// Turn a raw transcript into final text for the given style.
pub fn clean(app: &AppHandle, raw: &str, style: Style, app_name: &str) -> CleanResult {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let (dict, snippets) = match state.db.lock() {
        Ok(db) => (db.dict_entries(), db.snippet_entries()),
        Err(_) => (vec![], vec![]),
    };

    if let Some(exp) = text::exact_snippet(raw, &snippets) {
        return CleanResult { text: exp, provider: "Snippet".into() };
    }

    let expand = |t: &str| -> String {
        let (p, map) = text::protect_snippets(t, &snippets);
        text::restore_snippets(&p, &map).unwrap_or_else(|| t.to_string())
    };

    if !settings.cleanup_enabled {
        let t = text::apply_dictionary(&text::strip_whisper_artifacts(raw), &dict);
        return CleanResult { text: expand(&t), provider: "Raw".into() };
    }

    let opts = CleanOptions { style, spoken_punctuation: settings.spoken_punctuation, dictionary: dict.clone() };
    let rules = expand(&text::rule_clean(raw, &opts));

    // Code stays rule-based: literal and instant.
    if style == Style::Code {
        return CleanResult { text: rules, provider: "Rules".into() };
    }
    let provider = llm::resolve(&settings);
    if provider == llm::Provider::None {
        return CleanResult { text: rules, provider: "Rules".into() };
    }

    let (protected, map) = text::protect_snippets(raw, &snippets);
    let system = text::build_system_prompt(style, &dict, app_name);
    let user = text::build_user_message(&protected);
    let llm_text = llm::chat(&provider, &system, &user, 20)
        .ok()
        .and_then(|out| text::sanitize_llm_output(&protected, &out))
        .and_then(|out| text::restore_snippets(&out, &map));
    match llm_text {
        Some(t) => {
            let mut t = text::apply_dictionary(&t, &dict);
            if style == Style::Casual {
                // Keep chat messages light even if the model adds a final period.
                t = text::rule_clean(&t, &CleanOptions { style, ..Default::default() });
                t = text::apply_dictionary(&t, &dict);
            }
            CleanResult { text: t, provider: provider.label() }
        }
        None => CleanResult { text: rules, provider: "Rules (AI fallback)".into() },
    }
}

fn style_for(app: &AppHandle, sess: &Session) -> Style {
    let s = app.state::<AppState>().settings();
    let default = Style::parse(&s.default_style);
    if !s.per_app_styles {
        return default;
    }
    let rules: Vec<text::AppRule> = s
        .app_rules
        .iter()
        .map(|r| text::AppRule { pattern: r.pattern.clone(), style: Style::parse(&r.style) })
        .collect();
    text::detect_style(&sess.app.name, &sess.app.title, &rules, default)
}

fn save(app: &AppHandle, p: &DictationPayload) -> Option<i64> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let db = state.db.lock().ok()?;
    db.add_history(
        settings.save_history,
        &p.raw,
        &p.text,
        &p.app,
        &p.style,
        &p.mode,
        p.duration_ms,
        p.process_ms,
        p.words,
    )
    .ok()
    .flatten()
    .map(|h| h.id)
}

fn deliver(app: &AppHandle, text: &str) -> Result<(), String> {
    let restore = app.state::<AppState>().settings().restore_clipboard;
    if let Err(e) = insert::insert_text(text, restore) {
        // Leave the text on the clipboard so nothing is lost.
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.set_text(text.to_string());
        }
        return Err(format!("Couldn't type into the app ({e}). The text is on your clipboard."));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

fn run_dictation(
    app: &AppHandle,
    sess: &Session,
    samples: Vec<f32>,
    duration_ms: u32,
) -> Result<DictationPayload, String> {
    let t0 = Instant::now();
    let raw = transcribe(app, samples)?.ok_or("Didn't catch that. Try again closer to the mic.")?;
    let style = style_for(app, sess);
    let cleaned = clean(app, &raw, style, &sess.app.name);
    if cleaned.text.trim().is_empty() {
        return Err("Nothing to insert.".into());
    }
    deliver(app, &cleaned.text)?;
    let mut p = DictationPayload {
        id: None,
        words: text::word_count(&cleaned.text) as i64,
        raw,
        text: cleaned.text,
        app: sess.app.name.clone(),
        style: style.as_str().into(),
        mode: "dictate".into(),
        provider: cleaned.provider,
        duration_ms: duration_ms as i64,
        process_ms: t0.elapsed().as_millis() as i64,
    };
    p.id = save(app, &p);
    Ok(p)
}

fn run_command(
    app: &AppHandle,
    sess: &Session,
    samples: Vec<f32>,
    duration_ms: u32,
) -> Result<DictationPayload, String> {
    let t0 = Instant::now();
    let settings = app.state::<AppState>().settings();
    let provider = llm::resolve(&settings);
    if provider == llm::Provider::None {
        return Err("Command mode needs an AI model. Set one up in Settings → AI clean-up.".into());
    }
    let raw = transcribe(app, samples)?.ok_or("Didn't catch the instruction — try again.")?;
    let instruction = text::rule_clean(&raw, &CleanOptions::default());
    // The hotkey has been released by now, so copying won't collide with it.
    std::thread::sleep(Duration::from_millis(120));
    let selection = insert::read_selection().unwrap_or_default();
    let (system, user) = text::build_command_messages(&selection, &instruction, &sess.app.name);
    let out = llm::chat(&provider, &system, &user, 90).map_err(|e| format!("AI request failed: {e}"))?;
    let result = text::sanitize_command_output(&out).ok_or("The AI model returned nothing usable.")?;
    deliver(app, &result)?;
    let mut p = DictationPayload {
        id: None,
        words: text::word_count(&result) as i64,
        raw: instruction,
        text: result,
        app: sess.app.name.clone(),
        style: "command".into(),
        mode: "command".into(),
        provider: provider.label(),
        duration_ms: duration_ms as i64,
        process_ms: t0.elapsed().as_millis() as i64,
    };
    p.id = save(app, &p);
    Ok(p)
}
