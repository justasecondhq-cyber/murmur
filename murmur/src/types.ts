export type StyleName = "neutral" | "casual" | "formal" | "code";

export interface AppRule {
  pattern: string;
  style: StyleName;
}

export interface Settings {
  onboarded: boolean;
  hotkey_dictate: string;
  hotkey_command: string;
  hotkey_mode: "hold" | "toggle";
  model_id: string;
  language: string;
  input_device: string | null;
  whisper_mode: boolean;
  max_recording_secs: number;
  cleanup_enabled: boolean;
  spoken_punctuation: boolean;
  llm_provider: "auto" | "ollama" | "openai" | "anthropic" | "none";
  ollama_url: string;
  ollama_model: string;
  openai_base_url: string;
  openai_api_key: string;
  openai_model: string;
  anthropic_api_key: string;
  anthropic_model: string;
  per_app_styles: boolean;
  default_style: StyleName;
  app_rules: AppRule[];
  show_overlay: boolean;
  play_sounds: boolean;
  restore_clipboard: boolean;
  save_history: boolean;
  history_retention_days: number;
  api_enabled: boolean;
  api_port: number;
  typing_wpm: number;
}

export interface HistoryItem {
  id: number;
  created_at: number;
  raw: string;
  text: string;
  app: string;
  style: string;
  mode: "dictate" | "command";
  duration_ms: number;
  process_ms: number;
  words: number;
}

export interface Stats {
  total_words: number;
  total_dictations: number;
  audio_minutes: number;
  minutes_saved: number;
  avg_wpm: number;
  streak_days: number;
  words_today: number;
  daily_words: number[];
}

export interface DictionaryRow {
  id: number;
  word: string;
  variants: string[];
  created_at: number;
}

export interface SnippetRow {
  id: number;
  trigger: string;
  expansion: string;
  created_at: number;
}

export interface ModelStatus {
  id: string;
  name: string;
  file: string;
  size_mb: number;
  english_only: boolean;
  speed: string;
  accuracy: string;
  note: string;
  installed: boolean;
}

export interface ModelList {
  models: ModelStatus[];
  downloading: string[];
}

export interface DownloadProgress {
  id: string;
  downloaded: number;
  total: number;
  done: boolean;
  error: string | null;
}

export interface Permissions {
  platform: "macos" | "windows" | "linux";
  accessibility: boolean | null;
  microphone_devices: number;
  wayland: boolean;
}

export interface LlmStatus {
  active: string;
  active_is_local: boolean;
  using_llm: boolean;
  ollama_running: boolean;
  ollama_models: string[];
  ollama_has_model: boolean;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  platform: "macos" | "windows" | "linux";
  api_port: number | null;
  recording: boolean;
}

export type Phase = "idle" | "recording" | "processing" | "done" | "error";

export interface StatePayload {
  phase: Phase;
  mode: "dictate" | "command";
  locked: boolean;
  message: string | null;
  sounds: boolean;
}

export interface DictationPayload {
  id: number | null;
  raw: string;
  text: string;
  app: string;
  style: string;
  mode: string;
  provider: string;
  duration_ms: number;
  process_ms: number;
  words: number;
}
