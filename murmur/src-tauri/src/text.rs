//! Text clean-up engine.
//!
//! Everything in this module is pure std Rust so it can be unit-tested without
//! audio, models or a GUI. The pipeline is:
//!
//!   raw whisper text
//!     -> strip_whisper_artifacts
//!     -> rule_clean   (self-corrections, fillers, stutters, spoken commands,
//!                      lists, code mode, capitalization, dictionary)
//!     -> optional LLM pass (prompt built here, output checked by sanitize_*)
//!     -> snippets restored, dictionary re-applied

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Neutral,
    Casual,
    Formal,
    Code,
}

impl Style {
    pub fn parse(s: &str) -> Style {
        match s.trim().to_lowercase().as_str() {
            "casual" => Style::Casual,
            "formal" => Style::Formal,
            "code" | "literal" => Style::Code,
            _ => Style::Neutral,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Style::Neutral => "neutral",
            Style::Casual => "casual",
            Style::Formal => "formal",
            Style::Code => "code",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DictEntry {
    /// The correct spelling, e.g. "Kubernetes".
    pub word: String,
    /// Things the recognizer tends to hear instead, e.g. ["cooper netties"].
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Snippet {
    pub trigger: String,
    pub expansion: String,
}

#[derive(Debug, Clone)]
pub struct CleanOptions {
    pub style: Style,
    /// Turn spoken "comma", "period", "colon" into punctuation.
    pub spoken_punctuation: bool,
    pub dictionary: Vec<DictEntry>,
}

impl Default for CleanOptions {
    fn default() -> Self {
        CleanOptions { style: Style::Neutral, spoken_punctuation: false, dictionary: vec![] }
    }
}

#[derive(Debug, Clone)]
pub struct AppRule {
    pub pattern: String,
    pub style: Style,
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Lowercased word with surrounding punctuation removed ("Um," -> "um").
fn norm(token: &str) -> String {
    token.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').trim_matches('\'').to_lowercase()
}

fn ends_sentence(token: &str) -> bool {
    let t = token.trim_end_matches(|c: char| c == '"' || c == '\'' || c == ')');
    t.ends_with('.') || t.ends_with('?') || t.ends_with('!')
}

fn ends_with_punct(token: &str) -> bool {
    token.ends_with(|c: char| matches!(c, ',' | '.' | '?' | '!' | ':' | ';'))
}

fn strip_trailing_punct(token: &str) -> String {
    token.trim_end_matches(|c: char| matches!(c, ',' | '.' | ';' | ':')).to_string()
}

fn trailing_punct(token: &str) -> String {
    let core = token.trim_end_matches(|c: char| matches!(c, ',' | '.' | '?' | '!' | ':' | ';'));
    token[core.len()..].to_string()
}

fn is_newline_token(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c == '\n')
}

fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace().map(|s| s.to_string()).collect()
}

/// Join tokens with single spaces, but no spaces around newline tokens.
fn join(tokens: &[String]) -> String {
    let mut out = String::new();
    for t in tokens {
        if t.is_empty() {
            continue;
        }
        if is_newline_token(t) {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push_str(t);
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push_str(t);
    }
    out.trim().to_string()
}

/// Does `toks[i..]` start with the given lowercase word sequence?
fn matches_at(toks: &[String], i: usize, phrase: &[&str]) -> bool {
    if i + phrase.len() > toks.len() {
        return false;
    }
    phrase.iter().enumerate().all(|(k, w)| norm(&toks[i + k]) == *w)
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count()
}

// ---------------------------------------------------------------------------
// Whisper artifacts and hallucinations
// ---------------------------------------------------------------------------

/// Removes things like "[BLANK_AUDIO]", "(music)", "*laughs*" and collapses whitespace.
pub fn strip_whisper_artifacts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth_sq = 0;
    let mut depth_par = 0;
    let mut in_star = false;
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '[' => depth_sq += 1,
            ']' if depth_sq > 0 => depth_sq -= 1,
            '(' => {
                // Only treat "(...)" as an annotation if it's short and lowercase-ish,
                // e.g. "(music)" or "(upbeat music)". Keep normal parentheses.
                let rest: String = chars[i + 1..].iter().take_while(|&&ch| ch != ')').collect();
                let is_annotation = rest.len() <= 24
                    && chars[i + 1..].contains(&')')
                    && rest.split_whitespace().count() <= 3
                    && ["music", "silence", "applause", "laugh", "noise", "inaudible", "blank", "sigh", "cough"]
                        .iter()
                        .any(|w| rest.to_lowercase().contains(w));
                if is_annotation || depth_par > 0 {
                    depth_par += 1;
                } else if depth_sq == 0 && !in_star {
                    out.push(c);
                }
            }
            ')' if depth_par > 0 => depth_par -= 1,
            '*' => {
                let rest: String = chars[i + 1..].iter().take_while(|&&ch| ch != '*').collect();
                if in_star {
                    in_star = false;
                } else if chars[i + 1..].contains(&'*') && rest.split_whitespace().count() <= 3 {
                    in_star = true;
                } else if depth_sq == 0 && depth_par == 0 {
                    out.push(c);
                }
            }
            _ => {
                if depth_sq == 0 && depth_par == 0 && !in_star {
                    out.push(c);
                }
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Phrases Whisper famously produces on silence or noise.
pub fn is_hallucination(text: &str) -> bool {
    let t = norm_phrase(text);
    if t.is_empty() {
        return true;
    }
    const KNOWN: &[&str] = &[
        "you",
        "thank you",
        "thanks",
        "thank you for watching",
        "thanks for watching",
        "thank you so much for watching",
        "please subscribe",
        "subscribe",
        "bye",
        "okay",
        "the end",
        "so",
        "subtitles by the amara org community",
        "transcribed by",
    ];
    KNOWN.contains(&t.as_str())
}

fn norm_phrase(text: &str) -> String {
    text.split_whitespace().map(norm).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Self-corrections
// ---------------------------------------------------------------------------

const CORRECTION_MARKERS: &[&[&str]] = &[
    &["no", "wait"],
    &["wait", "no"],
    &["no", "no"],
    &["no", "sorry"],
    &["sorry", "no"],
    &["sorry", "i", "mean"],
    &["or", "rather"],
    &["actually", "no"],
    &["i", "meant"],
];

const SCRATCH_MARKERS: &[&[&str]] = &[&["scratch", "that"], &["delete", "that"], &["strike", "that"]];

fn apply_corrections(mut toks: Vec<String>) -> Vec<String> {
    for _ in 0..8 {
        let mut changed = false;

        // "scratch that": drop everything back to the previous sentence boundary.
        'scratch: for i in 0..toks.len() {
            for m in SCRATCH_MARKERS {
                if matches_at(&toks, i, m) {
                    let mut keep_until = 0; // exclusive
                    for j in (0..i).rev() {
                        if ends_sentence(&toks[j]) {
                            keep_until = j + 1;
                            break;
                        }
                    }
                    let mut next: Vec<String> = toks[..keep_until].to_vec();
                    next.extend_from_slice(&toks[i + m.len()..]);
                    toks = next;
                    changed = true;
                    break 'scratch;
                }
            }
        }

        // "X, no wait, Y": replace the corrected part.
        'corr: for i in 1..toks.len() {
            for m in CORRECTION_MARKERS {
                if !matches_at(&toks, i, m) {
                    continue;
                }
                // Don't treat "no" at the very start of a sentence as a correction.
                if ends_sentence(&toks[i - 1]) {
                    continue;
                }
                let before: Vec<String> = toks[..i].to_vec();
                let after: Vec<String> = toks[i + m.len()..].to_vec();
                let mut next: Vec<String>;
                if after.is_empty() {
                    next = before;
                    if let Some(last) = next.last_mut() {
                        *last = strip_trailing_punct(last);
                    }
                } else {
                    let w1 = norm(&after[0]);
                    let lower_bound = before.len().saturating_sub(8);
                    let mut cut: Option<usize> = None;
                    for j in (lower_bound..before.len()).rev() {
                        if j < before.len() - 1 && ends_sentence(&before[j]) {
                            break;
                        }
                        if norm(&before[j]) == w1 {
                            cut = Some(j);
                            break;
                        }
                    }
                    let cut = cut.unwrap_or(before.len() - 1);
                    // Keep the sentence punctuation that belonged to the corrected word.
                    next = before[..cut].to_vec();
                    next.extend(after);
                }
                toks = next;
                changed = true;
                break 'corr;
            }
        }

        if !changed {
            break;
        }
    }
    toks
}

// ---------------------------------------------------------------------------
// Fillers and stutters
// ---------------------------------------------------------------------------

const FILLERS: &[&str] =
    &["um", "umm", "ummm", "uh", "uhh", "uhm", "er", "erm", "ah", "ahh", "hmm", "hm", "mm", "mmm", "eh"];

/// Remove the token at `i` (and `extra` tokens after it), fixing up the
/// punctuation around the hole.
fn remove_span(toks: &mut Vec<String>, i: usize, len: usize) {
    let last = &toks[i + len - 1];
    let punct = trailing_punct(last);
    toks.drain(i..i + len);
    if i > 0 {
        let prev = toks[i - 1].clone();
        if punct.contains('.') || punct.contains('?') || punct.contains('!') {
            // The filler closed a sentence: move the stop onto the previous word.
            let p = punct.trim_start_matches(',').to_string();
            toks[i - 1] = format!("{}{}", strip_trailing_punct(&prev), p);
        } else if punct.contains(',') && prev.ends_with(',') {
            // "I, uh, think" -> "I think"
            toks[i - 1] = prev.trim_end_matches(',').to_string();
        }
    }
}

fn remove_fillers(mut toks: Vec<String>) -> Vec<String> {
    let mut i = 0;
    while i < toks.len() {
        let n = norm(&toks[i]);
        let at_clause_start = i == 0 || ends_with_punct(&toks[i - 1]) || is_newline_token(&toks[i - 1]);

        if FILLERS.contains(&n.as_str()) && toks[i].chars().any(|c| c.is_alphabetic()) {
            remove_span(&mut toks, i, 1);
            continue;
        }
        // "you know," as a filler (not "do you know").
        if n == "you" && matches_at(&toks, i, &["you", "know"]) {
            let know_tok = &toks[i + 1];
            if at_clause_start && (know_tok.ends_with(',') || i + 2 == toks.len()) {
                remove_span(&mut toks, i, 2);
                continue;
            }
        }
        // ", like," as a filler.
        if n == "like" && i > 0 && toks[i - 1].ends_with(',') && toks[i].ends_with(',') {
            remove_span(&mut toks, i, 1);
            continue;
        }
        // "I mean," at the start of a clause.
        if n == "i" && matches_at(&toks, i, &["i", "mean"]) && at_clause_start && toks[i + 1].ends_with(',') {
            remove_span(&mut toks, i, 2);
            continue;
        }
        i += 1;
    }
    // Capitalize a sentence whose first word was a removed filler happens later.
    toks
}

const STUTTER_WORDS: &[&str] = &[
    "i", "the", "a", "an", "to", "and", "we", "it", "so", "but", "is", "was", "of", "in", "my", "you", "they", "he",
    "she", "this", "for", "on", "with", "are", "if",
];

fn remove_stutters(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(toks.len());
    for t in toks {
        if let Some(prev) = out.last() {
            let n = norm(&t);
            if !ends_with_punct(prev) && norm(prev) == n && STUTTER_WORDS.contains(&n.as_str()) {
                // Keep the later token (it carries any punctuation).
                let keep_case_of_prev = prev.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
                out.pop();
                let mut t2 = t.clone();
                if keep_case_of_prev {
                    t2 = capitalize_first(&t2);
                }
                out.push(t2);
                continue;
            }
        }
        out.push(t);
    }
    out
}

// ---------------------------------------------------------------------------
// Spoken commands: "new line", "question mark", ...
// ---------------------------------------------------------------------------

fn attach_punct(toks: &mut Vec<String>, p: &str) {
    if let Some(last) = toks.last_mut() {
        if !is_newline_token(last) {
            let base = strip_trailing_punct(last);
            *last = format!("{}{}", base, p);
            return;
        }
    }
    // Nothing to attach to: drop it.
}

fn apply_spoken_commands(toks: Vec<String>, spoken_punctuation: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let rest = &toks[..];
        // Each entry: phrase, replacement, needs the spoken_punctuation setting.
        let table: &[(&[&str], &str, bool)] = &[
            (&["new", "paragraph"], "\n\n", false),
            (&["new", "line"], "\n", false),
            (&["next", "line"], "\n", false),
            (&["question", "mark"], "?", false),
            (&["exclamation", "mark"], "!", false),
            (&["exclamation", "point"], "!", false),
            (&["full", "stop"], ".", false),
            (&["semicolon"], ";", false),
            (&["comma"], ",", true),
            (&["period"], ".", true),
            (&["colon"], ":", true),
        ];
        let mut matched = false;
        for (phrase, rep, needs_setting) in table {
            if *needs_setting && !spoken_punctuation {
                continue;
            }
            if matches_at(rest, i, phrase) {
                // Whisper often already punctuates around the command; keep any
                // trailing punctuation only if the command produced a newline.
                if rep.starts_with('\n') {
                    if let Some(last) = out.last_mut() {
                        if !is_newline_token(last) && !ends_with_punct(last) {
                            // leave as is - capitalization pass handles the next line
                        }
                        let trimmed = last.trim_end_matches(',').to_string();
                        *last = trimmed;
                    }
                    out.push(rep.to_string());
                } else {
                    attach_punct(&mut out, rep);
                }
                i += phrase.len();
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(toks[i].clone());
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Lists: "first eggs, second milk and third bread"
// ---------------------------------------------------------------------------

fn ordinal_at(toks: &[String], i: usize) -> Option<(usize, usize)> {
    // Returns (ordinal number, tokens consumed)
    let singles: &[(&str, usize)] = &[
        ("first", 1),
        ("firstly", 1),
        ("second", 2),
        ("secondly", 2),
        ("third", 3),
        ("thirdly", 3),
        ("fourth", 4),
        ("fourthly", 4),
        ("fifth", 5),
        ("sixth", 6),
        ("seventh", 7),
        ("eighth", 8),
        ("ninth", 9),
        ("tenth", 10),
    ];
    let numbers: &[(&str, usize)] = &[
        ("one", 1),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
        ("1", 1),
        ("2", 2),
        ("3", 3),
        ("4", 4),
        ("5", 5),
        ("6", 6),
        ("7", 7),
        ("8", 8),
        ("9", 9),
        ("10", 10),
    ];
    let n0 = norm(&toks[i]);
    if n0 == "number" && i + 1 < toks.len() {
        let n1 = norm(&toks[i + 1]);
        if let Some((_, v)) = numbers.iter().find(|(w, _)| *w == n1) {
            return Some((*v, 2));
        }
    }
    singles.iter().find(|(w, _)| *w == n0).map(|(_, v)| (*v, 1))
}

/// Collect ordinal markers ("first", "second", "number three"...) in order.
/// `strict` requires each marker to start a clause (after punctuation or "and").
fn find_list_markers(toks: &[String], strict: bool) -> Vec<(usize, usize, usize)> {
    let mut markers: Vec<(usize, usize, usize)> = vec![]; // (index, ordinal, len)
    let mut expected = 1;
    let mut i = 0;
    while i < toks.len() {
        let clause_start =
            i == 0 || ends_with_punct(&toks[i - 1]) || is_newline_token(&toks[i - 1]) || norm(&toks[i - 1]) == "and";
        if clause_start || !strict {
            if let Some((ord, len)) = ordinal_at(toks, i) {
                // An ordinal must be followed by something to list.
                if ord == expected && i + len < toks.len() {
                    markers.push((i, ord, len));
                    expected += 1;
                    i += len;
                    continue;
                }
            }
        }
        i += 1;
    }
    markers
}

fn format_lists(toks: Vec<String>) -> Vec<String> {
    let mut markers = find_list_markers(&toks, true);
    if markers.len() < 2 {
        // Unpunctuated speech: "the plan is first X second Y third Z" needs
        // three ordinals (or explicit "number one/two") to count as a list.
        let relaxed = find_list_markers(&toks, false);
        let explicit = relaxed.iter().all(|m| m.2 == 2);
        if relaxed.len() >= 3 || (relaxed.len() >= 2 && explicit) {
            markers = relaxed;
        }
    }
    if markers.len() < 2 {
        return toks;
    }

    let first_idx = markers[0].0;
    let mut out: Vec<String> = toks[..first_idx].to_vec();
    if let Some(last) = out.last_mut() {
        *last = format!("{}:", strip_trailing_punct(last));
    }
    if !out.is_empty() {
        out.push("\n".into());
    }

    for (k, (idx, ord, len)) in markers.iter().enumerate() {
        let start = idx + len;
        let end = if k + 1 < markers.len() { markers[k + 1].0 } else { toks.len() };
        let mut item: Vec<String> = toks[start..end].to_vec();
        // Drop a leading comma-only remnant like "first," -> handled by marker.
        // Drop trailing "and" (from "..., and third").
        if k + 1 < markers.len() {
            if let Some(last) = item.last() {
                if norm(last) == "and" {
                    item.pop();
                }
            }
        }
        // For the last item, stop at the end of its sentence.
        let mut tail: Vec<String> = vec![];
        if k + 1 == markers.len() {
            if let Some(pos) = item.iter().position(|t| ends_sentence(t)) {
                if pos + 1 < item.len() {
                    tail = item[pos + 1..].to_vec();
                    item.truncate(pos + 1);
                }
            }
        }
        if let Some(last) = item.last_mut() {
            *last = last.trim_end_matches(|c: char| matches!(c, ',' | ';' | '.')).to_string();
        }
        item.retain(|t| !t.is_empty());
        if item.is_empty() {
            continue;
        }
        item[0] = capitalize_first(&item[0]);
        out.push(format!("{}.", ord));
        out.extend(item);
        if k + 1 < markers.len() {
            out.push("\n".into());
        }
        if !tail.is_empty() {
            out.push("\n\n".into());
            out.extend(tail);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Code mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Glue {
    None,
    Left,
    Right,
    Both,
}

fn code_symbol(toks: &[String], i: usize) -> Option<(&'static str, usize, Glue)> {
    let table: &[(&[&str], &str, Glue)] = &[
        (&["open", "paren"], "(", Glue::Both),
        (&["open", "parenthesis"], "(", Glue::Both),
        (&["close", "paren"], ")", Glue::Left),
        (&["close", "parenthesis"], ")", Glue::Left),
        (&["open", "bracket"], "[", Glue::Both),
        (&["close", "bracket"], "]", Glue::Left),
        (&["open", "curly", "brace"], "{", Glue::None),
        (&["open", "brace"], "{", Glue::None),
        (&["open", "curly"], "{", Glue::None),
        (&["close", "curly", "brace"], "}", Glue::None),
        (&["close", "brace"], "}", Glue::None),
        (&["close", "curly"], "}", Glue::None),
        (&["triple", "equals"], "===", Glue::None),
        (&["double", "equals"], "==", Glue::None),
        (&["not", "equals"], "!=", Glue::None),
        (&["fat", "arrow"], "=>", Glue::None),
        (&["arrow"], "->", Glue::None),
        (&["equals"], "=", Glue::None),
        (&["plus", "equals"], "+=", Glue::None),
        (&["plus"], "+", Glue::None),
        (&["minus"], "-", Glue::None),
        (&["dot"], ".", Glue::Both),
        (&["underscore"], "_", Glue::Both),
        (&["dash", "dash"], "--", Glue::Right),
        (&["dash"], "-", Glue::Right),
        (&["slash"], "/", Glue::Both),
        (&["backslash"], "\\", Glue::Both),
        (&["colon"], ":", Glue::Left),
        (&["semicolon"], ";", Glue::Left),
        (&["comma"], ",", Glue::Left),
        (&["backtick"], "`", Glue::None),
        (&["hash"], "#", Glue::Right),
        (&["at", "sign"], "@", Glue::Right),
        (&["dollar", "sign"], "$", Glue::Right),
        (&["ampersand"], "&", Glue::None),
        (&["pipe"], "|", Glue::None),
        (&["asterisk"], "*", Glue::None),
        (&["less", "than"], "<", Glue::None),
        (&["greater", "than"], ">", Glue::None),
        (&["question", "mark"], "?", Glue::Left),
        (&["bang"], "!", Glue::Right),
        (&["tilde"], "~", Glue::Right),
    ];
    for (phrase, sym, glue) in table {
        if matches_at(toks, i, phrase) {
            return Some((sym, phrase.len(), *glue));
        }
    }
    None
}

fn case_command(toks: &[String], i: usize) -> Option<(&'static str, usize)> {
    let table: &[(&[&str], &str)] = &[
        (&["camel", "case"], "camel"),
        (&["snake", "case"], "snake"),
        (&["pascal", "case"], "pascal"),
        (&["kebab", "case"], "kebab"),
        (&["constant", "case"], "constant"),
        (&["screaming", "snake", "case"], "constant"),
        (&["all", "caps"], "upper"),
        (&["lower", "case"], "lower"),
    ];
    for (phrase, kind) in table {
        if matches_at(toks, i, phrase) {
            return Some((kind, phrase.len()));
        }
    }
    None
}

fn apply_case(kind: &str, words: &[String]) -> String {
    let ws: Vec<String> = words.iter().map(|w| norm(w)).filter(|w| !w.is_empty()).collect();
    match kind {
        "camel" => ws.iter().enumerate().map(|(k, w)| if k == 0 { w.clone() } else { capitalize_first(w) }).collect(),
        "pascal" => ws.iter().map(|w| capitalize_first(w)).collect(),
        "snake" => ws.join("_"),
        "kebab" => ws.join("-"),
        "constant" => ws.join("_").to_uppercase(),
        "upper" => ws.join(" ").to_uppercase(),
        _ => ws.join(" "),
    }
}

fn code_transform(toks: Vec<String>) -> String {
    // Produce (text, glue) pieces then join.
    let mut pieces: Vec<(String, Glue)> = vec![];
    let mut i = 0;
    while i < toks.len() {
        if is_newline_token(&toks[i]) {
            pieces.push((toks[i].clone(), Glue::Both));
            i += 1;
            continue;
        }
        if let Some((kind, len)) = case_command(&toks, i) {
            let mut j = i + len;
            let mut words = vec![];
            while j < toks.len() && words.len() < 6 {
                if code_symbol(&toks, j).is_some() || case_command(&toks, j).is_some() || is_newline_token(&toks[j]) {
                    break;
                }
                let t = &toks[j];
                words.push(t.clone());
                j += 1;
                if ends_with_punct(t) {
                    break;
                }
            }
            if !words.is_empty() {
                pieces.push((apply_case(kind, &words), Glue::None));
            }
            i = j;
            continue;
        }
        if let Some((sym, len, glue)) = code_symbol(&toks, i) {
            pieces.push((sym.to_string(), glue));
            i += len;
            continue;
        }
        pieces.push((toks[i].clone(), Glue::None));
        i += 1;
    }

    let mut out = String::new();
    let mut prev_glue_right = true; // no leading space
    for (text, glue) in pieces {
        let glue_left = matches!(glue, Glue::Left | Glue::Both);
        if !prev_glue_right && !glue_left && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push_str(&text);
        prev_glue_right = matches!(glue, Glue::Right | Glue::Both);
    }
    // Whisper likes to end commands with a period: "git status."
    let trimmed = out.trim().to_string();
    if trimmed.ends_with('.') && !trimmed.ends_with("..") && word_count(&trimmed) <= 12 {
        trimmed[..trimmed.len() - 1].to_string()
    } else {
        trimmed
    }
}

// ---------------------------------------------------------------------------
// Capitalization
// ---------------------------------------------------------------------------

pub fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Capitalize the first letter of every sentence and line, and fix "i" -> "I".
fn capitalize_sentences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cap_next = true;
    let mut prev: char = '\n';
    for c in text.chars() {
        if cap_next && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            cap_next = false;
        } else {
            out.push(c);
            if c.is_alphanumeric() {
                cap_next = false;
            }
        }
        if c == '\n' {
            cap_next = true;
        }
        if c.is_whitespace() && matches!(prev, '.' | '?' | '!') {
            cap_next = true;
        }
        prev = c;
    }
    // Standalone "i" and its contractions.
    let fixed: Vec<String> = out
        .split(' ')
        .map(|w| {
            let n = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            if matches!(n, "i" | "i'm" | "i've" | "i'll" | "i'd") {
                w.replacen('i', "I", 1)
            } else {
                w.to_string()
            }
        })
        .collect();
    fixed.join(" ")
}

fn finish_punctuation(text: &str, style: Style) -> String {
    let t = text.trim_end().to_string();
    if t.is_empty() {
        return t;
    }
    let last_line = t.lines().last().unwrap_or("");
    let is_list_item = last_line
        .split_whitespace()
        .next()
        .map(|w| w.ends_with('.') && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit()) && w.len() > 1)
        .unwrap_or(false)
        || last_line.trim_start().starts_with("- ");
    match style {
        Style::Casual => {
            // Chat apps: drop a single trailing period.
            if t.ends_with('.') && !t.ends_with("..") && !t.contains('\n') && t.matches('.').count() == 1 {
                t[..t.len() - 1].to_string()
            } else {
                t
            }
        }
        Style::Code => t,
        _ => {
            if is_list_item {
                return t;
            }
            let last = t.chars().last().unwrap();
            if last.is_alphanumeric() || last == '\'' || last == '"' && t.matches('"').count() % 2 == 0 {
                format!("{}.", t)
            } else if last == ',' || last == ';' || last == ':' {
                format!("{}.", &t[..t.len() - 1])
            } else {
                t
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

fn lower_chars(s: &str) -> Vec<char> {
    // One output char per input char so indices line up.
    s.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// Case-insensitive, whole-word replacement of `needle` with `replacement`.
pub fn replace_phrase(text: &str, needle: &str, replacement: &str) -> String {
    let needle = needle.trim();
    if needle.is_empty() {
        return text.to_string();
    }
    let hay: Vec<char> = text.chars().collect();
    let hay_l = lower_chars(text);
    let nd = lower_chars(needle);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < hay.len() {
        let fits = i + nd.len() <= hay.len() && hay_l[i..i + nd.len()] == nd[..];
        let left_ok = i == 0 || !hay[i - 1].is_alphanumeric();
        let right_ok = i + nd.len() >= hay.len() || !hay[i + nd.len()].is_alphanumeric();
        if fits && left_ok && right_ok {
            out.push_str(replacement);
            i += nd.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

pub fn apply_dictionary(text: &str, dict: &[DictEntry]) -> String {
    let mut t = text.to_string();
    for e in dict {
        if e.word.trim().is_empty() {
            continue;
        }
        for v in &e.variants {
            t = replace_phrase(&t, v, &e.word);
        }
        t = replace_phrase(&t, &e.word, &e.word);
    }
    t
}

/// Whisper's "initial prompt" biases recognition towards these spellings.
pub fn whisper_prompt(dict: &[DictEntry]) -> String {
    let words: Vec<&str> = dict.iter().map(|e| e.word.trim()).filter(|w| !w.is_empty()).take(80).collect();
    if words.is_empty() {
        String::new()
    } else {
        format!("Glossary: {}.", words.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Snippets
// ---------------------------------------------------------------------------

/// Replace snippet triggers with placeholder tokens so an LLM can't rewrite them.
pub fn protect_snippets(text: &str, snippets: &[Snippet]) -> (String, Vec<(String, String)>) {
    let mut t = text.to_string();
    let mut map = vec![];
    // Longest triggers first so "my work email" wins over "my email".
    let mut sorted: Vec<&Snippet> = snippets.iter().filter(|s| !s.trigger.trim().is_empty()).collect();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.trigger.len()));
    for s in sorted {
        let token = format!("[[S{}]]", map.len() + 1);
        let replaced = replace_phrase(&t, &s.trigger, &token);
        if replaced != t {
            t = replaced;
            map.push((token, s.expansion.clone()));
        }
    }
    (t, map)
}

/// Put the snippet expansions back. Returns None if the LLM dropped a token.
pub fn restore_snippets(text: &str, map: &[(String, String)]) -> Option<String> {
    let mut t = text.to_string();
    for (token, expansion) in map {
        if !t.contains(token.as_str()) {
            return None;
        }
        t = t.replace(token.as_str(), expansion);
    }
    Some(t)
}

/// If the whole utterance is just a snippet trigger, return the expansion as-is.
pub fn exact_snippet(text: &str, snippets: &[Snippet]) -> Option<String> {
    let n = norm_phrase(text);
    snippets.iter().find(|s| !s.trigger.trim().is_empty() && norm_phrase(&s.trigger) == n).map(|s| s.expansion.clone())
}

// ---------------------------------------------------------------------------
// The rule pipeline
// ---------------------------------------------------------------------------

pub fn rule_clean(text: &str, opts: &CleanOptions) -> String {
    let text = strip_whisper_artifacts(text);
    if text.is_empty() {
        return text;
    }
    let mut toks = tokens(&text);
    toks = apply_corrections(toks);
    toks = remove_fillers(toks);
    toks = remove_stutters(toks);
    toks = apply_spoken_commands(toks, opts.spoken_punctuation || opts.style == Style::Code);

    let out = if opts.style == Style::Code {
        code_transform(toks)
    } else {
        toks = format_lists(toks);
        let joined = join(&toks);
        let capped = capitalize_sentences(&joined);
        finish_punctuation(&capped, opts.style)
    };
    let out = apply_dictionary(&out, &opts.dictionary);
    // Tidy spacing left over by removals: " ," -> ","
    out.replace(" ,", ",").replace(" .", ".").replace(",,", ",").trim().to_string()
}

// ---------------------------------------------------------------------------
// LLM prompts and output checks
// ---------------------------------------------------------------------------

pub const CLEANUP_PROMPT: &str = include_str!("prompts/cleanup.md");
pub const COMMAND_PROMPT: &str = include_str!("prompts/command.md");

pub fn style_instructions(style: Style) -> &'static str {
    match style {
        Style::Neutral => "Style: clear, natural written text with standard punctuation.",
        Style::Casual => "Style: casual chat message. Keep it relaxed; contractions are fine; a trailing period on a one-line message is optional.",
        Style::Formal => "Style: professional email or document. Use complete sentences and proper paragraphs. Do not add a greeting or sign-off unless spoken.",
        Style::Code => "Style: the destination is a code editor or terminal. Be literal: keep identifiers, commands, file names and symbols exactly as spoken, do not add prose punctuation or capitalization to code, and never wrap the result in code fences.",
    }
}

pub fn build_system_prompt(style: Style, dict: &[DictEntry], app_name: &str) -> String {
    let mut p = String::from(CLEANUP_PROMPT.trim());
    p.push_str("\n\n");
    p.push_str(style_instructions(style));
    if !app_name.trim().is_empty() {
        p.push_str(&format!("\nThe text will be inserted into: {}.", app_name.trim()));
    }
    let words: Vec<&str> = dict.iter().map(|e| e.word.trim()).filter(|w| !w.is_empty()).take(150).collect();
    if !words.is_empty() {
        p.push_str(&format!(
            "\nPersonal dictionary - always use these exact spellings when the transcript refers to them: {}.",
            words.join(", ")
        ));
    }
    p
}

pub fn build_user_message(transcript: &str) -> String {
    format!("<transcript>\n{}\n</transcript>", transcript.trim())
}

pub fn build_command_messages(selection: &str, instruction: &str, app_name: &str) -> (String, String) {
    let mut system = String::from(COMMAND_PROMPT.trim());
    if !app_name.trim().is_empty() {
        system.push_str(&format!("\nDestination app: {}.", app_name.trim()));
    }
    let user = if selection.trim().is_empty() {
        format!("<instruction>\n{}\n</instruction>\n<selected_text></selected_text>", instruction.trim())
    } else {
        format!(
            "<instruction>\n{}\n</instruction>\n<selected_text>\n{}\n</selected_text>",
            instruction.trim(),
            selection
        )
    };
    (system, user)
}

fn strip_think(s: &str) -> String {
    let mut out = s.to_string();
    while let Some(start) = out.find("<think>") {
        if let Some(end_rel) = out[start..].find("</think>") {
            out.replace_range(start..start + end_rel + "</think>".len(), "");
        } else {
            out.replace_range(start.., "");
        }
    }
    out
}

fn strip_wrappers(s: &str, input: &str) -> String {
    let mut t = strip_think(s).trim().to_string();
    for tag in ["<transcript>", "</transcript>", "<selected_text>", "</selected_text>", "<output>", "</output>"] {
        t = t.replace(tag, "");
    }
    t = t.trim().to_string();
    // Code fences the input didn't ask for.
    if t.starts_with("```") && !input.contains("```") {
        let inner = t.trim_start_matches("```");
        let inner = match inner.find('\n') {
            Some(nl) if !inner[..nl].contains(' ') => &inner[nl + 1..],
            _ => inner,
        };
        t = inner.trim_end().trim_end_matches("```").trim().to_string();
    }
    // Chatty preambles.
    let lower = t.to_lowercase();
    for pre in [
        "here is the cleaned text:",
        "here's the cleaned text:",
        "here is the cleaned-up text:",
        "here's the cleaned-up text:",
        "here is the cleaned up text:",
        "cleaned text:",
        "cleaned transcript:",
        "here is the text:",
        "here's the text:",
        "here is the result:",
        "here's the result:",
        "output:",
    ] {
        if lower.starts_with(pre) {
            t = t[pre.len()..].trim().to_string();
            break;
        }
    }
    // Wrapping quotes the speaker didn't say.
    let input_t = input.trim();
    if t.len() >= 2
        && t.starts_with('"')
        && t.ends_with('"')
        && !input_t.starts_with('"')
        && t.matches('"').count() == 2
    {
        t = t[1..t.len() - 1].trim().to_string();
    }
    t
}

fn looks_like_refusal(s: &str) -> bool {
    let l = s.to_lowercase();
    ["i'm sorry", "i am sorry", "i cannot", "i can't help", "as an ai", "i'm unable", "i am unable"]
        .iter()
        .any(|p| l.starts_with(p))
}

/// Validate a clean-up response. Returns None if the model clearly did
/// something other than cleaning (answered a question, refused, rambled).
pub fn sanitize_llm_output(input: &str, output: &str) -> Option<String> {
    let t = strip_wrappers(output, input);
    if t.is_empty() {
        return None;
    }
    if looks_like_refusal(&t) && !looks_like_refusal(input) {
        return None;
    }
    let in_len = input.chars().count();
    let out_len = t.chars().count();
    if out_len > in_len * 2 + 80 {
        return None;
    }
    // A clean-up should never shrink text to a fraction unless it was mostly filler.
    if in_len > 60 && out_len * 5 < in_len {
        return None;
    }
    Some(t)
}

/// Command-mode responses may legitimately be much longer than the input.
pub fn sanitize_command_output(output: &str) -> Option<String> {
    let t = strip_wrappers(output, "");
    if t.is_empty() || looks_like_refusal(&t) {
        None
    } else {
        Some(t)
    }
}

// ---------------------------------------------------------------------------
// Per-app style detection
// ---------------------------------------------------------------------------

pub fn detect_style(app_name: &str, window_title: &str, rules: &[AppRule], default: Style) -> Style {
    let app = app_name.to_lowercase();
    let title = window_title.to_lowercase();
    for r in rules {
        let p = r.pattern.trim().to_lowercase();
        if !p.is_empty() && (app.contains(&p) || title.contains(&p)) {
            return r.style;
        }
    }
    const CODE: &[&str] = &[
        "code",
        "cursor",
        "windsurf",
        "zed",
        "sublime",
        "xcode",
        "intellij",
        "pycharm",
        "webstorm",
        "goland",
        "rider",
        "clion",
        "android studio",
        "terminal",
        "iterm",
        "warp",
        "alacritty",
        "kitty",
        "wezterm",
        "ghostty",
        "powershell",
        "cmd.exe",
        "windowsterminal",
        "konsole",
        "gnome-terminal",
        "vim",
        "emacs",
        "neovim",
        "nvim",
        "hyper",
    ];
    const CASUAL: &[&str] = &[
        "slack",
        "discord",
        "whatsapp",
        "telegram",
        "messages",
        "messenger",
        "signal",
        "wechat",
        "line",
        "viber",
        "instagram",
        "imessage",
    ];
    const FORMAL: &[&str] = &["mail", "outlook", "thunderbird", "spark", "superhuman", "word", "pages", "docs"];

    let has = |list: &[&str], s: &str| list.iter().any(|k| contains_word(s, k));
    // Browser tabs: look at the window title.
    if has(&["gmail", "outlook", "proton mail", "fastmail"], &title) {
        return Style::Formal;
    }
    if has(&["slack", "discord", "whatsapp", "messenger", "telegram"], &title) && !has(CODE, &app) {
        return Style::Casual;
    }
    if has(CODE, &app) {
        return Style::Code;
    }
    if has(CASUAL, &app) {
        return Style::Casual;
    }
    if has(FORMAL, &app) {
        return Style::Formal;
    }
    default
}

/// Substring match on word-ish boundaries ("code" matches "Visual Studio Code"
/// and "code.exe" but not "barcode").
fn contains_word(hay: &str, needle: &str) -> bool {
    let h: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    for i in 0..=h.len() - n.len() {
        if h[i..i + n.len()] == n[..] {
            let left_ok = i == 0 || !h[i - 1].is_alphanumeric();
            let right_ok = i + n.len() == h.len() || !h[i + n.len()].is_alphanumeric();
            if left_ok && right_ok {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Learning from edits
// ---------------------------------------------------------------------------

/// Words the user typed into a corrected transcript that look like names or
/// jargon worth adding to the personal dictionary.
pub fn suggest_dictionary_words(original: &str, edited: &str) -> Vec<String> {
    let orig: HashSet<String> = original.split_whitespace().map(norm).collect();
    let mut seen = HashSet::new();
    let mut out = vec![];
    let edited_words: Vec<&str> = edited.split_whitespace().collect();
    for (idx, raw) in edited_words.iter().enumerate() {
        let w = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-' && c != '.');
        let w = w.trim_end_matches('.');
        if w.chars().count() < 2 {
            continue;
        }
        let n = w.to_lowercase();
        if orig.contains(&n) || !seen.insert(n.clone()) {
            continue;
        }
        let sentence_start = idx == 0 || ends_sentence(edited_words[idx - 1]);
        let has_inner_upper = w.chars().skip(1).any(|c| c.is_uppercase());
        let has_digit = w.chars().any(|c| c.is_ascii_digit());
        let capitalized = w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        let interesting = has_inner_upper || has_digit || (capitalized && !sentence_start) || w.chars().count() >= 8;
        if interesting {
            out.push(w.to_string());
        }
        if out.len() >= 10 {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(s: &str) -> String {
        rule_clean(s, &CleanOptions::default())
    }
    fn clean_style(s: &str, style: Style) -> String {
        rule_clean(s, &CleanOptions { style, ..Default::default() })
    }

    #[test]
    fn removes_fillers() {
        assert_eq!(
            clean("um so I was thinking we could uh meet tomorrow"),
            "So I was thinking we could meet tomorrow."
        );
        assert_eq!(clean("I, uh, think it's fine."), "I think it's fine.");
        assert_eq!(clean("So, um, I think we should go."), "So I think we should go.");
        assert_eq!(clean("You know, it's really, like, hard."), "It's really hard.");
        assert_eq!(clean("Do you know where it is?"), "Do you know where it is?");
        assert_eq!(clean("It was good, um."), "It was good.");
    }

    #[test]
    fn keeps_meaningful_like() {
        assert_eq!(clean("I like apples"), "I like apples.");
    }

    #[test]
    fn resolves_self_corrections() {
        assert_eq!(clean("Let's meet at 5, no wait, 6."), "Let's meet at 6.");
        assert_eq!(clean("let's meet tuesday no wait let's meet wednesday"), "Let's meet wednesday.");
        assert_eq!(clean("Send it to John, no sorry, to Mike."), "Send it to Mike.");
        assert_eq!(clean("Go left or rather right at the corner."), "Go right at the corner.");
        assert_eq!(
            clean("I love this plan. Actually it's terrible, scratch that. Let's ship it."),
            "I love this plan. Let's ship it."
        );
        assert_eq!(clean("Scratch that. Hello there"), "Hello there.");
    }

    #[test]
    fn no_at_sentence_start_is_not_a_correction() {
        assert_eq!(clean("Is it ready? No, wait for me."), "Is it ready? No, wait for me.");
    }

    #[test]
    fn removes_stutters() {
        assert_eq!(clean("I I think the the plan works"), "I think the plan works.");
        assert_eq!(clean("that that is fine"), "That that is fine.");
    }

    #[test]
    fn spoken_commands() {
        assert_eq!(clean("hello there new line how are you question mark"), "Hello there\nHow are you?");
        assert_eq!(clean("hello there new paragraph thanks for coming"), "Hello there\n\nThanks for coming.");
        let opts = CleanOptions { spoken_punctuation: true, ..Default::default() };
        assert_eq!(rule_clean("hi comma how are you period", &opts), "Hi, how are you.");
        // Without the setting, "period" stays a word.
        assert_eq!(clean("the trial period ends today"), "The trial period ends today.");
    }

    #[test]
    fn formats_lists() {
        assert_eq!(
            clean("I need three things, first, eggs, second, milk, and third, bread."),
            "I need three things:\n1. Eggs\n2. Milk\n3. Bread"
        );
        assert_eq!(clean("Number one review the PR, number two merge it."), "1. Review the PR\n2. Merge it");
        // A single "first" is not a list.
        assert_eq!(clean("At first I thought it was fine."), "At first I thought it was fine.");
    }

    #[test]
    fn list_with_trailing_sentence() {
        assert_eq!(
            clean("Agenda: first, budget, second, hiring. Let me know if that works."),
            "Agenda:\n1. Budget\n2. Hiring\n\nLet me know if that works."
        );
    }

    #[test]
    fn capitalization_and_i() {
        assert_eq!(clean("i think i'm ready. are you"), "I think I'm ready. Are you.");
    }

    #[test]
    fn casual_drops_single_trailing_period() {
        assert_eq!(clean_style("sounds good see you there.", Style::Casual), "Sounds good see you there");
        assert_eq!(clean_style("Yes. See you there.", Style::Casual), "Yes. See you there.");
    }

    #[test]
    fn code_mode() {
        assert_eq!(clean_style("git status.", Style::Code), "git status");
        assert_eq!(clean_style("camel case user id equals five", Style::Code), "userId = five");
        assert_eq!(clean_style("print open paren hello close paren", Style::Code), "print(hello)");
        assert_eq!(clean_style("snake case max retry count", Style::Code), "max_retry_count");
        assert_eq!(clean_style("constant case api key", Style::Code), "API_KEY");
        assert_eq!(clean_style("pascal case user service dot get user", Style::Code), "UserService.get user");
        assert_eq!(clean_style("npm run dev dash dash port 3000", Style::Code), "npm run dev --port 3000");
        assert_eq!(clean_style("um ls dash la", Style::Code), "ls -la");
    }

    #[test]
    fn dictionary() {
        let dict = vec![
            DictEntry { word: "Kubernetes".into(), variants: vec!["cooper netties".into(), "kubernetis".into()] },
            DictEntry { word: "Anthropic".into(), variants: vec![] },
        ];
        let opts = CleanOptions { dictionary: dict.clone(), ..Default::default() };
        assert_eq!(
            rule_clean("deploy it on cooper netties at anthropic", &opts),
            "Deploy it on Kubernetes at Anthropic."
        );
        // Whole words only.
        assert_eq!(replace_phrase("anthropical", "anthropic", "Anthropic"), "anthropical");
        assert_eq!(whisper_prompt(&dict), "Glossary: Kubernetes, Anthropic.");
    }

    #[test]
    fn artifacts_and_hallucinations() {
        assert_eq!(strip_whisper_artifacts("[BLANK_AUDIO] hello (music) world"), "hello world");
        assert_eq!(strip_whisper_artifacts("call f(x) now"), "call f(x) now");
        assert_eq!(strip_whisper_artifacts("*laughs* okay then"), "okay then");
        assert!(is_hallucination(" Thank you."));
        assert!(is_hallucination(
            "[BLANK_AUDIO]".trim_matches(|c| c == '[' || c == ']').replace("BLANK_AUDIO", "").as_str()
        ));
        assert!(!is_hallucination("Thank you for the update."));
    }

    #[test]
    fn snippets() {
        let snips = vec![
            Snippet { trigger: "my calendar link".into(), expansion: "https://cal.com/me".into() },
            Snippet { trigger: "my email".into(), expansion: "me@example.com".into() },
        ];
        let (p, map) = protect_snippets("Book a slot with my calendar link or write to my email.", &snips);
        assert_eq!(p, "Book a slot with [[S1]] or write to [[S2]].");
        assert_eq!(
            restore_snippets(&p, &map).unwrap(),
            "Book a slot with https://cal.com/me or write to me@example.com."
        );
        assert!(restore_snippets("Book a slot.", &map).is_none());
        assert_eq!(exact_snippet("My calendar link.", &snips).unwrap(), "https://cal.com/me");
        assert!(exact_snippet("send my calendar link", &snips).is_none());
    }

    #[test]
    fn llm_output_guard() {
        let input = "um what's the capital of france";
        assert_eq!(
            sanitize_llm_output(input, "What's the capital of France?").unwrap(),
            "What's the capital of France?"
        );
        // Model answered the question at length instead of cleaning it.
        let long = "The capital of France is Paris. Paris is known for the Eiffel Tower, the Louvre, and much more. It has been the capital since the 10th century and is the largest city in the country.";
        assert!(sanitize_llm_output(input, long).is_none());
        assert_eq!(
            sanitize_llm_output("hello world", "Here is the cleaned text: Hello world.").unwrap(),
            "Hello world."
        );
        assert_eq!(sanitize_llm_output("hello world", "\"Hello world.\"").unwrap(), "Hello world.");
        assert_eq!(sanitize_llm_output("hello world", "<think>hmm</think>\nHello world.").unwrap(), "Hello world.");
        assert!(sanitize_llm_output("hello", "I'm sorry, I can't help with that.").is_none());
        assert_eq!(sanitize_command_output("```\nfoo\n```").unwrap(), "foo");
    }

    #[test]
    fn prompts() {
        let p = build_system_prompt(Style::Code, &[DictEntry { word: "Tauri".into(), variants: vec![] }], "Cursor");
        assert!(p.contains("dictation clean-up engine"));
        assert!(p.contains("code editor"));
        assert!(p.contains("Tauri"));
        assert!(p.contains("Cursor"));
        assert_eq!(build_user_message(" hi "), "<transcript>\nhi\n</transcript>");
        let (_, u) = build_command_messages("", "write a haiku about rain", "");
        assert!(u.contains("<selected_text></selected_text>"));
    }

    #[test]
    fn style_detection() {
        let rules = vec![AppRule { pattern: "notion".into(), style: Style::Formal }];
        assert_eq!(detect_style("Visual Studio Code", "", &[], Style::Neutral), Style::Code);
        assert_eq!(detect_style("Slack", "", &[], Style::Neutral), Style::Casual);
        assert_eq!(detect_style("Google Chrome", "Inbox (3) - Gmail", &[], Style::Neutral), Style::Formal);
        assert_eq!(detect_style("Notion", "", &rules, Style::Neutral), Style::Formal);
        assert_eq!(detect_style("Barcode Scanner", "", &[], Style::Neutral), Style::Neutral);
        assert_eq!(detect_style("Finder", "", &[], Style::Casual), Style::Casual);
    }

    #[test]
    fn dictionary_suggestions() {
        let s = suggest_dictionary_words(
            "ask dana about the cooper netties cluster",
            "Ask Dana about the Kubernetes cluster",
        );
        assert_eq!(s, vec!["Kubernetes".to_string()]);
        let s = suggest_dictionary_words("meeting with jon", "meeting with Jonh at OpenRouter");
        assert_eq!(s, vec!["Jonh".to_string(), "OpenRouter".to_string()]);
    }

    #[test]
    fn word_counting() {
        assert_eq!(word_count("Hello, world - again!"), 3);
    }

    /// Realistic messy transcripts end to end through the rule engine.
    #[test]
    fn end_to_end_samples() {
        let cases: &[(&str, &str)] = &[
            (
                "um hey Sarah, so uh I wanted to check if we're still on for, like, lunch on Friday, no wait, Thursday",
                "Hey Sarah, so I wanted to check if we're still on for lunch on Thursday.",
            ),
            (
                "okay so the plan is first we fix the login bug second we write the tests and third we ship it",
                "Okay so the plan is:\n1. We fix the login bug\n2. We write the tests\n3. We ship it",
            ),
            ("Thanks so much new line Best new line Alex", "Thanks so much\nBest\nAlex."),
        ];
        for (input, expected) in cases {
            assert_eq!(&clean(input), expected, "input: {input}");
        }
    }
}
