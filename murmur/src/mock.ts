// In-browser stand-in for the Rust backend, used by `npm run dev` outside
// Tauri and for UI screenshots. Holding Alt+Space simulates a dictation.
import type {
  DictionaryRow, HistoryItem, ModelStatus, Settings, SnippetRow, StatePayload,
} from "./types";

type Cb = (payload: any) => void;
const listeners = new Map<string, Set<Cb>>();
const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((cb) => cb(payload));

const params = new URLSearchParams(location.search);
const now = Math.floor(Date.now() / 1000);

const settings: Settings = {
  onboarded: params.get("onboarding") !== "1",
  hotkey_dictate: "Alt+Space",
  hotkey_command: "Alt+Shift+Space",
  hotkey_mode: "hold",
  model_id: "base.en",
  language: "auto",
  input_device: null,
  whisper_mode: false,
  max_recording_secs: 360,
  cleanup_enabled: true,
  spoken_punctuation: false,
  llm_provider: "auto",
  ollama_url: "http://127.0.0.1:11434",
  ollama_model: "qwen2.5:3b",
  openai_base_url: "https://api.openai.com/v1",
  openai_api_key: "",
  openai_model: "gpt-4o-mini",
  anthropic_api_key: "",
  anthropic_model: "claude-haiku-4-5",
  per_app_styles: true,
  default_style: "neutral",
  app_rules: [{ pattern: "Notion", style: "formal" }],
  show_overlay: true,
  play_sounds: true,
  restore_clipboard: true,
  save_history: true,
  history_retention_days: 0,
  api_enabled: false,
  api_port: 7071,
  typing_wpm: 40,
};

let history: HistoryItem[] = [
  { id: 6, created_at: now - 120, raw: "um hey sarah so uh I wanted to check if we're still on for like lunch on friday no wait thursday", text: "Hey Sarah, I wanted to check if we're still on for lunch on Thursday.", app: "Slack", style: "casual", mode: "dictate", duration_ms: 6100, process_ms: 840, words: 13 },
  { id: 5, created_at: now - 3600, raw: "camel case retry count equals three", text: "retryCount = three", app: "Cursor", style: "code", mode: "dictate", duration_ms: 2400, process_ms: 210, words: 3 },
  { id: 4, created_at: now - 7200, raw: "okay so the plan is first we fix the login bug second we write the tests and third we ship it", text: "Okay, so the plan is:\n1. Fix the login bug\n2. Write the tests\n3. Ship it", app: "Notion", style: "formal", mode: "dictate", duration_ms: 7300, process_ms: 1210, words: 15 },
  { id: 3, created_at: now - 86400, raw: "make this more friendly", text: "Thanks so much for flagging this! I'll take a look first thing tomorrow and get back to you.", app: "Mail", style: "command", mode: "command", duration_ms: 1900, process_ms: 2300, words: 18 },
  { id: 2, created_at: now - 90000, raw: "can you send the deck to dana at anthropic before the cooper netties review", text: "Can you send the deck to Dana at Anthropic before the Kubernetes review?", app: "Mail", style: "formal", mode: "dictate", duration_ms: 4200, process_ms: 960, words: 13 },
];

let dictionary: DictionaryRow[] = [
  { id: 1, word: "Kubernetes", variants: ["cooper netties"], created_at: now },
  { id: 2, word: "Anthropic", variants: [], created_at: now },
  { id: 3, word: "Tauri", variants: ["tory", "towery"], created_at: now },
];
let snippets: SnippetRow[] = [
  { id: 1, trigger: "my calendar link", expansion: "https://cal.com/alex/30min", created_at: now },
  { id: 2, trigger: "my work email", expansion: "alex@example.com", created_at: now },
];

const catalog: ModelStatus[] = [
  ["tiny.en", "Tiny (English)", 75, true, "Fastest", "Basic", "For older laptops. Expect more mistakes."],
  ["base.en", "Base (English)", 142, true, "Very fast", "Good", "Recommended starting point for English."],
  ["base", "Base (multilingual)", 142, false, "Very fast", "Good", "99 languages, including mixed speech like Taglish."],
  ["small.en", "Small (English)", 466, true, "Fast", "Great", "Noticeably better with names and jargon."],
  ["small", "Small (multilingual)", 466, false, "Fast", "Great", "Best balance for non-English speech."],
  ["large-v3-turbo-q5_0", "Large v3 Turbo (compressed)", 547, false, "Medium", "Excellent", "Near top accuracy. Best on Apple Silicon or a GPU."],
  ["large-v3-turbo", "Large v3 Turbo", 1620, false, "Slower", "Excellent", "Maximum accuracy. Needs a strong machine."],
].map(([id, name, size_mb, english_only, speed, accuracy, note]) => ({
  id, name, file: `ggml-${id}.bin`, size_mb, english_only, speed, accuracy, note,
  installed: id === "base.en" && params.get("onboarding") !== "1",
})) as ModelStatus[];
const downloading = new Set<string>();

const delay = <T,>(v: T, ms = 80) => new Promise<T>((r) => setTimeout(() => r(v), ms));

export async function mockInvoke<T>(cmd: string, a: Record<string, any> = {}): Promise<T> {
  switch (cmd) {
    case "get_settings": return delay({ ...settings } as T);
    case "save_settings": Object.assign(settings, a.settings); return delay({ warnings: [] } as T);
    case "finish_onboarding": settings.onboarded = true; return delay(undefined as T);
    case "get_history": {
      const q = (a.query ?? "").toLowerCase();
      return delay(history.filter((h) => !q || h.text.toLowerCase().includes(q) || h.app.toLowerCase().includes(q)) as T);
    }
    case "delete_history": history = history.filter((h) => h.id !== a.id); return delay(undefined as T);
    case "clear_history": history = []; return delay(undefined as T);
    case "update_history_text": {
      const h = history.find((x) => x.id === a.id);
      const before = h?.text ?? "";
      if (h) h.text = a.text;
      const words = String(a.text).split(/\s+/).map((w: string) => w.replace(/[^\w'-]/g, ""))
        .filter((w: string) => w.length > 1 && !before.toLowerCase().includes(w.toLowerCase()) && /[A-Z]/.test(w.slice(1) + (w[0] === w[0].toUpperCase() ? "X" : "")));
      return delay(words.slice(0, 5) as T);
    }
    case "copy_text": return delay(undefined as T);
    case "get_stats": return delay({
      total_words: 18342, total_dictations: 1204, audio_minutes: 112.4, minutes_saved: 346.1,
      avg_wpm: 163, streak_days: 9, words_today: 812,
      daily_words: [420, 0, 610, 980, 1220, 760, 300, 0, 540, 1130, 1480, 920, 1010, 812],
    } as T);
    case "list_dictionary": return delay([...dictionary] as T);
    case "add_dictionary": dictionary.push({ id: Date.now(), word: a.word, variants: a.variants, created_at: now }); return delay(undefined as T);
    case "delete_dictionary": dictionary = dictionary.filter((d) => d.id !== a.id); return delay(undefined as T);
    case "list_snippets": return delay([...snippets] as T);
    case "save_snippet": {
      if (String(a.trigger).trim().split(/\s+/).length < 2) throw "Use a phrase of at least two words so it isn't triggered by accident.";
      if (a.id) { const s = snippets.find((x) => x.id === a.id); if (s) { s.trigger = a.trigger; s.expansion = a.expansion; } }
      else snippets.push({ id: Date.now(), trigger: a.trigger, expansion: a.expansion, created_at: now });
      return delay(undefined as T);
    }
    case "delete_snippet": snippets = snippets.filter((s) => s.id !== a.id); return delay(undefined as T);
    case "list_models": return delay({ models: catalog.map((m) => ({ ...m })), downloading: [...downloading] } as T);
    case "download_model": {
      const m = catalog.find((x) => x.id === a.id)!;
      downloading.add(m.id);
      const total = m.size_mb * 1048576; let done = 0;
      const t = setInterval(() => {
        done += total / 25;
        if (done >= total) {
          clearInterval(t); downloading.delete(m.id); m.installed = true;
          emit("murmur://model-progress", { id: m.id, downloaded: total, total, done: true, error: null });
        } else emit("murmur://model-progress", { id: m.id, downloaded: done, total, done: false, error: null });
      }, 120);
      return delay(undefined as T);
    }
    case "delete_model": { const m = catalog.find((x) => x.id === a.id); if (m) m.installed = false; return delay(undefined as T); }
    case "list_input_devices": return delay(["MacBook Pro Microphone", "Shure MV7", "AirPods Pro"] as T);
    case "get_permissions": return delay({ platform: "macos", accessibility: params.get("onboarding") !== "1", microphone_devices: 3, wayland: false } as T);
    case "get_app_info": return delay({ version: "0.1.0", data_dir: "~/Library/Application Support/app.murmur.dictation", platform: "macos", api_port: null, recording: false } as T);
    case "llm_status": return delay({
      active: "Ollama · qwen2.5:3b", active_is_local: true, using_llm: true,
      ollama_running: true, ollama_models: ["qwen2.5:3b", "llama3.2:3b"], ollama_has_model: true,
    } as T, 300);
    case "ollama_pull": return delay(undefined as T, 1500);
    case "test_cleanup": {
      const t = String(a.text).replace(/\b(um|uh|like,)\s*/gi, "").trim();
      const text = a.style === "code" ? t : t.charAt(0).toUpperCase() + t.slice(1) + (/[.?!]$/.test(t) ? "" : a.style === "casual" ? "" : ".");
      return delay({ text, provider: "Ollama · qwen2.5:3b" } as T, 500);
    }
    case "delete_all_data": history = []; dictionary = []; snippets = []; return delay(undefined as T);
    case "open_permission_settings": case "open_data_folder": case "open_url": case "cancel_recording":
      return delay(undefined as T);
    default:
      console.warn("mock: unknown command", cmd, a);
      return delay(undefined as T);
  }
}

export async function mockListen<T>(event: string, cb: (payload: T) => void) {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(cb as Cb);
  return () => listeners.get(event)?.delete(cb as Cb);
}

// ---- simulated dictation (hold Alt+Space in the browser) ----
let recording = false;
let levelTimer: number | undefined;
const state = (p: Partial<StatePayload>) =>
  emit("murmur://state", { phase: "idle", mode: "dictate", locked: false, message: null, sounds: false, ...p });

const inTauri = !!(window as any).__TAURI__;
window.addEventListener("keydown", (e) => {
  if (inTauri) return;
  if (e.altKey && e.code === "Space" && !recording) {
    e.preventDefault();
    recording = true;
    state({ phase: "recording" });
    levelTimer = window.setInterval(() => emit("murmur://level", Math.random() * 0.12), 50);
  }
});
window.addEventListener("keyup", (e) => {
  if (inTauri) return;
  if (e.code === "Space" && recording) {
    recording = false;
    clearInterval(levelTimer);
    emit("murmur://level", 0);
    state({ phase: "processing" });
    setTimeout(() => {
      const text = "Hey Sarah, I wanted to check if we're still on for lunch on Thursday.";
      const active = document.activeElement as HTMLTextAreaElement | null;
      if (active && active.tagName === "TEXTAREA") active.value += (active.value ? " " : "") + text;
      emit("murmur://dictation", {
        id: null, raw: "um hey sarah so uh I wanted to check if we're still on for like lunch on friday no wait thursday",
        text, app: "Murmur", style: "neutral", mode: "dictate", provider: "Ollama · qwen2.5:3b",
        duration_ms: 5200, process_ms: 820, words: 13,
      });
      state({ phase: "done" });
      setTimeout(() => state({ phase: "idle" }), 900);
    }, 900);
  }
});

// Expose for screenshots.
(window as any).__mockEmit = emit;
