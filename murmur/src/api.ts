// Thin wrapper over Tauri's IPC. Outside Tauri (plain `vite` in a browser)
// it falls back to an in-memory mock so the UI can be developed and previewed.
import { mockInvoke, mockListen } from "./mock";

type Unlisten = () => void;

interface TauriGlobal {
  core: { invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T> };
  event: { listen: <T>(event: string, cb: (e: { payload: T }) => void) => Promise<Unlisten> };
}

const tauri = (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__;
export const isTauri = !!tauri;

export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (tauri) return tauri.core.invoke<T>(cmd, args);
  return mockInvoke<T>(cmd, args);
}

export function listen<T>(event: string, cb: (payload: T) => void): Promise<Unlisten> {
  if (tauri) return tauri.event.listen<T>(event, (e) => cb(e.payload));
  return mockListen<T>(event, cb);
}

export const EVENTS = {
  state: "murmur://state",
  level: "murmur://level",
  dictation: "murmur://dictation",
  data: "murmur://data-changed",
  modelProgress: "murmur://model-progress",
} as const;

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return "Something went wrong.";
}
