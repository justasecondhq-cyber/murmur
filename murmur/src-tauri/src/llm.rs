//! Optional AI clean-up through a local (Ollama) or cloud (OpenAI-compatible,
//! Anthropic) language model. Off-device providers are only used when the user
//! explicitly picks them and adds a key.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::Duration;

use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    None,
    Ollama { url: String, model: String },
    OpenAi { base_url: String, key: String, model: String },
    Anthropic { key: String, model: String },
}

impl Provider {
    pub fn label(&self) -> String {
        match self {
            Provider::None => "Rules only".into(),
            Provider::Ollama { model, .. } => format!("Ollama · {model}"),
            Provider::OpenAi { model, .. } => format!("OpenAI-compatible · {model}"),
            Provider::Anthropic { model, .. } => format!("Anthropic · {model}"),
        }
    }
    pub fn is_local(&self) -> bool {
        match self {
            Provider::Ollama { url, .. } => url.contains("127.0.0.1") || url.contains("localhost"),
            Provider::OpenAi { base_url, .. } => base_url.contains("127.0.0.1") || base_url.contains("localhost"),
            Provider::None => true,
            Provider::Anthropic { .. } => false,
        }
    }
}

fn client(timeout_secs: u64) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_millis(1500))
        .build()?)
}

/// Names of models Ollama has pulled, or Err if Ollama isn't reachable.
pub fn ollama_models(url: &str) -> Result<Vec<String>> {
    let resp: Value =
        client(2)?.get(format!("{}/api/tags", url.trim_end_matches('/'))).send()?.error_for_status()?.json()?;
    Ok(resp["models"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default())
}

fn ollama_has_model(installed: &[String], wanted: &str) -> bool {
    installed
        .iter()
        .any(|m| m == wanted || m.trim_end_matches(":latest") == wanted || wanted.trim_end_matches(":latest") == m)
}

/// Pull a model through Ollama (blocking; can take minutes).
pub fn ollama_pull(url: &str, model: &str) -> Result<()> {
    client(60 * 60)?
        .post(format!("{}/api/pull", url.trim_end_matches('/')))
        .json(&json!({ "model": model, "name": model, "stream": false }))
        .send()?
        .error_for_status()?;
    Ok(())
}

/// Work out which provider to use right now.
pub fn resolve(s: &Settings) -> Provider {
    let ollama = || Provider::Ollama { url: s.ollama_url.clone(), model: s.ollama_model.clone() };
    match s.llm_provider.as_str() {
        "none" => Provider::None,
        "ollama" => ollama(),
        "openai" if !s.openai_model.trim().is_empty() => Provider::OpenAi {
            base_url: s.openai_base_url.clone(),
            key: s.openai_api_key.clone(),
            model: s.openai_model.clone(),
        },
        "anthropic" if !s.anthropic_api_key.trim().is_empty() => {
            Provider::Anthropic { key: s.anthropic_api_key.clone(), model: s.anthropic_model.clone() }
        }
        "auto" => match ollama_models(&s.ollama_url) {
            Ok(models) if ollama_has_model(&models, &s.ollama_model) => ollama(),
            _ => Provider::None,
        },
        _ => Provider::None,
    }
}

/// Some local models wrap answers in reasoning tags; callers sanitize further.
pub fn chat(provider: &Provider, system: &str, user: &str, timeout_secs: u64) -> Result<String> {
    let c = client(timeout_secs)?;
    match provider {
        Provider::None => Err(anyhow!("no language model configured")),
        Provider::Ollama { url, model } => {
            let body = json!({
                "model": model,
                "stream": false,
                "keep_alive": "30m",
                "options": { "temperature": 0.0 },
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user }
                ]
            });
            let v: Value = c
                .post(format!("{}/api/chat", url.trim_end_matches('/')))
                .json(&body)
                .send()?
                .error_for_status()?
                .json()?;
            v["message"]["content"].as_str().map(|s| s.to_string()).ok_or_else(|| anyhow!("Ollama returned no text"))
        }
        Provider::OpenAi { base_url, key, model } => {
            let body = json!({
                "model": model,
                "temperature": 0.0,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user }
                ]
            });
            let mut req = c.post(format!("{}/chat/completions", base_url.trim_end_matches('/'))).json(&body);
            if !key.trim().is_empty() {
                req = req.bearer_auth(key.trim());
            }
            let resp = req.send()?;
            let status = resp.status();
            let v: Value = resp.json()?;
            if !status.is_success() {
                return Err(anyhow!("{}: {}", status, v["error"]["message"].as_str().unwrap_or("request failed")));
            }
            v["choices"][0]["message"]["content"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow!("No text in response"))
        }
        Provider::Anthropic { key, model } => {
            let body = json!({
                "model": model,
                "max_tokens": 2048,
                "temperature": 0.0,
                "system": system,
                "messages": [ { "role": "user", "content": user } ]
            });
            let resp = c
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", key.trim())
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()?;
            let status = resp.status();
            let v: Value = resp.json()?;
            if !status.is_success() {
                return Err(anyhow!("{}: {}", status, v["error"]["message"].as_str().unwrap_or("request failed")));
            }
            let text: String = v["content"]
                .as_array()
                .map(|blocks| blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(""))
                .unwrap_or_default();
            if text.is_empty() {
                Err(anyhow!("No text in response"))
            } else {
                Ok(text)
            }
        }
    }
}

/// Human-readable status for the Settings screen.
pub fn status(s: &Settings) -> Value {
    let models = ollama_models(&s.ollama_url);
    let ollama_running = models.is_ok();
    let installed = models.unwrap_or_default();
    let has_model = ollama_has_model(&installed, &s.ollama_model);
    let active = resolve(s);
    json!({
        "active": active.label(),
        "active_is_local": active.is_local(),
        "using_llm": active != Provider::None,
        "ollama_running": ollama_running,
        "ollama_models": installed,
        "ollama_has_model": has_model,
    })
}
