// The floating listening bar. Its window is always "shown" and click-through;
// while idle it draws nothing, so it never steals focus from your app.
import { StrictMode, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { EVENTS, listen } from "./api";
import type { StatePayload } from "./types";
import "./overlay.css";

const BARS = 22;

let audioCtx: AudioContext | null = null;
function tick(freq: number) {
  try {
    audioCtx ??= new AudioContext();
    const t = audioCtx.currentTime;
    const osc = audioCtx.createOscillator();
    const gain = audioCtx.createGain();
    osc.type = "sine";
    osc.frequency.setValueAtTime(freq, t);
    gain.gain.setValueAtTime(0.0001, t);
    gain.gain.exponentialRampToValueAtTime(0.06, t + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.12);
    osc.connect(gain).connect(audioCtx.destination);
    osc.start(t);
    osc.stop(t + 0.14);
  } catch {
    /* audio unavailable */
  }
}

function Overlay() {
  const [state, setState] = useState<StatePayload | null>(null);
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const prevPhase = useRef("idle");
  const hideTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const offs: Promise<() => void>[] = [];
    offs.push(
      listen<StatePayload>(EVENTS.state, (s) => {
        window.clearTimeout(hideTimer.current);
        if (s.sounds) {
          if (s.phase === "recording" && prevPhase.current !== "recording") tick(880);
          if (s.phase === "processing" && prevPhase.current === "recording") tick(620);
        }
        prevPhase.current = s.phase;
        setState(s);
        if (s.phase === "done") hideTimer.current = window.setTimeout(() => setState(null), 700);
        if (s.phase === "error" || (s.phase === "idle" && s.message)) hideTimer.current = window.setTimeout(() => setState(null), 3200);
        if (s.phase === "idle" && !s.message) setState(null);
      }),
    );
    offs.push(
      listen<number>(EVENTS.level, (l) => {
        // Speech RMS is usually 0.01–0.2; map onto 0..1 with a soft curve.
        const v = Math.min(1, Math.sqrt(Math.max(0, l) * 12));
        setLevels((cur) => [...cur.slice(1), v]);
      }),
    );
    return () => { offs.forEach((p) => p.then((off) => off())); };
  }, []);

  const phase = state?.phase ?? "idle";
  const visible = !!state && (phase !== "idle" || !!state.message);
  const command = state?.mode === "command";

  return (
    <div className={`pill ${phase}${visible ? " show" : ""}`} role="status" aria-live="polite">
      {phase === "recording" && (
        <>
          <span className={`dot${command ? " cmd" : ""}`} />
          <div className="wave" aria-hidden="true">
            {levels.map((v, i) => <span key={i} style={{ transform: `scaleY(${0.12 + v * 0.88})` }} />)}
          </div>
          <span className="label">{state?.locked ? "Hands-free" : command ? "Command" : "Esc"}</span>
        </>
      )}
      {phase === "processing" && (
        <>
          <div className="dots" aria-hidden="true"><span /><span /><span /></div>
          <span className="label wide">{command ? "Working on it" : "Writing"}</span>
        </>
      )}
      {phase === "done" && <span className="label wide">Inserted</span>}
      {(phase === "error" || (phase === "idle" && state?.message)) && <span className="label wide msg">{state?.message}</span>}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Overlay />
  </StrictMode>,
);
