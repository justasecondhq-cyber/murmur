//! Optional local HTTP API so scripts and other tools can use Murmur's
//! on-device transcription and clean-up. Binds to 127.0.0.1 only.
//!
//!   GET  /v1/health
//!   POST /v1/transcribe?style=neutral&clean=1   (body: WAV file)
//!   POST /v1/clean   {"text": "...", "style": "casual"}

use std::io::{Cursor, Read};
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::AppHandle;
use tiny_http::{Header, Method, Response, Server};

use crate::dictation;
use crate::dsp;
use crate::text::Style;

pub struct ApiServer {
    server: Arc<Server>,
    pub port: u16,
}

impl ApiServer {
    pub fn stop(self) {
        self.server.unblock();
    }
}

fn json_response(status: u16, v: Value) -> Response<Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("static header");
    Response::from_data(v.to_string().into_bytes()).with_status_code(status).with_header(header)
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| v.to_string())
    })
}

fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| format!("Not a WAV file: {e}"))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / scale).collect()
        }
    };
    let mono = dsp::downmix(&samples, channels);
    Ok(dsp::resample(&mono, spec.sample_rate, dsp::TARGET_RATE))
}

fn handle(app: &AppHandle, mut req: tiny_http::Request) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    let mut body = Vec::new();
    let _ = req.as_reader().take(100 * 1024 * 1024).read_to_end(&mut body);

    let resp = match (method, path.as_str()) {
        (Method::Get, "/v1/health") => json_response(200, json!({ "ok": true, "app": "murmur" })),
        (Method::Post, "/v1/transcribe") => match decode_wav(&body) {
            Err(e) => json_response(400, json!({ "error": e })),
            Ok(samples) => match dictation::transcribe(app, samples) {
                Err(e) => json_response(500, json!({ "error": e })),
                Ok(None) => json_response(200, json!({ "raw": "", "text": "" })),
                Ok(Some(raw)) => {
                    let style = Style::parse(&query_param(&url, "style").unwrap_or_default());
                    let do_clean = query_param(&url, "clean").map(|v| v != "0").unwrap_or(true);
                    let text = if do_clean { dictation::clean(app, &raw, style, "").text } else { raw.clone() };
                    json_response(200, json!({ "raw": raw, "text": text }))
                }
            },
        },
        (Method::Post, "/v1/clean") => match serde_json::from_slice::<Value>(&body) {
            Ok(v) => {
                let t = v["text"].as_str().unwrap_or("");
                let style = Style::parse(v["style"].as_str().unwrap_or("neutral"));
                let r = dictation::clean(app, t, style, "");
                json_response(200, json!({ "text": r.text, "provider": r.provider }))
            }
            Err(e) => json_response(400, json!({ "error": format!("Expected JSON: {e}") })),
        },
        _ => json_response(404, json!({ "error": "Not found. Try GET /v1/health" })),
    };
    let _ = req.respond(resp);
}

pub fn start(app: AppHandle, port: u16) -> Result<ApiServer, String> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| format!("Couldn't open port {port}: {e}"))?;
    let server = Arc::new(server);
    let s = server.clone();
    std::thread::spawn(move || {
        for req in s.incoming_requests() {
            let a = app.clone();
            std::thread::spawn(move || handle(&a, req));
        }
    });
    Ok(ApiServer { server, port })
}
