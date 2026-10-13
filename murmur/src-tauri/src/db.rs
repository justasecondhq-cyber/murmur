//! Local SQLite storage: history, personal dictionary, snippets and stats.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::text::{DictEntry, Snippet};

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: i64,
    pub created_at: i64, // unix seconds
    pub raw: String,
    pub text: String,
    pub app: String,
    pub style: String,
    pub mode: String, // "dictate" | "command"
    pub duration_ms: i64,
    pub process_ms: i64,
    pub words: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryRow {
    pub id: i64,
    pub word: String,
    pub variants: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetRow {
    pub id: i64,
    pub trigger: String,
    pub expansion: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Stats {
    pub total_words: i64,
    pub total_dictations: i64,
    pub audio_minutes: f64,
    pub minutes_saved: f64,
    pub avg_wpm: f64,
    pub streak_days: i64,
    pub words_today: i64,
    /// Words per day for the last 14 days, oldest first.
    pub daily_words: Vec<i64>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

impl Db {
    pub fn open(dir: &Path) -> Result<Db> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join("murmur.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                raw TEXT NOT NULL,
                text TEXT NOT NULL,
                app TEXT NOT NULL DEFAULT '',
                style TEXT NOT NULL DEFAULT 'neutral',
                mode TEXT NOT NULL DEFAULT 'dictate',
                duration_ms INTEGER NOT NULL DEFAULT 0,
                process_ms INTEGER NOT NULL DEFAULT 0,
                words INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS history_created ON history(created_at);
             CREATE TABLE IF NOT EXISTS dictionary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                word TEXT NOT NULL UNIQUE COLLATE NOCASE,
                variants TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS snippets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trigger TEXT NOT NULL UNIQUE COLLATE NOCASE,
                expansion TEXT NOT NULL,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS usage_days (
                day TEXT PRIMARY KEY,
                words INTEGER NOT NULL DEFAULT 0,
                dictations INTEGER NOT NULL DEFAULT 0,
                audio_ms INTEGER NOT NULL DEFAULT 0
             );",
        )?;
        Ok(Db { conn })
    }

    // ---------------- history ----------------

    #[allow(clippy::too_many_arguments)]
    pub fn add_history(
        &self,
        save_text: bool,
        raw: &str,
        text: &str,
        app: &str,
        style: &str,
        mode: &str,
        duration_ms: i64,
        process_ms: i64,
        words: i64,
    ) -> Result<Option<HistoryItem>> {
        // Usage stats are always counted (numbers only, no text).
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.conn.execute(
            "INSERT INTO usage_days(day, words, dictations, audio_ms) VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(day) DO UPDATE SET words = words + ?2, dictations = dictations + 1, audio_ms = audio_ms + ?3",
            params![day, words, duration_ms],
        )?;
        if !save_text {
            return Ok(None);
        }
        let ts = now();
        self.conn.execute(
            "INSERT INTO history(created_at, raw, text, app, style, mode, duration_ms, process_ms, words)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![ts, raw, text, app, style, mode, duration_ms, process_ms, words],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Some(HistoryItem {
            id,
            created_at: ts,
            raw: raw.into(),
            text: text.into(),
            app: app.into(),
            style: style.into(),
            mode: mode.into(),
            duration_ms,
            process_ms,
            words,
        }))
    }

    pub fn history(&self, query: &str, limit: i64) -> Result<Vec<HistoryItem>> {
        let like = format!("%{}%", query.trim());
        let mut stmt = self.conn.prepare(
            "SELECT id, created_at, raw, text, app, style, mode, duration_ms, process_ms, words
             FROM history WHERE (?1 = '%%' OR text LIKE ?1 OR raw LIKE ?1 OR app LIKE ?1)
             ORDER BY created_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![like, limit], |r| {
                Ok(HistoryItem {
                    id: r.get(0)?,
                    created_at: r.get(1)?,
                    raw: r.get(2)?,
                    text: r.get(3)?,
                    app: r.get(4)?,
                    style: r.get(5)?,
                    mode: r.get(6)?,
                    duration_ms: r.get(7)?,
                    process_ms: r.get(8)?,
                    words: r.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn history_item(&self, id: i64) -> Result<Option<HistoryItem>> {
        Ok(self.history("", 100_000)?.into_iter().find(|h| h.id == id))
    }

    pub fn update_history_text(&self, id: i64, text: &str) -> Result<()> {
        self.conn.execute("UPDATE history SET text = ?1 WHERE id = ?2", params![text, id])?;
        Ok(())
    }

    pub fn delete_history(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        self.conn.execute("DELETE FROM history", [])?;
        Ok(())
    }

    pub fn prune_history(&self, retention_days: u32) -> Result<()> {
        if retention_days == 0 {
            return Ok(());
        }
        let cutoff = now() - retention_days as i64 * 86_400;
        self.conn.execute("DELETE FROM history WHERE created_at < ?1", params![cutoff])?;
        Ok(())
    }

    // ---------------- dictionary ----------------

    pub fn dictionary(&self) -> Result<Vec<DictionaryRow>> {
        let mut stmt =
            self.conn.prepare("SELECT id, word, variants, created_at FROM dictionary ORDER BY word COLLATE NOCASE")?;
        let rows = stmt
            .query_map([], |r| {
                let v: String = r.get(2)?;
                Ok(DictionaryRow {
                    id: r.get(0)?,
                    word: r.get(1)?,
                    variants: serde_json::from_str(&v).unwrap_or_default(),
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn dict_entries(&self) -> Vec<DictEntry> {
        self.dictionary()
            .unwrap_or_default()
            .into_iter()
            .map(|r| DictEntry { word: r.word, variants: r.variants })
            .collect()
    }

    pub fn upsert_dictionary(&self, word: &str, variants: &[String]) -> Result<()> {
        let v: Vec<String> = variants.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        self.conn.execute(
            "INSERT INTO dictionary(word, variants, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(word) DO UPDATE SET word = ?1, variants = ?2",
            params![word.trim(), serde_json::to_string(&v)?, now()],
        )?;
        Ok(())
    }

    pub fn delete_dictionary(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM dictionary WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn has_word(&self, word: &str) -> bool {
        self.conn
            .query_row("SELECT 1 FROM dictionary WHERE word = ?1", params![word], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    // ---------------- snippets ----------------

    pub fn snippets(&self) -> Result<Vec<SnippetRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, trigger, expansion, created_at FROM snippets ORDER BY trigger COLLATE NOCASE")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SnippetRow { id: r.get(0)?, trigger: r.get(1)?, expansion: r.get(2)?, created_at: r.get(3)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn snippet_entries(&self) -> Vec<Snippet> {
        self.snippets()
            .unwrap_or_default()
            .into_iter()
            .map(|r| Snippet { trigger: r.trigger, expansion: r.expansion })
            .collect()
    }

    pub fn save_snippet(&self, id: Option<i64>, trigger: &str, expansion: &str) -> Result<()> {
        match id {
            Some(id) => {
                self.conn.execute(
                    "UPDATE snippets SET trigger = ?1, expansion = ?2 WHERE id = ?3",
                    params![trigger.trim(), expansion, id],
                )?;
            }
            None => {
                self.conn.execute(
                    "INSERT INTO snippets(trigger, expansion, created_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(trigger) DO UPDATE SET expansion = ?2",
                    params![trigger.trim(), expansion, now()],
                )?;
            }
        }
        Ok(())
    }

    pub fn delete_snippet(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ---------------- stats ----------------

    pub fn stats(&self, typing_wpm: u32) -> Result<Stats> {
        let (words, dictations, audio_ms): (i64, i64, i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(words),0), COALESCE(SUM(dictations),0), COALESCE(SUM(audio_ms),0) FROM usage_days",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let audio_minutes = audio_ms as f64 / 60_000.0;
        let typing_minutes = words as f64 / typing_wpm.max(1) as f64;
        let minutes_saved = (typing_minutes - audio_minutes).max(0.0);
        let avg_wpm = if audio_minutes > 0.05 { words as f64 / audio_minutes } else { 0.0 };

        // Per-day words for the last 14 days and the current streak.
        let mut daily = vec![];
        let today = chrono::Local::now().date_naive();
        let mut stmt = self.conn.prepare("SELECT words FROM usage_days WHERE day = ?1")?;
        let mut day_words = |d: chrono::NaiveDate| -> i64 {
            stmt.query_row(params![d.format("%Y-%m-%d").to_string()], |r| r.get::<_, i64>(0)).unwrap_or(0)
        };
        for back in (0..14).rev() {
            let d = today - chrono::Duration::days(back);
            daily.push(day_words(d));
        }
        let words_today = *daily.last().unwrap_or(&0);
        let mut streak = 0;
        // A streak survives until the end of today even if you haven't dictated yet.
        let mut d = if words_today > 0 { today } else { today - chrono::Duration::days(1) };
        loop {
            if day_words(d) > 0 {
                streak += 1;
                d = d - chrono::Duration::days(1);
            } else {
                break;
            }
            if streak > 3650 {
                break;
            }
        }

        Ok(Stats {
            total_words: words,
            total_dictations: dictations,
            audio_minutes,
            minutes_saved,
            avg_wpm,
            streak_days: streak,
            words_today,
            daily_words: daily,
        })
    }

    /// Wipe every table. Used by "Delete all my data".
    pub fn wipe(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM history; DELETE FROM dictionary; DELETE FROM snippets; DELETE FROM usage_days; VACUUM;",
        )?;
        Ok(())
    }
}
