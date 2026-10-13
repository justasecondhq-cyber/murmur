//! Shared application state.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Mutex};
use std::time::Instant;

use tauri_plugin_global_shortcut::Shortcut;

use crate::audio::Recorder;
use crate::db::Db;
use crate::platform::FocusedApp;
use crate::settings::Settings;
use crate::stt::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dictate,
    Command,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Dictate => "dictate",
            Mode::Command => "command",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: u64,
    pub mode: Mode,
    pub started: Instant,
    /// Hands-free: recording continues after the key is released.
    pub locked: bool,
    pub app: FocusedApp,
}

#[derive(Debug, Clone, Default)]
pub struct Hotkeys {
    pub dictate: Option<Shortcut>,
    pub command: Option<Shortcut>,
    pub escape: Option<Shortcut>,
}

#[derive(Debug, Clone, Copy)]
pub enum KeyEvent {
    Pressed(Mode),
    Released(Mode),
    Cancel,
    /// Fired by the max-length watchdog with the session id it was armed for.
    Timeout(u64),
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub db: Mutex<Db>,
    pub recorder: Recorder,
    pub engine: Engine,
    pub session: Mutex<Option<Session>>,
    pub busy: AtomicBool,
    pub hotkeys: Mutex<Hotkeys>,
    pub key_tx: Mutex<Option<mpsc::Sender<KeyEvent>>>,
    pub downloading: Mutex<Vec<String>>,
    pub api: Mutex<Option<crate::api::ApiServer>>,
    pub next_session: Mutex<u64>,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
}
