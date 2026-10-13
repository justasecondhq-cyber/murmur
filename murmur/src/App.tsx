import { createContext, useCallback, useContext, useEffect, useState, type ComponentType } from "react";
import { EVENTS, errorText, invoke } from "./api";
import { Icon, ToastHost, toast, useEvent } from "./components";
import type { LlmStatus, Settings, StatePayload } from "./types";
import Home from "./pages/Home";
import History from "./pages/History";
import Dictionary from "./pages/Dictionary";
import Snippets from "./pages/Snippets";
import Styles from "./pages/Styles";
import SettingsPage from "./pages/Settings";
import Onboarding from "./pages/Onboarding";

type Page = "home" | "history" | "dictionary" | "snippets" | "styles" | "settings";

interface Ctx {
  settings: Settings;
  update: (patch: Partial<Settings>) => Promise<void>;
  live: StatePayload | null;
  llm: LlmStatus | null;
  refreshLlm: () => void;
  go: (p: Page) => void;
}
const SettingsCtx = createContext<Ctx>(null as unknown as Ctx);
export const useApp = () => useContext(SettingsCtx);

const NAV: { id: Page; label: string; icon: ComponentType }[] = [
  { id: "home", label: "Home", icon: Icon.home },
  { id: "history", label: "History", icon: Icon.history },
  { id: "dictionary", label: "Dictionary", icon: Icon.book },
  { id: "snippets", label: "Snippets", icon: Icon.bolt },
  { id: "styles", label: "Styles", icon: Icon.palette },
  { id: "settings", label: "Settings", icon: Icon.gear },
];

export default function App() {
  const initialPage = (new URLSearchParams(location.search).get("page") as Page) || "home";
  const [page, setPage] = useState<Page>(initialPage);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [live, setLive] = useState<StatePayload | null>(null);
  const [llm, setLlm] = useState<LlmStatus | null>(null);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setSettings);
  }, []);

  const refreshLlm = useCallback(() => {
    invoke<LlmStatus>("llm_status").then(setLlm).catch(() => setLlm(null));
  }, []);
  useEffect(refreshLlm, [refreshLlm]);

  useEvent<StatePayload>(EVENTS.state, (p) => setLive(p));

  const update = useCallback(async (patch: Partial<Settings>) => {
    setSettings((cur) => (cur ? { ...cur, ...patch } : cur));
    try {
      const current = await invoke<Settings>("get_settings");
      const res = await invoke<{ warnings: string[] }>("save_settings", { settings: { ...current, ...patch } });
      if (res?.warnings?.length) toast(res.warnings.join(" "));
    } catch (e) {
      toast(errorText(e));
      invoke<Settings>("get_settings").then(setSettings);
    }
  }, []);

  if (!settings) return null;

  if (!settings.onboarded) {
    return (
      <SettingsCtx.Provider value={{ settings, update, live, llm, refreshLlm, go: setPage }}>
        <Onboarding onDone={() => invoke<Settings>("get_settings").then(setSettings)} />
        <ToastHost />
      </SettingsCtx.Provider>
    );
  }

  const phase = live?.phase ?? "idle";
  return (
    <SettingsCtx.Provider value={{ settings, update, live, llm, refreshLlm, go: setPage }}>
      <div className="shell">
        <nav className="sidebar" aria-label="Sections">
          <div className="brand">
            <span className="brand-mark"><Icon.mark /></span>
            <span>Murmur</span>
          </div>
          {NAV.map((n) => (
            <button key={n.id} className="nav-item" aria-current={page === n.id ? "page" : undefined} onClick={() => setPage(n.id)}>
              <n.icon />
              <span>{n.label}</span>
            </button>
          ))}
          <div className="sidebar-foot">
            <div>
              <span className={`status-dot ${phase === "recording" ? "rec" : "on"}`} />
              {phase === "recording" ? "Listening" : phase === "processing" ? "Writing" : "Ready"}
            </div>
            <div title={llm?.active}>
              <span className={`status-dot ${llm?.using_llm ? "on" : "warn"}`} />
              {llm?.using_llm ? (llm.active_is_local ? "AI clean-up on device" : "AI clean-up in cloud") : "Rule-based clean-up"}
            </div>
          </div>
        </nav>
        <main className="main">
          {page === "home" && <Home />}
          {page === "history" && <History />}
          {page === "dictionary" && <Dictionary />}
          {page === "snippets" && <Snippets />}
          {page === "styles" && <Styles />}
          {page === "settings" && <SettingsPage />}
        </main>
      </div>
      <ToastHost />
    </SettingsCtx.Provider>
  );
}
