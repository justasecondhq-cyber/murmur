import { useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "./api";

// ---------------- platform & shortcuts ----------------

export const isMac = /Mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);

const KEY_LABELS: Record<string, { sym?: string; label: string; macSym?: string; macLabel?: string }> = {
  alt: { label: "Alt", macSym: "⌥", macLabel: "Option" },
  option: { label: "Alt", macSym: "⌥", macLabel: "Option" },
  shift: { label: "Shift", sym: "⇧" },
  control: { label: "Ctrl", macSym: "⌃", macLabel: "Control" },
  ctrl: { label: "Ctrl", macSym: "⌃", macLabel: "Control" },
  super: { label: "Win", macSym: "⌘", macLabel: "Cmd" },
  cmd: { label: "Win", macSym: "⌘", macLabel: "Cmd" },
  command: { label: "Win", macSym: "⌘", macLabel: "Cmd" },
  commandorcontrol: { label: "Ctrl", macSym: "⌘", macLabel: "Cmd" },
  cmdorctrl: { label: "Ctrl", macSym: "⌘", macLabel: "Cmd" },
  space: { label: "Space" },
  escape: { label: "Esc" },
  enter: { label: "Enter", sym: "↵" },
};

export function keyParts(shortcut: string): { sym?: string; label: string; wide?: boolean }[] {
  return shortcut
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => {
      const k = KEY_LABELS[p.toLowerCase()];
      if (!k) return { label: p.replace(/^Key/, "").replace(/^Digit/, "") };
      return {
        sym: isMac ? k.macSym ?? k.sym : k.sym,
        label: isMac ? k.macLabel ?? k.label : k.label,
        wide: p.toLowerCase() === "space",
      };
    });
}

export function Keys({ shortcut, big, pressed }: { shortcut: string; big?: boolean; pressed?: boolean }) {
  const parts = keyParts(shortcut);
  return (
    <span className={`keys${big ? " big" : ""}${pressed ? " pressed" : ""}`} aria-label={shortcut}>
      {parts.map((p, i) => (
        <span key={i} className={`key${p.wide ? " wide" : ""}`}>
          {p.sym && <span className="sym">{p.sym}</span>}
          {p.label}
        </span>
      ))}
    </span>
  );
}

/** Click, then press a key combination. Produces strings like "Alt+Shift+Space". */
export function ShortcutRecorder({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [listening, setListening] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!listening) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") { setListening(false); return; }
      if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;
      const mods: string[] = [];
      if (e.ctrlKey) mods.push("Control");
      if (e.altKey) mods.push("Alt");
      if (e.shiftKey) mods.push("Shift");
      if (e.metaKey) mods.push("Super");
      let key = e.code;
      if (key.startsWith("Key")) key = key.slice(3);
      else if (key.startsWith("Digit")) key = key.slice(5);
      if (mods.length === 0 && !/^F\d+$/.test(key)) return; // need a modifier unless it's a function key
      onChange([...mods, key].join("+"));
      setListening(false);
      ref.current?.blur();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [listening, onChange]);
  return (
    <button ref={ref} type="button" className={`recorder${listening ? " listening" : ""}`} onClick={() => setListening(true)} onBlur={() => setListening(false)}>
      {listening ? "Press a key combination…" : value ? <Keys shortcut={value} /> : <span className="faint">Not set</span>}
    </button>
  );
}

// ---------------- controls ----------------

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <span className="toggle">
      <input type="checkbox" role="switch" aria-label={label} checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span />
    </span>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: {
  value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; label: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={value === o.value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Setting({ title, desc, children, stack }: { title: ReactNode; desc?: ReactNode; children?: ReactNode; stack?: boolean }) {
  return (
    <div className={`setting${stack ? " stack" : ""}`}>
      <div className="setting-text">
        <div className="setting-title">{title}</div>
        {desc && <div className="setting-desc">{desc}</div>}
      </div>
      {children && <div className="setting-control">{children}</div>}
    </div>
  );
}

// ---------------- hooks ----------------

export function useEvent<T>(event: string, cb: (payload: T) => void) {
  const saved = useRef(cb);
  saved.current = cb;
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    listen<T>(event, (p) => saved.current(p)).then((u) => (cancelled ? u() : (off = u)));
    return () => { cancelled = true; off?.(); };
  }, [event]);
}

let toastSetter: ((m: string | null) => void) | null = null;
export function toast(message: string) {
  toastSetter?.(message);
}
export function ToastHost() {
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    toastSetter = setMsg;
    return () => { toastSetter = null; };
  }, []);
  useEffect(() => {
    if (!msg) return;
    const t = setTimeout(() => setMsg(null), 2600);
    return () => clearTimeout(t);
  }, [msg]);
  return msg ? <div className="toast" role="status">{msg}</div> : null;
}

// ---------------- formatting ----------------

export function timeAgo(unix: number): string {
  const s = Math.max(0, Date.now() / 1000 - unix);
  if (s < 60) return "Just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = new Date(unix * 1000);
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" }) + ", " + d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

export function formatNumber(n: number): string {
  return new Intl.NumberFormat().format(Math.round(n));
}

export function formatMinutes(m: number): string {
  if (m < 60) return `${Math.round(m)} min`;
  const h = m / 60;
  return h < 10 ? `${h.toFixed(1)} h` : `${Math.round(h)} h`;
}

export const STYLE_LABELS: Record<string, string> = {
  neutral: "Standard",
  casual: "Casual",
  formal: "Formal",
  code: "Code",
  command: "Command",
};

// ---------------- icons ----------------

const I = ({ d, size = 17 }: { d: string; size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d={d} />
  </svg>
);
export const Icon = {
  home: () => <I d="M4 10.5 12 4l8 6.5V20a1 1 0 0 1-1 1h-5v-6h-4v6H5a1 1 0 0 1-1-1z" />,
  history: () => <I d="M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5M12 7v5l3 3" />,
  book: () => <I d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2zm0 0v16M8 7h7" />,
  bolt: () => <I d="M13 2 4 14h7l-1 8 9-12h-7z" />,
  palette: () => <I d="M12 3a9 9 0 0 0 0 18c1.1 0 1.6-.9 1.2-1.8-.5-1.1.2-2.2 1.4-2.2H17a4 4 0 0 0 4-4c0-5.5-4-10-9-10zM7.5 11.5h.01M10 7.5h.01M15 7.5h.01" />,
  gear: () => <I d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm7.4-3a7.4 7.4 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7.5 7.5 0 0 0-2-1.2L14.5 3h-5l-.4 2.6a7.5 7.5 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7.4 7.4 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7.5 7.5 0 0 0 2 1.2l.4 2.6h5l.4-2.6a7.5 7.5 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2z" />,
  copy: () => <I size={15} d="M9 9h10v12H9zM5 15V3h10" />,
  trash: () => <I size={15} d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />,
  edit: () => <I size={15} d="M4 20h4L19 9l-4-4L4 16zM14 6l4 4" />,
  eye: () => <I size={15} d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zm10 3a3 3 0 1 0 0-6 3 3 0 0 0 0 6z" />,
  lock: () => <I size={14} d="M6 11h12v10H6zM8 11V7a4 4 0 1 1 8 0v4" />,
  mark: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true" fill="#fff">
      <rect x="3" y="10" width="2.6" height="5" rx="1.3" />
      <rect x="7.5" y="7" width="2.6" height="11" rx="1.3" />
      <rect x="12" y="4" width="2.6" height="17" rx="1.3" />
      <rect x="16.5" y="8.5" width="2.6" height="8" rx="1.3" />
    </svg>
  ),
};

/** A button that asks for a second click instead of a browser confirm() dialog
 *  (native dialogs aren't reliable inside every desktop webview). */
export function ConfirmButton({ children, confirmLabel, onConfirm, className = "btn danger" }: {
  children: ReactNode; confirmLabel: string; onConfirm: () => void; className?: string;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(t);
  }, [armed]);
  return (
    <button className={`${className}${armed ? " solid" : ""}`} onClick={() => (armed ? (setArmed(false), onConfirm()) : setArmed(true))}>
      {armed ? confirmLabel : children}
    </button>
  );
}
