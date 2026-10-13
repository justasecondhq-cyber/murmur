import { useEffect, useState, type FormEvent } from "react";
import { errorText, invoke } from "../api";
import { Icon, toast } from "../components";
import type { SnippetRow } from "../types";

export default function Snippets() {
  const [rows, setRows] = useState<SnippetRow[]>([]);
  const [editId, setEditId] = useState<number | null>(null);
  const [trigger, setTrigger] = useState("");
  const [expansion, setExpansion] = useState("");
  const [error, setError] = useState("");

  const load = () => invoke<SnippetRow[]>("list_snippets").then(setRows);
  useEffect(() => { load(); }, []);

  const reset = () => { setEditId(null); setTrigger(""); setExpansion(""); setError(""); };
  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError("");
    try {
      await invoke("save_snippet", { id: editId, trigger, expansion });
      toast(editId ? "Snippet updated" : "Snippet added");
      reset();
      load();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const remove = async (id: number) => {
    await invoke("delete_snippet", { id });
    if (editId === id) reset();
    load();
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Snippets</h1>
          <p className="lede">
            Say a short phrase and Murmur types the full text: links, addresses, sign-offs, boilerplate. Snippets work in the
            middle of a sentence too.
          </p>
        </div>
      </div>

      <form className="row-form two" onSubmit={save} style={{ gridTemplateColumns: "1fr" }}>
        <div className="field">
          <label htmlFor="st">When I say</label>
          <input id="st" className="input" type="text" placeholder="my calendar link" value={trigger} onChange={(e) => setTrigger(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="se">Type this</label>
          <textarea id="se" className="textarea" style={{ minHeight: 72 }} placeholder="https://cal.com/you/30min" value={expansion} onChange={(e) => setExpansion(e.target.value)} />
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button className="btn primary" type="submit" disabled={!trigger.trim() || !expansion.trim()}>{editId ? "Save snippet" : "Add snippet"}</button>
          {editId && <button className="btn" type="button" onClick={reset}>Cancel</button>}
          {error && <span className="error-text">{error}</span>}
        </div>
      </form>

      {rows.length === 0 ? (
        <div className="empty">
          <strong>No snippets yet</strong>
          Try “my calendar link” for your booking page, or “my address” for where you live.
        </div>
      ) : (
        <div className="list">
          {rows.map((r) => (
            <div className="item" key={r.id}>
              <div>
                <div style={{ fontWeight: 600 }}>“{r.trigger}”</div>
                <div className="item-text muted small">{r.expansion}</div>
              </div>
              <div className="item-actions">
                <button className="btn ghost" title="Edit" aria-label={`Edit ${r.trigger}`} onClick={() => { setEditId(r.id); setTrigger(r.trigger); setExpansion(r.expansion); }}><Icon.edit /></button>
                <button className="btn ghost" title="Delete" aria-label={`Delete ${r.trigger}`} onClick={() => remove(r.id)}><Icon.trash /></button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
