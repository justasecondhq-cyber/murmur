import { useEffect, useRef, useState } from "react";
import { useApp } from "../App";
import { EVENTS, invoke } from "../api";
import { Keys, ShortcutRecorder, useEvent } from "../components";
import type { DictationPayload, ModelList, Permissions, StatePayload } from "../types";
import { ModelPicker } from "./Settings";

const STEPS = ["welcome", "permissions", "shortcut", "model", "practice"] as const;
type Step = (typeof STEPS)[number];

export default function Onboarding({ onDone }: { onDone: () => void }) {
  const { settings, update } = useApp();
  const [step, setStep] = useState<Step>("welcome");
  const [perm, setPerm] = useState<Permissions | null>(null);
  const [models, setModels] = useState<ModelList | null>(null);
  const [result, setResult] = useState<DictationPayload | null>(null);
  const [live, setLive] = useState<StatePayload | null>(null);
  const practiceRef = useRef<HTMLTextAreaElement>(null);

  const idx = STEPS.indexOf(step);
  const next = () => setStep(STEPS[Math.min(idx + 1, STEPS.length - 1)]);
  const back = () => setStep(STEPS[Math.max(idx - 1, 0)]);

  const refreshPerm = () => invoke<Permissions>("get_permissions").then(setPerm);
  const refreshModels = () => invoke<ModelList>("list_models").then(setModels);
  useEffect(() => { refreshPerm(); refreshModels(); }, []);
  useEffect(() => {
    if (step !== "permissions") return;
    const t = setInterval(refreshPerm, 1500);
    return () => clearInterval(t);
  }, [step]);
  useEffect(() => { if (step === "practice") practiceRef.current?.focus(); }, [step]);
  useEvent(EVENTS.modelProgress, () => refreshModels());
  useEvent<DictationPayload>(EVENTS.dictation, setResult);
  useEvent<StatePayload>(EVENTS.state, setLive);

  const finish = async () => {
    await invoke("finish_onboarding");
    onDone();
  };

  const modelReady = models?.models.some((m) => m.id === settings.model_id && m.installed);
  const downloading = (models?.downloading.length ?? 0) > 0;

  return (
    <div className="onb">
      <div className="onb-card">
        <div className="onb-steps" aria-label={`Step ${idx + 1} of ${STEPS.length}`}>
          {STEPS.map((s, i) => <span key={s} className={i <= idx ? "done" : ""} />)}
        </div>

        {step === "welcome" && (
          <>
            <h1>Talk instead of type, in every app.</h1>
            <p className="lede">
              Hold a shortcut, say what you mean, and Murmur types clean, punctuated text where your cursor is. Speech
              recognition runs on this computer. There's no account and nothing to pay.
            </p>
            <div className="onb-body">
              <div className="compare">
                <div><h3>You say</h3>um so can we move the sync to 3, no wait, 4 tomorrow</div>
                <div className="after"><h3>Murmur types</h3>Can we move the sync to 4 tomorrow?</div>
              </div>
            </div>
            <div className="onb-actions"><span /><button className="btn primary" onClick={next}>Set up Murmur</button></div>
          </>
        )}

        {step === "permissions" && (
          <>
            <h1>Let Murmur hear you and type for you.</h1>
            <p className="lede">Murmur only listens while your shortcut is active. It never records keystrokes.</p>
            <div className="onb-body">
              <div className="check">
                <div>
                  <div className="setting-title">Microphone</div>
                  <div className="setting-desc">
                    {perm && perm.microphone_devices > 0
                      ? `${perm.microphone_devices} microphone${perm.microphone_devices > 1 ? "s" : ""} found. ${perm.platform === "macos" ? "macOS will ask for permission the first time you dictate." : ""}`
                      : "No microphone found. Plug one in, or check your sound settings."}
                  </div>
                </div>
                <span className={`check-state ${perm && perm.microphone_devices > 0 ? "ok" : "todo"}`}>{perm && perm.microphone_devices > 0 ? "Ready" : "Missing"}</span>
              </div>
              {perm?.platform === "macos" && (
                <div className="check">
                  <div>
                    <div className="setting-title">Accessibility</div>
                    <div className="setting-desc">Needed to paste into other apps. Turn on Murmur in the list, then come back here.</div>
                  </div>
                  {perm.accessibility ? (
                    <span className="check-state ok">Allowed</span>
                  ) : (
                    <button className="btn small primary" onClick={() => invoke("open_permission_settings", { kind: "accessibility" })}>Open System Settings</button>
                  )}
                </div>
              )}
              {perm?.platform === "linux" && perm.wayland && (
                <div className="callout warn">
                  You're on Wayland. Global shortcuts may not work here; you can start dictation from the tray menu instead, or
                  use an X11 session.
                </div>
              )}
            </div>
            <div className="onb-actions">
              <button className="btn ghost" onClick={back}>Back</button>
              <button className="btn primary" onClick={next}>{perm?.platform === "macos" && !perm.accessibility ? "I'll do this later" : "Continue"}</button>
            </div>
          </>
        )}

        {step === "shortcut" && (
          <>
            <h1>Pick your talk key.</h1>
            <p className="lede">Choose something comfortable to hold with one hand. You can change it any time.</p>
            <div className="onb-body">
              <div className="check">
                <div>
                  <div className="setting-title">Dictate</div>
                  <div className="setting-desc">Hold to talk, release to insert. Tap to go hands-free.</div>
                </div>
                <ShortcutRecorder value={settings.hotkey_dictate} onChange={(v) => update({ hotkey_dictate: v })} />
              </div>
              <div className="check">
                <div>
                  <div className="setting-title">Command mode</div>
                  <div className="setting-desc">Select text, hold this, and say how to change it.</div>
                </div>
                <ShortcutRecorder value={settings.hotkey_command} onChange={(v) => update({ hotkey_command: v })} />
              </div>
            </div>
            <div className="onb-actions">
              <button className="btn ghost" onClick={back}>Back</button>
              <button className="btn primary" onClick={next}>Continue</button>
            </div>
          </>
        )}

        {step === "model" && (
          <>
            <h1>Download a speech model.</h1>
            <p className="lede">
              This is the part that turns your voice into words, entirely on this computer. Base is quick and accurate for most
              people; pick a multilingual model if you speak more than English.
            </p>
            <div className="onb-body"><ModelPicker compact /></div>
            <div className="onb-actions">
              <button className="btn ghost" onClick={back}>Back</button>
              <button className="btn primary" onClick={next} disabled={!modelReady}>
                {modelReady ? "Continue" : downloading ? "Downloading…" : "Download a model to continue"}
              </button>
            </div>
          </>
        )}

        {step === "practice" && (
          <>
            <h1>Try it.</h1>
            <p className="lede">
              Click in the box, hold <Keys shortcut={settings.hotkey_dictate} /> and say something messy, like “um let's meet at
              5, no wait, 6”. Release to see what Murmur makes of it.
            </p>
            <div className="onb-body">
              <textarea ref={practiceRef} className="textarea" placeholder="Your words will appear here" aria-label="Practice area" />
              {live?.phase === "recording" && <div className="live recording"><span className="status-dot rec" />Listening…</div>}
              {live?.phase === "processing" && <div className="live processing"><span className="status-dot on" />Writing it up…</div>}
              {live?.phase === "error" && <div className="callout warn">{live.message}</div>}
              {result && (
                <div className="compare">
                  <div><h3>Heard</h3>{result.raw}</div>
                  <div className="after"><h3>Inserted</h3>{result.text}</div>
                </div>
              )}
            </div>
            <div className="onb-actions">
              <button className="btn ghost" onClick={back}>Back</button>
              <button className="btn primary" onClick={finish}>{result ? "Start using Murmur" : "Skip for now"}</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
