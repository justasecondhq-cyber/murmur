//! Speech-to-text with whisper.cpp (via whisper-rs). Runs fully on-device.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

struct Loaded {
    path: PathBuf,
    ctx: Arc<WhisperContext>,
}

#[derive(Default)]
pub struct Engine {
    loaded: Mutex<Option<Loaded>>,
}

impl Engine {
    pub fn new() -> Self {
        Engine { loaded: Mutex::new(None) }
    }

    pub fn is_loaded(&self, path: &Path) -> bool {
        self.loaded.lock().map(|l| l.as_ref().map(|x| x.path == path).unwrap_or(false)).unwrap_or(false)
    }

    /// Load (or reuse) the model at `path`.
    pub fn context(&self, path: &Path) -> Result<Arc<WhisperContext>> {
        let mut guard = self.loaded.lock().map_err(|_| anyhow!("model lock poisoned"))?;
        if let Some(l) = guard.as_ref() {
            if l.path == path {
                return Ok(l.ctx.clone());
            }
        }
        if !path.exists() {
            return Err(anyhow!("The speech model isn't downloaded yet. Open Murmur → Settings → Speech model."));
        }
        let path_str = path.to_str().ok_or_else(|| anyhow!("Model path is not valid UTF-8"))?;
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| anyhow!("Couldn't load the speech model: {e:?}"))?;
        let ctx = Arc::new(ctx);
        *guard = Some(Loaded { path: path.to_path_buf(), ctx: ctx.clone() });
        Ok(ctx)
    }

    pub fn unload(&self) {
        if let Ok(mut g) = self.loaded.lock() {
            *g = None;
        }
    }

    /// Transcribe 16 kHz mono audio.
    pub fn transcribe(
        &self,
        model_path: &Path,
        samples: &[f32],
        language: &str,
        english_only: bool,
        prompt: &str,
    ) -> Result<String> {
        let ctx = self.context(model_path)?;
        let mut state = ctx.create_state().map_err(|e| anyhow!("whisper state: {e:?}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8) as i32;
        params.set_n_threads(threads);
        let lang = if english_only { "en".to_string() } else { language.to_string() };
        params.set_language(Some(lang.as_str()));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_token_timestamps(false);
        if !prompt.is_empty() {
            params.set_initial_prompt(prompt);
        }

        state.full(params, samples).map_err(|e| anyhow!("transcription failed: {e:?}"))?;
        let n = state.full_n_segments().map_err(|e| anyhow!("{e:?}"))?;
        let mut out = String::new();
        for i in 0..n {
            if let Ok(seg) = state.full_get_segment_text(i) {
                out.push_str(&seg);
            }
        }
        Ok(out.trim().to_string())
    }
}
