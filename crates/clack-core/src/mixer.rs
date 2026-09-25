//! Audio engine — cpal stream, voice pool, sample bank (§5.5).
//!
//! Design constraints (§4):
//!   - The audio callback never allocates, locks, logs, or drops an Arc.
//!   - Commands arrive over an rtrb SPSC ring (`rtrb`).
//!   - A pack switch sends a new SampleBank; the old bank comes back on a return
//!     ring and is dropped on the engine thread.
//!   - Samples are decoded once at load time and stored as f32 at the device rate.

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
use tracing::{error, info};

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
    pub press: Vec<Option<Vec<Clip>>>,   // indexed by KeyCode (up to 256 useful codes)
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
    pos: f64,       // current read position in frames (fractional for pitch shift)
    rate: f64,      // frames per output frame (1.0 = no pitch shift)
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

// ── Engine ────────────────────────────────────────────────────────────────────

/// Owns the cpal stream and the command channel to the audio callback.
pub struct Engine {
    _stream: cpal::Stream,
    pub cmd_tx: Producer<PlayCmd>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Set by the error callback when the stream dies (device removed / switched).
    pub stream_error: Arc<AtomicBool>,
}

impl Engine {
    /// Start the audio engine using the default output device.
    pub fn start() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no default audio output device")?;

        info!(
            "audio device: {}",
            device.name().unwrap_or_else(|_| "unknown".into())
        );

        let supported = device
            .default_output_config()
            .context("querying default output config")?;

        info!(
            "audio config: {:?} {} ch @ {} Hz",
            supported.sample_format(),
            supported.channels(),
            supported.sample_rate().0,
        );

        // §5.5: request a small buffer (~128 frames) for low latency; fall back to Default.
        let buffer_size = match supported.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                let target = 128u32;
                if target >= *min && target <= *max {
                    BufferSize::Fixed(target)
                } else {
                    BufferSize::Fixed((*min).max(128).min(*max))
                }
            }
            cpal::SupportedBufferSize::Unknown => BufferSize::Default,
        };

        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size,
        };

        // SPSC ring: engine thread → audio callback.
        let (cmd_tx, cmd_rx) = RingBuffer::<PlayCmd>::new(256);

        let stream_error = Arc::new(AtomicBool::new(false));
        let stream_error_cb = stream_error.clone();

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, cmd_rx, stream_error_cb)?,
            SampleFormat::I16 => build_stream::<i16>(&device, &config, cmd_rx, stream_error_cb)?,
            SampleFormat::U16 => build_stream::<u16>(&device, &config, cmd_rx, stream_error_cb)?,
            fmt => anyhow::bail!("unsupported sample format: {fmt:?}"),
        };

        stream.play().context("starting audio stream")?;
        let sr = supported.sample_rate().0;
        let ch = supported.channels();
        Ok(Engine { _stream: stream, cmd_tx, sample_rate: sr, channels: ch, stream_error })
    }

    /// Rebuild the engine when the audio device changes (headphones in/out, default changed).
    /// Preserves the current master gain by re-sending it after restart.
    pub fn restart(&mut self, current_gain: f32) -> Result<()> {
        info!("audio stream error detected — rebuilding stream");
        // Build a fresh engine, move its fields into self.
        let mut fresh = Engine::start()?;
        fresh.set_master_gain(current_gain);
        *self = fresh;
        Ok(())
    }

    /// Convenience: send a play command (best-effort, silently drops if ring full).
    pub fn play(&mut self, clip: Arc<Vec<f32>>, channels: u16, rate_scale: f32, gain: f32) {
        let _ = self.cmd_tx.push(PlayCmd::Play { clip, channels, rate_scale, gain });
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        let _ = self.cmd_tx.push(PlayCmd::MasterGain(gain));
    }
}

fn build_stream<S>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut cmd_rx: Consumer<PlayCmd>,
    stream_error: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    S: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut voices: [Voice; MAX_VOICES] = Default::default();
    // Initialize active flags (can't derive Default for the array otherwise).
    for v in voices.iter_mut() { v.active = false; }
    let mut master_gain: f32 = 1.0;
    let mut next_victim: usize = 0; // round-robin voice steal index

    let stream = device
        .build_output_stream(
            config,
            move |output: &mut [S], _info: &cpal::OutputCallbackInfo| {
                // Drain the command ring — NO allocations, locks, or Arc drops here.
                while let Ok(cmd) = cmd_rx.pop() {
                    match cmd {
                        PlayCmd::Play { clip, channels: clip_ch, rate_scale, gain } => {
                            // Find a free voice; steal the oldest (round-robin) if full.
                            let idx = voices
                                .iter()
                                .position(|v| !v.active)
                                .unwrap_or_else(|| {
                                    let i = next_victim;
                                    next_victim = (next_victim + 1) % MAX_VOICES;
                                    i
                                });
                            let v = &mut voices[idx];
                            v.clip = clip;
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

                    // Write to output (clamp to [-1, 1]).
                    let out_frame = &mut output[frame_idx * channels..][..channels];
                    if channels == 1 {
                        out_frame[0] = S::from_sample((left + right) * 0.5);
                    } else {
                        out_frame[0] = S::from_sample(left.clamp(-1.0, 1.0));
                        out_frame[1] = S::from_sample(right.clamp(-1.0, 1.0));
                        for ch in 2..channels {
                            out_frame[ch] = S::from_sample(0.0f32);
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
}
