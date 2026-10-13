import { useEffect, useState } from "react";
import { useApp } from "../App";
import { EVENTS, errorText, invoke } from "../api";
import { ConfirmButton, Segmented, Setting, ShortcutRecorder, Toggle, toast, useEvent } from "../components";
import type { AppInfo, DownloadProgress, ModelList, Permissions, Settings } from "../types";

export const LANGUAGES: [string, string][] = [
  ["auto", "Detect automatically"], ["en", "English"], ["fil", "Filipino / Tagalog"], ["es", "Spanish"], ["fr", "French"],
  ["de", "German"], ["it", "Italian"], ["pt", "Portuguese"], ["nl", "Dutch"], ["pl", "Polish"], ["ru", "Russian"],
  ["uk", "Ukrainian"], ["tr", "Turkish"], ["ar", "Arabic"], ["hi", "Hindi"], ["id", "Indonesian"], ["ms", "Malay"],
  ["vi", "Vietnamese"], ["th", "Thai"], ["ja", "Japanese"], ["ko", "Korean"], ["zh", "Chinese"],
];

/** Speech model picker with download progress. Shared with onboarding. */
export function ModelPicker({ compact }: { compact?: boolean }) {
  const { settings, update } = useApp();
  const [list, setList] = useState<ModelList | null>(null);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const load = () => invoke<ModelList>("list_models").then(setList);
  useEffect(() => { load(); }, []);
  useEvent<DownloadProgress>(EVENTS.modelProgress, (p) => {
    setProgress((cur) => ({ ...cur, [p.id]: p }));
    if (p.done || p.error) {
      load();
      if (p.error) toast(p.error);
    }
  });

  const download = async (id: string) => {
    try {
      await invoke("download_model", { id });
      setList((l) => (l ? { ...l, downloading: [...l.downloading, id] } : l));
      if (!list?.models.some((m) => m.id === settings.model_id && m.installed)) update({ model_id: id });
    } catch (e) {
      toast(errorText(e));
    }
  };
  const remove = async (id: string) => {
    await invoke("delete_model", { id });
    load();
  };

  const models = (list?.models ?? []).filter((m) => !compact || ["base.en", "base", "small.en", "large-v3-turbo-q5_0"].includes(m.id));
  return (
    <div className="rows">
      {models.map((m) => {
        const busy = list?.downloading.includes(m.id) && !progress[m.id]?.done;
        const p = progress[m.id];
        const pct = p && p.total ? Math.min(100, (p.downloaded / p.total) * 100) : 0;
        return (
          <div className="model" key={m.id}>
            <input
              type="radio" name="model" aria-label={`Use ${m.name}`} checked={settings.model_id === m.id}
              disabled={!m.installed} onChange={() => update({ model_id: m.id })}
            />
            <div>
              <span className="model-name">{m.name}</span>
              {settings.model_id === m.id && m.installed && <span className="chip casual" style={{ marginLeft: 8 }}>In use</span>}
            </div>
            <div>
              {m.installed ? (
                settings.model_id === m.id ? null : (
                  <ConfirmButton className="btn ghost small" confirmLabel="Delete file?" onConfirm={() => remove(m.id)}>Delete</ConfirmButton>
                )
              ) : busy ? (
                <span className="small muted num">{Math.round(pct)}%</span>
              ) : (
                <button className="btn small" onClick={() => download(m.id)}>Download {m.size_mb >= 1000 ? `${(m.size_mb / 1024).toFixed(1)} GB` : `${m.size_mb} MB`}</button>
              )}
            </div>
            <div className="model-meta">
              <span>{m.speed}</span><span>{m.accuracy} accuracy</span><span>{m.english_only ? "English only" : "99 languages"}</span><span>{m.note}</span>
            </div>
            {busy && <div className="progress"><div style={{ width: `${pct}%` }} /></div>}
          </div>
        );
      })}
    </div>
  );
}

function AiSettings() {
  const { settings, update, llm, refreshLlm } = useApp();
  const [pulling, setPulling] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const pull = async () => {
    setPulling(true);
    try {
      await invoke("ollama_pull", { model: settings.ollama_model });
      toast(`${settings.ollama_model} is ready`);
      refreshLlm();
    } catch (e) {
      toast(errorText(e));
    } finally {
      setPulling(false);
    }
  };
  const set = (patch: Partial<Settings>) => update(patch).then(refreshLlm);
  const p = settings.llm_provider;

  return (
    <div className="group">
      <h2>AI clean-up</h2>
      <p className="lede">
        Fillers and corrections are always handled by Murmur's built-in rules. A language model adds smarter punctuation,
        rephrased corrections and command mode. Local models never send your words anywhere.
      </p>
      <div className="rows">
        <Setting title="Clean up my speech" desc="Turn off to insert exactly what was heard (raw mode).">
          <Toggle label="Clean up my speech" checked={settings.cleanup_enabled} onChange={(v) => set({ cleanup_enabled: v })} />
        </Setting>
        <Setting title="Language model" stack desc={llm ? <>Now using: <strong>{llm.active}</strong></> : "Checking…"}>
          <Segmented
            label="Language model"
            value={p}
            onChange={(v) => set({ llm_provider: v })}
            options={[
              { value: "auto", label: "Automatic" },
              { value: "ollama", label: "Ollama" },
              { value: "openai", label: "OpenAI-compatible" },
              { value: "anthropic", label: "Anthropic" },
              { value: "none", label: "Rules only" },
            ]}
          />
        </Setting>

        {(p === "auto" || p === "ollama") && (
          <>
            <Setting
              title="Ollama"
              desc={
                llm?.ollama_running
                  ? llm.ollama_has_model
                    ? `Running, with ${settings.ollama_model} installed.`
                    : `Running, but ${settings.ollama_model} isn't downloaded yet.`
                  : "Not running. Install it free from ollama.com, then come back here."
              }
            >
              {!llm?.ollama_running && <button className="btn small" onClick={() => invoke("open_url", { url: "https://ollama.com/download" })}>Get Ollama</button>}
              {llm?.ollama_running && !llm.ollama_has_model && (
                <button className="btn small primary" onClick={pull} disabled={pulling}>{pulling ? "Downloading…" : `Download ${settings.ollama_model}`}</button>
              )}
              <button className="btn ghost small" onClick={refreshLlm}>Check again</button>
            </Setting>
            <Setting title="Ollama model" desc="Small instruct models (2–4B) are fast enough for dictation. qwen2.5:3b and llama3.2:3b work well.">
              <input className="input" style={{ width: 180 }} type="text" value={settings.ollama_model} onChange={(e) => update({ ollama_model: e.target.value })} onBlur={refreshLlm} aria-label="Ollama model" list="ollama-models" />
              <datalist id="ollama-models">{llm?.ollama_models.map((m) => <option key={m} value={m} />)}</datalist>
            </Setting>
            <Setting title="Ollama address">
              <input className="input" style={{ width: 220 }} type="text" value={settings.ollama_url} onChange={(e) => update({ ollama_url: e.target.value })} onBlur={refreshLlm} aria-label="Ollama address" />
            </Setting>
          </>
        )}

        {p === "openai" && (
          <>
            <Setting title="Base URL" desc="Works with OpenAI, Groq, OpenRouter, LM Studio (http://localhost:1234/v1) and others.">
              <input className="input" style={{ width: 260 }} type="text" value={settings.openai_base_url} onChange={(e) => update({ openai_base_url: e.target.value })} aria-label="Base URL" />
            </Setting>
            <Setting title="API key" desc="Stored only on this computer. Leave empty for local servers.">
              <input className="input" style={{ width: 220 }} type={showKey ? "text" : "password"} value={settings.openai_api_key} onChange={(e) => update({ openai_api_key: e.target.value })} aria-label="API key" />
              <button className="btn ghost small" onClick={() => setShowKey((s) => !s)}>{showKey ? "Hide" : "Show"}</button>
            </Setting>
            <Setting title="Model">
              <input className="input" style={{ width: 220 }} type="text" value={settings.openai_model} onChange={(e) => update({ openai_model: e.target.value })} onBlur={refreshLlm} aria-label="Model" />
            </Setting>
          </>
        )}

        {p === "anthropic" && (
          <>
            <Setting title="API key" desc="From console.anthropic.com. Stored only on this computer.">
              <input className="input" style={{ width: 220 }} type={showKey ? "text" : "password"} value={settings.anthropic_api_key} onChange={(e) => update({ anthropic_api_key: e.target.value })} onBlur={refreshLlm} aria-label="Anthropic API key" />
              <button className="btn ghost small" onClick={() => setShowKey((s) => !s)}>{showKey ? "Hide" : "Show"}</button>
            </Setting>
            <Setting title="Model">
              <input className="input" style={{ width: 220 }} type="text" value={settings.anthropic_model} onChange={(e) => update({ anthropic_model: e.target.value })} aria-label="Anthropic model" />
            </Setting>
          </>
        )}

        {llm && !llm.active_is_local && (
          <div className="setting"><div className="callout warn" style={{ width: "100%" }}>Your transcripts are sent to {llm.active.split(" ·")[0]} for clean-up. Choose Ollama or Rules only to keep everything on this computer.</div></div>
        )}
        <Setting title="Spoken punctuation" desc="Say “comma”, “period” or “colon” to type them. “New line”, “new paragraph” and “question mark” always work.">
          <Toggle label="Spoken punctuation" checked={settings.spoken_punctuation} onChange={(v) => update({ spoken_punctuation: v })} />
        </Setting>
      </div>
    </div>
  );
}

export default function SettingsPage() {
  const { settings, update } = useApp();
  const [devices, setDevices] = useState<string[]>([]);
  const [perm, setPerm] = useState<Permissions | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    invoke<string[]>("list_input_devices").then(setDevices);
    invoke<Permissions>("get_permissions").then(setPerm);
    invoke<AppInfo>("get_app_info").then(setInfo);
  }, []);

  const wipe = async () => {
    await invoke("delete_all_data", { includeModels: false });
    toast("History, dictionary, snippets and stats deleted");
  };

  return (
    <div className="page">
      <div className="page-head"><div><h1>Settings</h1></div></div>

      <div className="group">
        <h2>Shortcuts</h2>
        <p className="lede">Click a shortcut, then press the new key combination.</p>
        <div className="rows">
          <Setting title="Dictate" desc="Hold to talk, release to insert. Tap once to keep listening hands-free.">
            <ShortcutRecorder value={settings.hotkey_dictate} onChange={(v) => update({ hotkey_dictate: v })} />
          </Setting>
          <Setting title="Command mode" desc="Select text, hold this and say what to do: “make it shorter”, “translate to Spanish”, “turn into bullet points”.">
            <ShortcutRecorder value={settings.hotkey_command} onChange={(v) => update({ hotkey_command: v })} />
          </Setting>
          <Setting title="How the shortcut works">
            <Segmented label="How the shortcut works" value={settings.hotkey_mode} onChange={(v) => update({ hotkey_mode: v })}
              options={[{ value: "hold", label: "Hold to talk" }, { value: "toggle", label: "Press to start and stop" }]} />
          </Setting>
        </div>
        {perm?.wayland && (
          <div className="callout warn" style={{ marginTop: 10 }}>
            You're on Wayland, where apps can't register global shortcuts directly. Bind a shortcut in your desktop settings to
            the tray menu's “Start / stop hands-free dictation”, or log in with an X11 session.
          </div>
        )}
      </div>

      <div className="group">
        <h2>Microphone</h2>
        <div className="rows">
          <Setting title="Input">
            <select className="select" style={{ width: 240 }} value={settings.input_device ?? ""} onChange={(e) => update({ input_device: e.target.value || null })} aria-label="Microphone">
              <option value="">System default</option>
              {devices.map((d) => <option key={d} value={d}>{d}</option>)}
            </select>
          </Setting>
          <Setting title="Whisper mode" desc="Boosts quiet speech so you can dictate softly in an office or library.">
            <Toggle label="Whisper mode" checked={settings.whisper_mode} onChange={(v) => update({ whisper_mode: v })} />
          </Setting>
          {perm?.platform === "macos" && (
            <Setting title="Permissions" desc="Murmur needs Microphone access to hear you and Accessibility access to paste into other apps.">
              <button className="btn small" onClick={() => invoke("open_permission_settings", { kind: "microphone" })}>Microphone</button>
              <button className="btn small" onClick={() => invoke("open_permission_settings", { kind: "accessibility" })}>
                Accessibility{perm.accessibility === false ? " (needed)" : ""}
              </button>
            </Setting>
          )}
        </div>
      </div>

      <div className="group">
        <h2>Speech model</h2>
        <p className="lede">Runs on your computer with whisper.cpp. Bigger models are more accurate and a little slower.</p>
        <ModelPicker />
        <div className="rows" style={{ marginTop: 12 }}>
          <Setting title="Language" desc="Automatic works for most people, including mixed-language speech. Needs a multilingual model.">
            <select className="select" style={{ width: 220 }} value={settings.language} onChange={(e) => update({ language: e.target.value })} aria-label="Language">
              {LANGUAGES.map(([code, name]) => <option key={code} value={code}>{name}</option>)}
            </select>
          </Setting>
        </div>
      </div>

      <AiSettings />

      <div className="group">
        <h2>While dictating</h2>
        <div className="rows">
          <Setting title="Show the listening bar" desc="A small bar near the bottom of the screen with a live waveform.">
            <Toggle label="Show the listening bar" checked={settings.show_overlay} onChange={(v) => update({ show_overlay: v })} />
          </Setting>
          <Setting title="Sounds" desc="A soft tick when Murmur starts and stops listening.">
            <Toggle label="Sounds" checked={settings.play_sounds} onChange={(v) => update({ play_sounds: v })} />
          </Setting>
          <Setting title="Keep my clipboard" desc="Murmur pastes through the clipboard, then puts back what was there before.">
            <Toggle label="Keep my clipboard" checked={settings.restore_clipboard} onChange={(v) => update({ restore_clipboard: v })} />
          </Setting>
          <Setting title="Your typing speed" desc="Used to estimate time saved.">
            <input className="input num" style={{ width: 90 }} type="number" min={10} max={200} value={settings.typing_wpm} onChange={(e) => update({ typing_wpm: Number(e.target.value) || 40 })} aria-label="Typing speed in words per minute" />
            <span className="muted small">wpm</span>
          </Setting>
        </div>
      </div>

      <div className="group">
        <h2>Privacy</h2>
        <p className="lede">
          Murmur has no account, no analytics and no servers. It only listens while your shortcut is active and never records
          keystrokes. Audio is discarded right after transcription.
        </p>
        <div className="rows">
          <Setting title="Save history" desc="Keep transcripts so you can search, copy and correct them. Word counts for stats are kept either way.">
            <Toggle label="Save history" checked={settings.save_history} onChange={(v) => update({ save_history: v })} />
          </Setting>
          <Setting title="Delete history older than">
            <select className="select" style={{ width: 160 }} value={settings.history_retention_days} onChange={(e) => update({ history_retention_days: Number(e.target.value) })} aria-label="History retention">
              <option value={0}>Never</option><option value={1}>1 day</option><option value={7}>7 days</option><option value={30}>30 days</option><option value={90}>90 days</option>
            </select>
          </Setting>
          <Setting title="Your data folder" desc={<span className="selectable code">{info?.data_dir}</span>}>
            <button className="btn small" onClick={() => invoke("open_data_folder")}>Open</button>
          </Setting>
          <Setting title="Delete everything" desc="Erases history, dictionary, snippets, stats and settings. Downloaded models stay.">
            <ConfirmButton confirmLabel="Click again to erase" onConfirm={wipe}>Delete everything</ConfirmButton>
          </Setting>
        </div>
      </div>

      <div className="group">
        <h2>Developers</h2>
        <div className="rows">
          <Setting title="Local API" desc="Lets scripts and other apps use Murmur's transcription. Only reachable from this computer.">
            <Toggle label="Local API" checked={settings.api_enabled} onChange={(v) => update({ api_enabled: v })} />
          </Setting>
          {settings.api_enabled && (
            <>
              <Setting title="Port">
                <input className="input num" style={{ width: 100 }} type="number" value={settings.api_port} onChange={(e) => update({ api_port: Number(e.target.value) || 7071 })} aria-label="API port" />
              </Setting>
              <Setting stack title="Example">
                <div className="codeblock">{`curl -X POST --data-binary @note.wav "http://127.0.0.1:${settings.api_port}/v1/transcribe?style=formal"\n\ncurl -X POST -d '{"text":"um so yeah lets ship it","style":"casual"}' http://127.0.0.1:${settings.api_port}/v1/clean`}</div>
              </Setting>
            </>
          )}
        </div>
      </div>

      <p className="small faint">
        Murmur {info?.version} is free and open source under the MIT license.{" "}
        <button className="btn ghost small" onClick={() => invoke("open_url", { url: "https://github.com/ggerganov/whisper.cpp" })}>Powered by whisper.cpp</button>
      </p>
    </div>
  );
}
