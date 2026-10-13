import { useEffect, useState, type FormEvent } from "react";
import { errorText, invoke } from "../api";
import { Icon, toast } from "../components";
import type { DictionaryRow } from "../types";

export default function Dictionary() {
  const [rows, setRows] = useState<DictionaryRow[]>([]);
  const [word, setWord] = useState("");
  const [variants, setVariants] = useState("");
  const [error, setError] = useState("");

  const load = () => invoke<DictionaryRow[]>("list_dictionary").then(setRows);
  useEffect(() => { load(); }, []);

  const add = async (e: FormEvent) => {
    e.preventDefault();
    setError("");
    try {
      await invoke("add_dictionary", {
        word: word.trim(),
        variants: variants.split(",").map((v) => v.trim()).filter(Boolean),
      });
      setWord("");
      setVariants("");
      load();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const remove = async (id: number) => {
    await invoke("delete_dictionary", { id });
    load();
    toast("Removed");
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Dictionary</h1>
          <p className="lede">
            Names, products and jargon Murmur should always spell your way. They also steer the speech model toward hearing them
            correctly in the first place.
          </p>
        </div>
      </div>

      <form className="row-form" onSubmit={add}>
        <div className="field">
          <label htmlFor="dw">Correct spelling</label>
          <input id="dw" className="input" type="text" placeholder="Kubernetes" value={word} onChange={(e) => setWord(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="dv">Often heard as <span className="faint">(optional, comma-separated)</span></label>
          <input id="dv" className="input" type="text" placeholder="cooper netties, cuber nettis" value={variants} onChange={(e) => setVariants(e.target.value)} />
        </div>
        <button className="btn primary" type="submit" disabled={!word.trim()}>Add word</button>
        {error && <div className="error-text" style={{ gridColumn: "1 / -1" }}>{error}</div>}
      </form>

      {rows.length === 0 ? (
        <div className="empty">
          <strong>Your dictionary is empty</strong>
          Add your name, your team's names, or any word Murmur keeps misspelling. Fixing a transcript in History suggests words
          automatically.
        </div>
      ) : (
        <div className="list">
          {rows.map((r) => (
            <div className="item" key={r.id}>
              <div>
                <div style={{ fontWeight: 600 }} className="selectable">{r.word}</div>
                {r.variants.length > 0 && <div className="small muted">Replaces: {r.variants.join(", ")}</div>}
              </div>
              <div className="item-actions">
                <button className="btn ghost" aria-label={`Remove ${r.word}`} title="Remove" onClick={() => remove(r.id)}><Icon.trash /></button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
