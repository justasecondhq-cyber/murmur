//! Putting text into whatever app has focus, and reading the current selection.
//!
//! We paste through the clipboard (fast and Unicode-safe in every app) and then
//! put the user's previous clipboard text back.

use anyhow::{anyhow, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::thread::sleep;
use std::time::Duration;

#[cfg(target_os = "macos")]
const MODIFIER: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const MODIFIER: Key = Key::Control;

fn chord(letter: char) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| anyhow!("keyboard access: {e:?}"))?;
    // Make sure the user's hotkey modifiers aren't still considered held.
    for k in [Key::Alt, Key::Shift] {
        let _ = enigo.key(k, Direction::Release);
    }
    enigo.key(MODIFIER, Direction::Press).map_err(|e| anyhow!("{e:?}"))?;
    let r = enigo.key(Key::Unicode(letter), Direction::Click);
    enigo.key(MODIFIER, Direction::Release).map_err(|e| anyhow!("{e:?}"))?;
    r.map_err(|e| anyhow!("{e:?}"))
}

/// Insert `text` at the cursor of the focused app.
pub fn insert_text(text: &str, restore_clipboard: bool) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let mut clipboard = arboard::Clipboard::new().map_err(|e| anyhow!("clipboard: {e}"))?;
    let previous = if restore_clipboard { clipboard.get_text().ok() } else { None };
    clipboard.set_text(text.to_string()).map_err(|e| anyhow!("clipboard: {e}"))?;
    sleep(Duration::from_millis(40));
    chord('v')?;
    if let Some(prev) = previous {
        // Give the target app time to read the clipboard before restoring it.
        sleep(Duration::from_millis(250));
        let _ = clipboard.set_text(prev);
    }
    Ok(())
}

/// Copy the current selection in the focused app. Returns "" if nothing is selected.
pub fn read_selection() -> Result<String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| anyhow!("clipboard: {e}"))?;
    let previous = clipboard.get_text().ok();
    // Put a sentinel in so we can tell whether the copy did anything.
    let sentinel = "\u{2063}murmur-no-selection\u{2063}";
    let _ = clipboard.set_text(sentinel.to_string());
    sleep(Duration::from_millis(30));
    chord('c')?;
    let mut selected = String::new();
    for _ in 0..12 {
        sleep(Duration::from_millis(25));
        if let Ok(t) = clipboard.get_text() {
            if t != sentinel {
                selected = t;
                break;
            }
        }
    }
    match previous {
        Some(p) => {
            let _ = clipboard.set_text(p);
        }
        None => {
            let _ = clipboard.clear();
        }
    }
    Ok(selected)
}
