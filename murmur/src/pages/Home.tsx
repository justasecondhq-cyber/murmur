import { useEffect, useState } from "react";
import { useApp } from "../App";
import { EVENTS, invoke } from "../api";
import { Keys, STYLE_LABELS, formatMinutes, formatNumber, timeAgo, useEvent } from "../components";
import type { DictationPayload, HistoryItem, Stats } from "../types";

export default function Home() {
  const { settings, live, go } = useApp();
  const [stats, setStats] = useState<Stats | null>(null);
  const [recent, setRecent] = useState<HistoryItem[]>([]);
  const [last, setLast] = useState<DictationPayload | null>(null);

  const load = () => {
    invoke<Stats>("get_stats").then(setStats);
    invoke<HistoryItem[]>("get_history", { limit: 4 }).then(setRecent);
  };
  useEffect(load, []);
  useEvent(EVENTS.data, load);
  useEvent<DictationPayload>(EVENTS.dictation, setLast);

  const phase = live?.phase ?? "idle";
  const recording = phase === "recording";
  const status =
    phase === "recording"
      ? live?.locked
        ? "Listening hands-free. Press the shortcut again to finish, Esc to cancel."
        : "Listening. Release to insert, Esc to cancel."
      : phase === "processing"
        ? "Writing it up…"
        : phase === "error"
          ? live?.message ?? "Something went wrong."
          : last
            ? `Inserted ${last.words} words into ${last.app || "your app"} in ${(last.process_ms / 1000).toFixed(1)} s.`
            : "Ready in every app.";

  const daily = stats?.daily_words ?? [];
  const max = Math.max(1, ...daily);
  const weekWords = daily.slice(-7).reduce((a, b) => a + b, 0);

  return (
    <div className="page">
      <section className="hero">
        <p className="hero-line">
          <span>Hold</span>
          <Keys shortcut={settings.hotkey_dictate} big pressed={recording} />
          <span>and talk.</span>
        </p>
        <p className="hero-sub">
          Release and Murmur types clean, punctuated text wherever your cursor is. Tap the shortcut instead of holding it to keep
          going hands-free. To rewrite selected text, hold <Keys shortcut={settings.hotkey_command} /> and say what to change.
        </p>
        <div className={`live ${phase}`} role="status" aria-live="polite">
          <span className={`status-dot ${recording ? "rec" : phase === "error" ? "warn" : "on"}`} />
          {status}
        </div>
      </section>

      <div className="stats">
        <div className="stat">
          <div className="stat-value">{formatNumber(weekWords)}</div>
          <div className="stat-label">Words this week</div>
        </div>
        <div className="stat">
          <div className="stat-value">{stats ? formatMinutes(stats.minutes_saved) : "—"}</div>
          <div className="stat-label" title={`Compared with typing at ${settings.typing_wpm} wpm`}>Time saved</div>
        </div>
        <div className="stat">
          <div className="stat-value">{stats && stats.avg_wpm > 0 ? Math.round(stats.avg_wpm) : "—"}</div>
          <div className="stat-label">Words per minute</div>
        </div>
        <div className="stat">
          <div className="stat-value">{stats?.streak_days ?? 0} {stats?.streak_days === 1 ? "day" : "days"}</div>
          <div className="stat-label">Current streak</div>
        </div>
      </div>

      <div className="spark" aria-label="Words per day, last 14 days">
        {daily.map((w, i) => (
          <div
            key={i}
            className={`spark-bar${w === 0 ? " zero" : ""}${i === daily.length - 1 ? " today" : ""}`}
            style={{ height: `${Math.max(3, (w / max) * 100)}%` }}
            title={`${formatNumber(w)} words`}
          />
        ))}
      </div>
      <div className="spark-axis"><span>Two weeks ago</span><span>Today, {formatNumber(stats?.words_today ?? 0)} words</span></div>

      <section className="section">
        <div className="section-head">
          <h2>Recent</h2>
          <button className="btn ghost" onClick={() => go("history")}>See all history</button>
        </div>
        {recent.length === 0 ? (
          <div className="empty">
            <strong>Nothing dictated yet</strong>
            Click into any text field, hold <Keys shortcut={settings.hotkey_dictate} /> and say a sentence.
          </div>
        ) : (
          <div className="list">
            {recent.map((h) => (
              <div className="item" key={h.id}>
                <div className="item-text">{h.text}</div>
                <div className="item-meta">
                  <span>{timeAgo(h.created_at)}</span>
                  {h.app && <span>{h.app}</span>}
                  <span className={`chip ${h.style}`}>{STYLE_LABELS[h.style] ?? h.style}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
