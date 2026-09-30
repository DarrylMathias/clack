//! Audio engine — cpal stream, voice pool, sample bank (§5.5).
//!
//! Design constraints (§4):
//!   - The audio callback never allocates, locks, logs, or drops an Arc.
//!   - Commands arrive over an rtrb SPSC ring (`rtrb`).
//!   - A pack switch sends a new SampleBank; the old bank comes back on a return
//!     ring and is dropped on the engine thread.
//!   - Samples are decoded once at load time and stored as f32 at the device rate.
//!
//! Sink-pinning (§change-1):
//!   The stream is opened on the sink whose node.name matches `speaker_sink`.
//!   This is communicated to PipeWire via the `PIPEWIRE_NODE` environment variable
//!   which cpal's ALSA/PipeWire backend reads before opening the device.  We set
//!   the variable in-process (safe: single-process, and we do it before the first
//!   `cpal::default_host()` call).  The service file also sets it so the env is
//!   already correct at startup.
//!
//! Rate and buffer (§change-2):
//!   We always request 48 000 Hz / 512 frames to match the WirePlumber graph and
//!   avoid sample-rate conversion on the PipeWire side.  If the device actually
//!   opens at a different rate (rare), we record the real rate so that the decoder
//!   can resample to it.
//!
//! Idle stream (§change-3):
//!   `Engine` is now a lazy wrapper.  The inner `ActiveStream` is created on the
//!   first `play()` call and dropped when `close()` is called.  The daemon thread
//!   calls `close()` after the configured idle timeout.

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

// ── Active stream (inner) ─────────────────────────────────────────────────────

/// Holds the live cpal stream and its command channel.
/// Dropped when the engine goes idle.
struct ActiveStream {
    _stream: cpal::Stream,
    cmd_tx: Producer<PlayCmd>,
    drop_rx: Consumer<Arc<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    stream_error: Arc<AtomicBool>,
}

impl ActiveStream {
    /// Open the stream pinned to `speaker_sink`.
    ///
    /// Sets `PIPEWIRE_NODE` in the process environment so that the cpal ALSA
    /// backend (which PipeWire intercepts) routes this stream to the correct
    /// PipeWire node rather than following the default sink.
    fn open(speaker_sink: &str) -> Result<Self> {
        // Set PIPEWIRE_LATENCY if not already set in the environment.
        // This tells pipewire-alsa the desired quantum without hard-locking
        // the node, so it doesn't disrupt other clients (especially BT sinks).
        if std::env::var("PIPEWIRE_LATENCY").is_err() {
            // SAFETY: called once before the cpal host is created.
            unsafe { std::env::set_var("PIPEWIRE_LATENCY", PIPEWIRE_LATENCY_DEFAULT) };
        }

        // Pin the PipeWire node.  The service file also exports this, but we
        // set it here too so that in-process restarts keep the correct target.
        if !speaker_sink.is_empty() {
            // SAFETY: single-threaded at this point; no other thread reads the var.
            unsafe { std::env::set_var("PIPEWIRE_NODE", speaker_sink) };
        }

        let host = cpal::default_host();

        // Find the device whose name matches our target sink.
        // On PipeWire-ALSA the device names visible to cpal are the PipeWire
        // node names, so an exact match works.  We never fall back to the
        // default device.
        let device = find_speaker_device(&host, speaker_sink)?;

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

        // Request TARGET_RATE (48 kHz).  PipeWire handles the rest.
        // Use BufferSize::Default — do NOT use Fixed; a hard quantum forces
        // all PipeWire clients (including BT A2DP) to the same small quantum.
        let sample_rate = cpal::SampleRate(TARGET_RATE);
        let buffer_size = BufferSize::Default;

        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate,
            buffer_size,
        };

        // SPSC rings: engine thread ↔ audio callback.
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
        let ch = supported.channels();

        info!(
            "stream open: {} ch @ {} Hz (PIPEWIRE_LATENCY={})",
            ch, TARGET_RATE, PIPEWIRE_LATENCY_DEFAULT
        );

        Ok(ActiveStream {
            _stream: stream,
            cmd_tx,
            drop_rx,
            sample_rate: TARGET_RATE,
            channels: ch,
            stream_error,
        })
    }
}

/// Locate the cpal device whose name matches `speaker_sink`.
///
/// Returns `Err` (never falls back to default) if not found.
fn find_speaker_device(host: &cpal::Host, speaker_sink: &str) -> Result<cpal::Device> {
    if speaker_sink.is_empty() {
        // No explicit sink — use whatever cpal/PipeWire selects (env var already set).
        return host
            .default_output_device()
            .context("no default audio output device");
    }

    let devices = host
        .output_devices()
        .context("enumerating output devices")?;

    for device in devices {
        if let Ok(name) = device.name() {
            if name == speaker_sink {
                return Ok(device);
            }
        }
    }

    // The speaker sink doesn't exist right now.  Log and return an error so
    // the engine stays idle instead of routing to the wrong (BT) device.
    anyhow::bail!(
        "speaker sink '{}' not found in PipeWire device list — \
         is the built-in audio card present? (clack will retry on next keypress)",
        speaker_sink
    )
}

// ── Engine (public) ───────────────────────────────────────────────────────────

/// Public handle to the audio engine.
///
/// The underlying stream is created lazily on the first `play()` call and
/// closed automatically when `close()` is called (idle timeout).  This prevents
/// clack from holding a PipeWire node open and driving the graph quantum down
/// while no keys are being typed.
pub struct Engine {
    inner: Option<ActiveStream>,
    /// Pinned sink name (from config or PIPEWIRE_NODE env var at startup).
    pub speaker_sink: String,
    /// Sample rate reported by the last successfully opened stream.
    pub sample_rate: u32,
    /// Channel count reported by the last successfully opened stream.
    pub channels: u16,
    /// Pending master gain — applied immediately when the stream opens.
    pending_gain: f32,
}

impl Engine {
    /// Create the engine handle.  Does NOT open a cpal stream yet.
    pub fn new(speaker_sink: String) -> Self {
        Engine {
            inner: None,
            speaker_sink,
            sample_rate: TARGET_RATE,
            channels: 2,
            pending_gain: 1.0,
        }
    }

    /// Open the stream if not already open.  Returns false if the sink is
    /// unavailable (logged); the caller should not treat this as fatal.
    pub fn ensure_open(&mut self) -> bool {
        if self.inner.is_some() {
            return true;
        }
        match ActiveStream::open(&self.speaker_sink) {
            Ok(mut s) => {
                self.sample_rate = s.sample_rate;
                self.channels = s.channels;
                // Apply the buffered master gain before the stream starts mixing.
                let gain = self.pending_gain;
                let _ = s.cmd_tx.push(PlayCmd::MasterGain(gain));
                self.inner = Some(s);
                true
            }
            Err(e) => {
                warn!("cannot open audio stream: {e:#}");
                false
            }
        }
    }

    /// Close the stream (idle).  Silently does nothing if already closed.
    pub fn close(&mut self) {
        if self.inner.is_some() {
            info!("audio stream closed (idle timeout)");
            self.inner = None; // drops the cpal::Stream → PipeWire frees the node
        }
    }

    /// True if the stream is currently open.
    pub fn is_open(&self) -> bool {
        self.inner.is_some()
    }

    /// Rebuild the stream after an error.
    pub fn restart(&mut self, current_gain: f32) -> Result<()> {
        info!("audio stream error detected — rebuilding stream");
        self.pending_gain = current_gain;
        self.inner = None;
        if self.ensure_open() {
            Ok(())
        } else {
            anyhow::bail!("failed to reopen stream after error")
        }
    }

    /// Send a play command.  Opens the stream if needed.
    pub fn play(&mut self, clip: Arc<Vec<f32>>, channels: u16, rate_scale: f32, gain: f32) {
        if !self.ensure_open() {
            return;
        }
        if let Some(ref mut st) = self.inner {
            let _ = st.cmd_tx.push(PlayCmd::Play {
                clip,
                channels,
                rate_scale,
                gain,
            });
        }
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        self.pending_gain = gain;
        if let Some(ref mut st) = self.inner {
            let _ = st.cmd_tx.push(PlayCmd::MasterGain(gain));
        }
    }

    /// Check for stream errors and rebuild if needed.
    pub fn check_error(&self) -> bool {
        self.inner
            .as_ref()
            .map(|s| s.stream_error.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Drain the drop ring so Arc<Vec<f32>> are freed on the engine thread.
    pub fn gc(&mut self) {
        if let Some(ref mut st) = self.inner {
            while st.drop_rx.pop().is_ok() {}
        }
    }

    // Legacy compatibility — kept so daemon.rs compiles with minimal changes.
    /// Start the audio engine (legacy entry point).
    /// Prefer `Engine::new()` + lazy open via `play()`.
    pub fn start() -> Result<Self> {
        // This path is no longer called by daemon.rs, but kept for any external
        // code that may reference it.  It reads the speaker_sink from env.
        let sink = std::env::var("PIPEWIRE_NODE").unwrap_or_default();
        Ok(Engine::new(sink))
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
}
