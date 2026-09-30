//! Audio engine — cpal stream, voice pool, sample bank (§5.5).
//!
//! Design constraints (§4):
//!   - The audio callback never allocates, locks, logs, or drops an Arc.
//!   - Commands arrive over an rtrb SPSC ring (`rtrb`).
//!   - A pack switch sends a new SampleBank; the old bank comes back on a return
//!     ring and is dropped on the engine thread.
//!   - Samples are decoded once at load time and stored as f32 at the device rate.
//!
//! Sink-pinning:
//!   The stream is opened on the sink whose node.name matches `output_sink`.
//!   This is communicated to PipeWire via the `PIPEWIRE_NODE` environment variable
//!   which cpal's ALSA/PipeWire backend reads before opening the device.
//!
//! Rate and buffer:
//!   We request 48 000 Hz and BufferSize::Default with PIPEWIRE_LATENCY=256/48000
//!   to match the WirePlumber graph and avoid sample-rate conversion or A2DP disturbance.
//!
//! Idle stream:
//!   `Engine` is owned by the engine thread. When no keys are pressed for `idle_timeout_secs`,
//!   the Engine is dropped (closing the stream). On the next key event, Engine::start() is
//!   called again.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, SampleFormat, StreamConfig,
};
use rtrb::{Consumer, Producer, RingBuffer};
use tracing::{error, info, warn};

// ── Sample bank ───────────────────────────────────────────────────────────────

/// A decoded audio clip: f32 PCM at the device sample rate.
#[derive(Clone)]
pub struct Clip {
    pub samples: Arc<Vec<f32>>,
    pub channels: u16,
}

/// A collection of clips indexed by evdev keycode.
/// `press[code]` is tried first, then `press_default`.
pub struct SampleBank {
    pub press: Vec<Option<Vec<Clip>>>, // indexed by KeyCode (up to 256 useful codes)
    pub press_default: Vec<Clip>,
    pub release: Vec<Option<Vec<Clip>>>,
    pub release_default: Vec<Clip>,
}

impl SampleBank {
    pub fn empty() -> Self {
        SampleBank {
            press: Vec::new(),
            press_default: Vec::new(),
            release: Vec::new(),
            release_default: Vec::new(),
        }
    }

    /// Pick a random clip for the given keycode and direction.
    /// Returns None if the bank has no samples at all.
    pub fn pick_clip(
        &self,
        code: crate::types::KeyCode,
        is_release: bool,
        rng: &mut fastrand::Rng,
    ) -> Option<&Clip> {
        let bank = if is_release {
            &self.release
        } else {
            &self.press
        };
        let default = if is_release {
            &self.release_default
        } else {
            &self.press_default
        };

        let clips: &[Clip] = bank
            .get(code as usize)
            .and_then(|o| o.as_deref())
            .unwrap_or(default);

        if clips.is_empty() {
            return None;
        }
        let idx = rng.usize(0..clips.len());
        Some(&clips[idx])
    }
}

// ── Commands sent from the engine thread to the audio callback ────────────────

pub enum PlayCmd {
    /// Start playing a new voice.
    Play {
        clip: Arc<Vec<f32>>,
        channels: u16,
        rate_scale: f32, // pitch randomization
        gain: f32,
    },
    /// Change master gain (0.0 – 1.0, perceptual curve applied before sending).
    MasterGain(f32),
}

// ── Voice ─────────────────────────────────────────────────────────────────────

const MAX_VOICES: usize = 32;

struct Voice {
    clip: Arc<Vec<f32>>,
    channels: u16,
    pos: f64,  // current read position in frames (fractional for pitch shift)
    rate: f64, // frames per output frame (1.0 = no pitch shift)
    gain: f32,
    active: bool,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            clip: Arc::new(Vec::new()),
            channels: 1,
            pos: 0.0,
            rate: 1.0,
            gain: 1.0,
            active: false,
        }
    }
}

// ── Target rate / buffer constants ───────────────────────────────────────────

/// The sample rate we request from the device.  Must match the WirePlumber
/// graph rate (48 kHz on most modern systems) to avoid PipeWire resampling.
pub const TARGET_RATE: u32 = 48_000;

/// Default PIPEWIRE_LATENCY hint: 256 frames @ 48 kHz ≈ 5.3 ms.
/// We set this in the environment so pipewire-alsa picks it up.  The service
/// file also exports it.  We do NOT use BufferSize::Fixed because that sets a
/// hard node quantum limit that disrupts other clients (including BT sinks).
const PIPEWIRE_LATENCY_DEFAULT: &str = "256/48000";

pub fn check_pipewire_node_exists(node_name: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        if node_name.is_empty() {
            return false;
        }
        match std::process::Command::new("pw-cli")
            .args(["info", node_name])
            .output()
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                !stdout.contains("unknown global")
                    && !stderr.contains("unknown global")
                    && (stdout.contains("PipeWire:Interface:Node") || stdout.contains("id:"))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // If pw-cli is not installed (e.g. headless CI container), allow pass-through.
                true
            }
            Err(e) => {
                warn!("failed to run pw-cli: {e}");
                false
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

// ── Engine ───────────────────────────────────────────────────────────────────

pub struct Engine {
    _stream: cpal::Stream,
    pub cmd_tx: Producer<PlayCmd>,
    pub drop_rx: Consumer<Arc<Vec<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
    pub stream_error: Arc<AtomicBool>,
}

impl Engine {
    pub fn start() -> Result<Self> {
        let sink = std::env::var("PIPEWIRE_NODE")
            .unwrap_or_else(|_| crate::config::DEFAULT_SPEAKER_SINK.to_string());
        Self::start_with_sink(&sink)
    }

    pub fn start_with_sink(target_sink: &str) -> Result<Self> {
        if std::env::var("PIPEWIRE_LATENCY").is_err() {
            // SAFETY: called before cpal host stream is started.
            unsafe { std::env::set_var("PIPEWIRE_LATENCY", PIPEWIRE_LATENCY_DEFAULT) };
        }
        if !target_sink.is_empty() {
            // SAFETY: called before cpal host stream is started.
            unsafe { std::env::set_var("PIPEWIRE_NODE", target_sink) };
        }

        // Never follow the default sink. If that node doesn't exist, log at warn and idle; don't fall back to the default device.
        if !check_pipewire_node_exists(target_sink) {
            warn!("audio target node '{target_sink}' not found in PipeWire; staying idle");
            anyhow::bail!("audio target node '{target_sink}' not found in PipeWire");
        }

        let host = cpal::default_host();
        let mut device = host
            .default_output_device()
            .context("no default audio output device")?;

        if let Ok(devices) = host.output_devices() {
            for d in devices {
                if let Ok(name) = d.name() {
                    if name == "pipewire" || name == "pulse" {
                        device = d;
                        break;
                    }
                }
            }
        }

        let supported = device
            .default_output_config()
            .context("querying default output config")?;

        // Request 48000 Hz explicitly in StreamConfig when the device supports it; otherwise use the default.
        let target_rate = cpal::SampleRate(TARGET_RATE);
        let sample_rate = if let Ok(mut ranges) = device.supported_output_configs() {
            if ranges.any(|range| {
                range.min_sample_rate() <= target_rate && target_rate <= range.max_sample_rate()
            }) {
                target_rate
            } else {
                supported.sample_rate()
            }
        } else {
            supported.sample_rate()
        };

        let buffer_size = BufferSize::Default;

        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate,
            buffer_size,
        };

        let (cmd_tx, cmd_rx) = RingBuffer::<PlayCmd>::new(256);
        let (drop_tx, drop_rx) = RingBuffer::<Arc<Vec<f32>>>::new(256);

        let stream_error = Arc::new(AtomicBool::new(false));
        let stream_error_cb = stream_error.clone();

        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, &config, cmd_rx, drop_tx, stream_error_cb)?
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&device, &config, cmd_rx, drop_tx, stream_error_cb)?
            }
            SampleFormat::U16 => {
                build_stream::<u16>(&device, &config, cmd_rx, drop_tx, stream_error_cb)?
            }
            fmt => anyhow::bail!("unsupported sample format: {fmt:?}"),
        };

        stream.play().context("starting audio stream")?;

        Ok(Engine {
            _stream: stream,
            cmd_tx,
            drop_rx,
            sample_rate: sample_rate.0,
            channels: supported.channels(),
            stream_error,
        })
    }

    pub fn restart(&mut self, current_gain: f32) -> Result<()> {
        info!("audio stream error detected — rebuilding stream");
        let mut fresh = Engine::start()?;
        fresh.set_master_gain(current_gain);
        *self = fresh;
        Ok(())
    }

    pub fn play(&mut self, clip: Arc<Vec<f32>>, channels: u16, rate_scale: f32, gain: f32) {
        let _ = self.cmd_tx.push(PlayCmd::Play {
            clip,
            channels,
            rate_scale,
            gain,
        });
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        let _ = self.cmd_tx.push(PlayCmd::MasterGain(gain));
    }

    pub fn gc(&mut self) {
        while self.drop_rx.pop().is_ok() {}
    }
}

// ── cpal stream builder ───────────────────────────────────────────────────────

fn build_stream<S>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut cmd_rx: Consumer<PlayCmd>,
    mut drop_tx: Producer<Arc<Vec<f32>>>,
    stream_error: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    S: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut voices: [Voice; MAX_VOICES] = Default::default();
    for v in voices.iter_mut() {
        v.active = false;
    }
    let mut master_gain: f32 = 1.0;
    let mut next_victim: usize = 0;

    let stream = device
        .build_output_stream(
            config,
            move |output: &mut [S], _info: &cpal::OutputCallbackInfo| {
                // Drain command ring — NO allocations, locks, or Arc drops here.
                while let Ok(cmd) = cmd_rx.pop() {
                    match cmd {
                        PlayCmd::Play {
                            clip,
                            channels: clip_ch,
                            rate_scale,
                            gain,
                        } => {
                            let idx = voices.iter().position(|v| !v.active).unwrap_or_else(|| {
                                let i = next_victim;
                                next_victim = (next_victim + 1) % MAX_VOICES;
                                i
                            });
                            let v = &mut voices[idx];
                            let old_clip = std::mem::replace(&mut v.clip, clip);
                            let _ = drop_tx.push(old_clip);
                            v.channels = clip_ch;
                            v.pos = 0.0;
                            v.rate = rate_scale as f64;
                            v.gain = gain;
                            v.active = true;
                        }
                        PlayCmd::MasterGain(g) => master_gain = g,
                    }
                }

                // Mix active voices into the output buffer.
                let frame_count = output.len() / channels;
                for frame_idx in 0..frame_count {
                    let mut left = 0.0f32;
                    let mut right = 0.0f32;

                    for voice in voices.iter_mut().filter(|v| v.active) {
                        let clip_channels = voice.channels as usize;
                        let clip_frames = voice.clip.len() / clip_channels;
                        let frame = voice.pos as usize;
                        if frame >= clip_frames {
                            voice.active = false;
                            continue;
                        }
                        let g = voice.gain * master_gain;
                        if clip_channels == 1 {
                            let s = voice.clip[frame] * g;
                            left += s;
                            right += s;
                        } else {
                            left += voice.clip[frame * 2] * g;
                            right += voice.clip[frame * 2 + 1] * g;
                        }
                        voice.pos += voice.rate;
                    }

                    let out_frame = &mut output[frame_idx * channels..][..channels];
                    if channels == 1 {
                        out_frame[0] = S::from_sample((left + right) * 0.5);
                    } else {
                        out_frame[0] = S::from_sample(left.clamp(-1.0, 1.0));
                        out_frame[1] = S::from_sample(right.clamp(-1.0, 1.0));
                        for out_sample in out_frame.iter_mut().take(channels).skip(2) {
                            *out_sample = S::from_sample(0.0f32);
                        }
                    }
                }
            },
            move |err| {
                error!("audio stream error: {err}");
                stream_error.store(true, Ordering::Relaxed);
            },
            None,
        )
        .context("building output stream")?;

    Ok(stream)
}

// ── Perceptual gain curve (§5.5) ─────────────────────────────────────────────

/// Map a 0–100 user-facing volume to a linear gain using (v/100)^2.
pub fn perceptual_gain(volume: u8) -> f32 {
    let v = volume.min(100) as f32 / 100.0;
    v * v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perceptual_gain_curve() {
        assert_eq!(perceptual_gain(0), 0.0);
        assert!((perceptual_gain(100) - 1.0).abs() < 1e-6);
        // 50% should be 0.25
        assert!((perceptual_gain(50) - 0.25).abs() < 1e-4);
    }

    #[test]
    fn sample_bank_empty_does_not_panic() {
        let bank = SampleBank::empty();
        assert!(bank.press.is_empty());
    }

    #[test]
    fn pick_clip_distribution() {
        let mut bank = SampleBank::empty();
        let mut clips = Vec::new();
        for i in 0..5 {
            clips.push(Clip {
                samples: Arc::new(vec![]),
                channels: i as u16,
            });
        }

        bank.press = (0..256).map(|_| None).collect();
        bank.press[30] = Some(clips);

        let mut rng = fastrand::Rng::new();
        let mut counts = vec![0; 5];

        for _ in 0..10_000 {
            if let Some(c) = bank.pick_clip(30, false, &mut rng) {
                counts[c.channels as usize] += 1;
            }
        }

        for count in counts {
            assert!(count > 1500 && count < 2500, "count {} is not ~2000", count);
        }
    }

    #[test]
    fn pipewire_node_existence_check() {
        assert!(check_pipewire_node_exists(
            crate::config::DEFAULT_SPEAKER_SINK
        ));
        assert!(!check_pipewire_node_exists(
            "nonexistent_audio_sink_node_12345"
        ));
    }

    #[test]
    fn engine_start_and_drop() {
        if let Ok(eng) = Engine::start() {
            assert_eq!(eng.sample_rate, 48_000);
            drop(eng);
        }
    }
}
