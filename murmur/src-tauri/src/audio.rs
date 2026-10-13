//! Microphone capture with cpal.
//!
//! cpal streams are not `Send` on every platform, so each recording owns its
//! stream on a dedicated thread. Audio is downmixed to mono as it arrives and
//! resampled to 16 kHz when recording stops.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::dsp;

#[derive(Clone)]
struct Sink {
    buffer: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
    channels: usize,
}

impl Sink {
    fn push(&self, mono_or_interleaved: Vec<f32>) {
        let mono = dsp::downmix(&mono_or_interleaved, self.channels);
        let level = dsp::rms(&mono);
        self.level.store(level.to_bits(), Ordering::Relaxed);
        if let Ok(mut b) = self.buffer.lock() {
            b.extend_from_slice(&mono);
        }
    }
}

struct Active {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    rate: u32,
}

pub struct Recorder {
    buffer: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
    active: Mutex<Option<Active>>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devs) => devs.filter_map(|d| d.name().ok()).collect(),
        Err(_) => vec![],
    }
}

fn pick_device(preferred: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = preferred {
        if let Ok(mut devs) = host.input_devices() {
            if let Some(d) = devs.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                return Ok(d);
            }
        }
    }
    host.default_input_device().ok_or_else(|| anyhow!("No microphone found. Plug one in or check your sound settings."))
}

fn build_stream<T, F>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sink: Sink,
    convert: F,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + Copy + 'static,
    F: Fn(T) -> f32 + Send + 'static,
{
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            sink.push(data.iter().map(|s| convert(*s)).collect());
        },
        |err| eprintln!("[murmur] audio stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

fn open_stream(
    preferred: Option<&str>,
    sink_buffer: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
) -> Result<(cpal::Stream, u32)> {
    let device = pick_device(preferred)?;
    let supported = device.default_input_config()?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let sink = Sink { buffer: sink_buffer, level, channels: config.channels as usize };
    let rate = config.sample_rate.0;
    let stream = match format {
        cpal::SampleFormat::F32 => build_stream::<f32, _>(&device, &config, sink, |s| s)?,
        cpal::SampleFormat::I16 => build_stream::<i16, _>(&device, &config, sink, |s| s as f32 / 32768.0)?,
        cpal::SampleFormat::U16 => build_stream::<u16, _>(&device, &config, sink, |s| (s as f32 - 32768.0) / 32768.0)?,
        cpal::SampleFormat::I32 => build_stream::<i32, _>(&device, &config, sink, |s| s as f32 / 2_147_483_648.0)?,
        other => return Err(anyhow!("Unsupported microphone sample format: {other:?}")),
    };
    Ok((stream, rate))
}

impl Recorder {
    pub fn new() -> Self {
        Recorder {
            buffer: Arc::new(Mutex::new(Vec::new())),
            level: Arc::new(AtomicU32::new(0)),
            active: Mutex::new(None),
        }
    }

    pub fn is_recording(&self) -> bool {
        self.active.lock().map(|a| a.is_some()).unwrap_or(false)
    }

    /// Current input level (RMS, 0..1).
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn start(&self, preferred_device: Option<String>) -> Result<()> {
        let mut active = self.active.lock().map_err(|_| anyhow!("recorder lock poisoned"))?;
        if active.is_some() {
            return Ok(());
        }
        if let Ok(mut b) = self.buffer.lock() {
            b.clear();
        }
        self.level.store(0f32.to_bits(), Ordering::Relaxed);

        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<u32, String>>();
        let buffer = self.buffer.clone();
        let level = self.level.clone();
        let stop_flag = stop.clone();
        let handle = std::thread::spawn(move || {
            match open_stream(preferred_device.as_deref(), buffer, level) {
                Ok((stream, rate)) => {
                    if let Err(e) = stream.play() {
                        let _ = ready_tx.send(Err(format!("Couldn't start the microphone: {e}")));
                        return;
                    }
                    let _ = ready_tx.send(Ok(rate));
                    while !stop_flag.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    // Let the last callback land.
                    std::thread::sleep(Duration::from_millis(60));
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                }
            }
        });

        let rate = ready_rx
            .recv_timeout(Duration::from_secs(4))
            .map_err(|_| anyhow!("The microphone didn't respond. Check that Murmur has microphone permission."))?
            .map_err(|e| anyhow!(e))?;
        *active = Some(Active { stop, handle, rate });
        Ok(())
    }

    /// Stop recording and return 16 kHz mono samples plus the raw duration in ms.
    pub fn stop(&self) -> (Vec<f32>, u32) {
        let taken = self.active.lock().ok().and_then(|mut a| a.take());
        let Some(active) = taken else {
            return (vec![], 0);
        };
        active.stop.store(true, Ordering::Relaxed);
        let _ = active.handle.join();
        self.level.store(0f32.to_bits(), Ordering::Relaxed);
        let raw = self.buffer.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default();
        let duration_ms = (raw.len() as u64 * 1000 / active.rate.max(1) as u64) as u32;
        (dsp::resample(&raw, active.rate, dsp::TARGET_RATE), duration_ms)
    }

    /// Stop and throw the audio away (e.g. the user cancelled).
    pub fn cancel(&self) {
        let _ = self.stop();
    }
}
