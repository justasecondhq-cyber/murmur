//! Whisper model catalog and downloader.

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub file: &'static str,
    pub size_mb: u32,
    pub english_only: bool,
    /// Rough guidance shown in the UI.
    pub speed: &'static str,
    pub accuracy: &'static str,
    pub note: &'static str,
}

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

pub const CATALOG: &[ModelInfo] = &[
    ModelInfo {
        id: "tiny.en",
        name: "Tiny (English)",
        file: "ggml-tiny.en.bin",
        size_mb: 75,
        english_only: true,
        speed: "Fastest",
        accuracy: "Basic",
        note: "For older laptops. Expect more mistakes.",
    },
    ModelInfo {
        id: "base.en",
        name: "Base (English)",
        file: "ggml-base.en.bin",
        size_mb: 142,
        english_only: true,
        speed: "Very fast",
        accuracy: "Good",
        note: "Recommended starting point for English.",
    },
    ModelInfo {
        id: "base",
        name: "Base (multilingual)",
        file: "ggml-base.bin",
        size_mb: 142,
        english_only: false,
        speed: "Very fast",
        accuracy: "Good",
        note: "99 languages, including mixed speech like Taglish.",
    },
    ModelInfo {
        id: "small.en",
        name: "Small (English)",
        file: "ggml-small.en.bin",
        size_mb: 466,
        english_only: true,
        speed: "Fast",
        accuracy: "Great",
        note: "Noticeably better with names and jargon.",
    },
    ModelInfo {
        id: "small",
        name: "Small (multilingual)",
        file: "ggml-small.bin",
        size_mb: 466,
        english_only: false,
        speed: "Fast",
        accuracy: "Great",
        note: "Best balance for non-English speech.",
    },
    ModelInfo {
        id: "large-v3-turbo-q5_0",
        name: "Large v3 Turbo (compressed)",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size_mb: 547,
        english_only: false,
        speed: "Medium",
        accuracy: "Excellent",
        note: "Near top accuracy. Best on Apple Silicon or a GPU.",
    },
    ModelInfo {
        id: "large-v3-turbo",
        name: "Large v3 Turbo",
        file: "ggml-large-v3-turbo.bin",
        size_mb: 1620,
        english_only: false,
        speed: "Slower",
        accuracy: "Excellent",
        note: "Maximum accuracy. Needs a strong machine.",
    },
];

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    CATALOG.iter().find(|m| m.id == id)
}

pub fn models_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models")
}

pub fn model_path(data_dir: &Path, id: &str) -> Option<PathBuf> {
    find(id).map(|m| models_dir(data_dir).join(m.file))
}

pub fn is_installed(data_dir: &Path, id: &str) -> bool {
    model_path(data_dir, id).map(|p| p.exists()).unwrap_or(false)
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    #[serde(flatten)]
    pub info: ModelInfo,
    pub installed: bool,
}

pub fn list(data_dir: &Path) -> Vec<ModelStatus> {
    CATALOG.iter().map(|m| ModelStatus { info: m.clone(), installed: is_installed(data_dir, m.id) }).collect()
}

/// Download a model, calling `progress(downloaded, total)` as it goes.
pub fn download(data_dir: &Path, id: &str, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf> {
    let info = find(id).ok_or_else(|| anyhow!("Unknown model {id}"))?;
    let dir = models_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(info.file);
    let tmp = dir.join(format!("{}.part", info.file));

    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()?;
    let mut resp = client.get(format!("{BASE_URL}{}", info.file)).send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("Download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(info.size_mb as u64 * 1_048_576);
    let mut file = std::fs::File::create(&tmp)?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut last_report = 0u64;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if done - last_report > total / 200 + 1 {
            progress(done, total);
            last_report = done;
        }
    }
    file.flush()?;
    drop(file);
    if done < 1_000_000 {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow!("Download was incomplete. Check your connection and try again."));
    }
    std::fs::rename(&tmp, &dest)?;
    progress(total, total);
    Ok(dest)
}

pub fn delete(data_dir: &Path, id: &str) -> Result<()> {
    if let Some(p) = model_path(data_dir, id) {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}
