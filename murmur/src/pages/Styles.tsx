import { useState } from "react";
import { useApp } from "../App";
import { errorText, invoke } from "../api";
import { Icon, Segmented, Setting, Toggle } from "../components";
import type { AppRule, StyleName } from "../types";

const STYLE_OPTIONS: { value: StyleName; label: string }[] = [
  { value: "neutral", label: "Standard" },
  { value: "casual", label: "Casual" },
  { value: "formal", label: "Formal" },
  { value: "code", label: "Code" },
];

const BUILT_IN: { style: string; apps: string }[] = [
  { style: "Casual", apps: "Slack, Discord, WhatsApp, Telegram, Messages, Signal" },
  { style: "Formal", apps: "Mail, Outlook, Gmail and Outlook in the browser, Word, Pages" },
  { style: "Code", apps: "VS Code, Cursor, Zed, JetBrains IDEs, Xcode, Terminal, iTerm, Warp, PowerShell" },
];

const EXAMPLES: Record<StyleName, string> = {
  neutral: "um so I think we should, like, push the launch to next week no wait two weeks",
  casual: "haha yeah um that works for me see you at 7",
  formal: "hi Dana uh just following up on the contract can you send the signed copy by friday",
  code: "camel case user name equals open paren close paren",
};

export default function Styles() {
  const { settings, update } = useApp();
  const [pattern, setPattern] = useState("");
  const [ruleStyle, setRuleStyle] = useState<StyleName>("formal");
  const [tryStyle, setTryStyle] = useState<StyleName>("neutral");
  const [input, setInput] = useState(EXAMPLES.neutral);
  const [output, setOutput] = useState<{ text: string; provider: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const rules = settings.app_rules;
  const addRule = () => {
    if (!pattern.trim()) return;
    update({ app_rules: [...rules.filter((r) => r.pattern.toLowerCase() !== pattern.trim().toLowerCase()), { pattern: pattern.trim(), style: ruleStyle }] });
    setPattern("");
  };
  const removeRule = (r: AppRule) => update({ app_rules: rules.filter((x) => x !== r) });

  const run = async () => {
    setBusy(true);
    try {
      setOutput(await invoke("test_cleanup", { text: input, style: tryStyle }));
    } catch (e) {
      setOutput({ text: errorText(e), provider: "" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Styles</h1>
          <p className="lede">Murmur adjusts its writing to the app you're in: relaxed in chat, polished in email, literal in code.</p>
        </div>
      </div>

      <div className="group">
        <div className="rows">
          <Setting title="Match style to the app" desc="Detects the app you're typing into each time you dictate.">
            <Toggle label="Match style to the app" checked={settings.per_app_styles} onChange={(v) => update({ per_app_styles: v })} />
          </Setting>
          <Setting title="Everywhere else" desc="Used when no rule below matches, or when matching is off.">
            <Segmented label="Default style" value={settings.default_style} options={STYLE_OPTIONS} onChange={(v) => update({ default_style: v })} />
          </Setting>
        </div>
      </div>

      <div className="group">
        <h2>Your rules</h2>
        <p className="lede">Matched against the app name and window title, checked before the built-in list.</p>
        <div className="rows">
          {rules.map((r) => (
            <Setting key={r.pattern} title={r.pattern} desc={`Uses the ${STYLE_OPTIONS.find((o) => o.value === r.style)?.label ?? r.style} style`}>
              <button className="btn ghost" aria-label={`Remove rule for ${r.pattern}`} onClick={() => removeRule(r)}><Icon.trash /></button>
            </Setting>
          ))}
          <div className="setting">
            <input className="input" type="text" placeholder="App or window title, e.g. Notion" value={pattern} onChange={(e) => setPattern(e.target.value)} onKeyDown={(e) => e.key === "Enter" && addRule()} aria-label="App or window title" />
            <div className="setting-control">
              <select className="select" style={{ width: 130 }} value={ruleStyle} onChange={(e) => setRuleStyle(e.target.value as StyleName)} aria-label="Style">
                {STYLE_OPTIONS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
              </select>
              <button className="btn" onClick={addRule} disabled={!pattern.trim()}>Add rule</button>
            </div>
          </div>
        </div>
      </div>

      <div className="group">
        <h2>Built in</h2>
        <div className="rows">
          {BUILT_IN.map((b) => <Setting key={b.style} title={b.style} desc={b.apps} />)}
        </div>
      </div>

      <div className="group">
        <h2>Try a style</h2>
        <p className="lede">Type something the way you'd say it and see what Murmur would insert.</p>
        <div className="rows" style={{ padding: 16, display: "grid", gap: 12 }}>
          <Segmented label="Style to try" value={tryStyle} options={STYLE_OPTIONS} onChange={(v) => { setTryStyle(v); setInput(EXAMPLES[v]); setOutput(null); }} />
          <textarea className="textarea" value={input} onChange={(e) => setInput(e.target.value)} aria-label="Text to clean up" />
          <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
            <button className="btn primary" onClick={run} disabled={busy || !input.trim()}>{busy ? "Cleaning up…" : "Clean up"}</button>
            {output?.provider && <span className="small muted">Cleaned by {output.provider}</span>}
          </div>
          {output && <div className="codeblock" style={{ fontFamily: tryStyle === "code" ? undefined : "var(--font)", fontSize: 14 }}>{output.text}</div>}
        </div>
      </div>
    </div>
  );
}
