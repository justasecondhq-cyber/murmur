import { useEffect, useState } from "react";
import { useApp } from "../App";
import { EVENTS, errorText, invoke } from "../api";
import { ConfirmButton, Icon, Keys, STYLE_LABELS, timeAgo, toast, useEvent } from "../components";
import type { HistoryItem } from "../types";

export default function History() {
  const { settings, go } = useApp();
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [showRaw, setShowRaw] = useState<Set<number>>(new Set());
  const [suggest, setSuggest] = useState<{ id: number; words: string[] } | null>(null);

  const load = () => invoke<HistoryItem[]>("get_history", { query, limit: 500 }).then(setItems);
  useEffect(() => { load(); }, [query]);
  useEvent(EVENTS.data, () => load());

  const copy = async (t: string) => {
    await invoke("copy_text", { text: t });
    toast("Copied");
  };
  const remove = async (id: number) => {
    await invoke("delete_history", { id });
    load();
  };
  const saveEdit = async (id: number) => {
    try {
      const words = await invoke<string[]>("update_history_text", { id, text: draft });
      setEditing(null);
      setSuggest(words.length ? { id, words } : null);
      load();
    } catch (e) {
      toast(errorText(e));
    }
  };
  const addWord = async (w: string) => {
    await invoke("add_dictionary", { word: w, variants: [] });
    setSuggest((s) => (s ? { ...s, words: s.words.filter((x) => x !== w) } : s));
    toast(`Added “${w}” to your dictionary`);
  };
  const clearAll = async () => {
    await invoke("clear_history");
    load();
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>History</h1>
          <p className="lede">
            Everything you've dictated, stored only on this computer. Fix a transcript here and Murmur suggests words to add to
            your dictionary.
          </p>
        </div>
        {items.length > 0 && <ConfirmButton confirmLabel="Click again to delete all" onConfirm={clearAll}>Clear history</ConfirmButton>}
      </div>

      <input className="input" type="text" placeholder="Search text or app" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Search history" />

      {!settings.save_history && (
        <div className="callout warn" style={{ marginTop: 14 }}>
          History is turned off, so new dictations aren't saved.{" "}
          <button className="btn ghost small" onClick={() => go("settings")}>Change in Settings</button>
        </div>
      )}

      <div className="list" style={{ marginTop: 18 }}>
        {items.map((h) => (
          <div className="item" key={h.id}>
            {editing === h.id ? (
              <div style={{ gridColumn: "1 / -1", display: "grid", gap: 8 }}>
                <textarea className="textarea" value={draft} onChange={(e) => setDraft(e.target.value)} autoFocus aria-label="Edit transcript" />
                <div style={{ display: "flex", gap: 8 }}>
                  <button className="btn primary small" onClick={() => saveEdit(h.id)}>Save correction</button>
                  <button className="btn small" onClick={() => setEditing(null)}>Cancel</button>
                </div>
              </div>
            ) : (
              <>
                <div>
                  <div className="item-text">{h.text}</div>
                  {showRaw.has(h.id) && <div className="raw selectable"><span className="faint">{h.mode === "command" ? "You said: " : "Heard: "}</span>{h.raw}</div>}
                </div>
                <div className="item-actions">
                  <button className="btn ghost" title="Copy" aria-label="Copy" onClick={() => copy(h.text)}><Icon.copy /></button>
                  <button className="btn ghost" title="Edit" aria-label="Edit" onClick={() => { setEditing(h.id); setDraft(h.text); }}><Icon.edit /></button>
                  <button
                    className="btn ghost" title="Show what was heard" aria-label="Show what was heard"
                    onClick={() => setShowRaw((s) => { const n = new Set(s); n.has(h.id) ? n.delete(h.id) : n.add(h.id); return n; })}
                  ><Icon.eye /></button>
                  <button className="btn ghost" title="Delete" aria-label="Delete" onClick={() => remove(h.id)}><Icon.trash /></button>
                </div>
              </>
            )}
            {suggest?.id === h.id && suggest.words.length > 0 && (
              <div className="callout" style={{ gridColumn: "1 / -1", alignItems: "center", flexWrap: "wrap" }}>
                Add to dictionary so Murmur gets it right next time:
                {suggest.words.map((w) => (
                  <button key={w} className="btn small" onClick={() => addWord(w)}>{w}</button>
                ))}
              </div>
            )}
            <div className="item-meta">
              <span>{timeAgo(h.created_at)}</span>
              {h.app && <span>{h.app}</span>}
              <span className={`chip ${h.style}`}>{STYLE_LABELS[h.style] ?? h.style}</span>
              <span className="num">{h.words} words</span>
              <span className="num">{(h.duration_ms / 1000).toFixed(1)} s spoken</span>
            </div>
          </div>
        ))}
      </div>

      {items.length === 0 && (
        <div className="empty">
          <strong>{query ? "No matches" : "No history yet"}</strong>
          {query ? "Try a different word or app name." : <>Hold <Keys shortcut={settings.hotkey_dictate} /> in any app to dictate.</>}
        </div>
      )}
    </div>
  );
}
