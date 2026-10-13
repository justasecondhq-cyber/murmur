//! User settings, persisted as JSON in the app data folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRuleSetting {
    pub pattern: String,
    pub style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub onboarded: bool,

    // Hotkeys
    pub hotkey_dictate: String,
    pub hotkey_command: String,
    /// "hold" = push-to-talk (a quick tap locks hands-free), "toggle" = tap to start/stop.
    pub hotkey_mode: String,

    // Speech recognition
    pub model_id: String,
    /// "auto" or an ISO code such as "en", "fil", "es".
    pub language: String,
    pub input_device: Option<String>,
    pub whisper_mode: bool,
    pub max_recording_secs: u32,

    // Clean-up
    pub cleanup_enabled: bool,
    pub spoken_punctuation: bool,
    /// "auto" | "ollama" | "openai" | "anthropic" | "none"
    pub llm_provider: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_base_url: String,
    pub openai_api_key: String,
    pub openai_model: String,
    pub anthropic_api_key: String,
    pub anthropic_model: String,

    // Per-app styles
    pub per_app_styles: bool,
    pub default_style: String,
    pub app_rules: Vec<AppRuleSetting>,

    // Interface
    pub show_overlay: bool,
    pub play_sounds: bool,
    pub restore_clipboard: bool,

    // Privacy
    pub save_history: bool,
    /// 0 = keep forever.
    pub history_retention_days: u32,

    // Local API for other tools
    pub api_enabled: bool,
    pub api_port: u16,

    // Stats
    pub typing_wpm: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            onboarded: false,
            hotkey_dictate: "Alt+Space".into(),
            hotkey_command: "Alt+Shift+Space".into(),
            hotkey_mode: "hold".into(),
            model_id: "base.en".into(),
            language: "auto".into(),
            input_device: None,
            whisper_mode: false,
            max_recording_secs: 360,
            cleanup_enabled: true,
            spoken_punctuation: false,
            llm_provider: "auto".into(),
            ollama_url: "http://127.0.0.1:11434".into(),
            ollama_model: "qwen2.5:3b".into(),
            openai_base_url: "https://api.openai.com/v1".into(),
            openai_api_key: String::new(),
            openai_model: "gpt-4o-mini".into(),
            anthropic_api_key: String::new(),
            anthropic_model: "claude-haiku-4-5".into(),
            per_app_styles: true,
            default_style: "neutral".into(),
            app_rules: vec![],
            show_overlay: true,
            play_sounds: true,
            restore_clipboard: true,
            save_history: true,
            history_retention_days: 0,
            api_enabled: false,
            api_port: 7071,
            typing_wpm: 40,
        }
    }
}

pub fn settings_path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

pub fn load(dir: &Path) -> Settings {
    match std::fs::read_to_string(settings_path(dir)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(dir: &Path, s: &Settings) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join("settings.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(s)?)?;
    std::fs::rename(tmp, settings_path(dir))?;
    Ok(())
}
