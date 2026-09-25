//! Pack loading and audio decoding (§5.4).
//!
//! Loads a pack from disk, decodes all OGG/WAV/MP3/FLAC files with `symphonia`,
//! and resamples (nearest-neighbour) to the target sample rate.
//! The result is a `SampleBank` ready for the audio engine.

use std::{fs, path::Path, sync::Arc};

use anyhow::{bail, Context, Result};
use symphonia::core::{
    audio::{AudioBufferRef, Signal},
    codecs::DecoderOptions,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use tracing::{debug, warn};

use crate::{
    mixer::{Clip, SampleBank},
    pack::{Manifest, SoundRef},
};

// ── Public API ────────────────────────────────────────────────────────────────

/// Load a pack directory into a `SampleBank` at the given sample rate.
pub fn load_pack(pack_dir: &Path, target_rate: u32) -> Result<SampleBank> {
    let manifest = Manifest::load(pack_dir)
        .with_context(|| format!("loading manifest in {}", pack_dir.display()))?;

    // Decode the sprite (if sprite mode) or all per-key files.
    let sprite_decoded: Option<(Vec<f32>, u32, u16)> = if let Some(ref sprite) = manifest.sprite {
        let sprite_path = pack_dir.join(sprite);
        debug!("decoding sprite: {}", sprite_path.display());
        let (samples, rate, channels) = decode_file(&sprite_path)
            .with_context(|| format!("decoding sprite {}", sprite_path.display()))?;
        Some((samples, rate, channels))
    } else {
        None
    };

    let resolve = |sound_ref: &SoundRef| -> Vec<Clip> {
        resolve_clip(sound_ref, pack_dir, sprite_decoded.as_ref(), target_rate).unwrap_or_default()
    };

    // Build press map.
    let mut press_map: Vec<Option<Vec<Clip>>> = vec![None; 256];
    let press_default = manifest
        .press
        .default
        .as_ref()
        .map(&resolve)
        .unwrap_or_default();

    for (code_str, sound_ref) in &manifest.press.keys {
        if let Ok(code) = code_str.parse::<usize>() {
            if code < 256 {
                press_map[code] = Some(resolve(sound_ref));
            }
        }
    }

    // Build release map (optional).
    let (release_map, release_default) = if let Some(ref rel) = manifest.release {
        let def = rel.default.as_ref().map(&resolve).unwrap_or_default();
        let mut map: Vec<Option<Vec<Clip>>> = vec![None; 256];
        for (code_str, sound_ref) in &rel.keys {
            if let Ok(code) = code_str.parse::<usize>() {
                if code < 256 {
                    map[code] = Some(resolve(sound_ref));
                }
            }
        }
        (map, def)
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(SampleBank {
        press: press_map,
        press_default,
        release: release_map,
        release_default,
    })
}

// ── Clip resolution ───────────────────────────────────────────────────────────

fn resolve_clip(
    sound_ref: &SoundRef,
    pack_dir: &Path,
    sprite: Option<&(Vec<f32>, u32, u16)>,
    target_rate: u32,
) -> Result<Vec<Clip>> {
    match sound_ref {
        SoundRef::Sprite([start_ms, dur_ms]) => {
            let (sprite_samples, sprite_rate, channels) =
                sprite.context("manifest uses sprite ranges but no sprite file loaded")?;
            let start_frame = (*start_ms as f64 * *sprite_rate as f64 / 1000.0) as usize;
            let dur_frames = (*dur_ms as f64 * *sprite_rate as f64 / 1000.0) as usize;
            let end_frame =
                (start_frame + dur_frames).min(sprite_samples.len() / *channels as usize);
            let ch = *channels as usize;
            let raw: Vec<f32> = sprite_samples[start_frame * ch..end_frame * ch].to_vec();
            let resampled = resample(&raw, *sprite_rate, target_rate, *channels);
            Ok(vec![Clip {
                samples: Arc::new(resampled),
                channels: *channels,
            }])
        }
        SoundRef::File(rel_path) => {
            let path = pack_dir.join(rel_path);
            let (raw, src_rate, channels) =
                decode_file(&path).with_context(|| format!("decoding {}", path.display()))?;
            let resampled = resample(&raw, src_rate, target_rate, channels);
            Ok(vec![Clip {
                samples: Arc::new(resampled),
                channels,
            }])
        }
        SoundRef::Multi(refs) => {
            let mut clips = Vec::new();
            for r in refs {
                clips.extend(resolve_clip(r, pack_dir, sprite, target_rate)?);
            }
            Ok(clips)
        }
    }
}

// ── Audio decoding via symphonia ──────────────────────────────────────────────

/// Decode any audio file into interleaved f32 samples.
/// Returns (samples, sample_rate, channels).
fn decode_file(path: &Path) -> Result<(Vec<f32>, u32, u16)> {
    let file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("probing {}", path.display()))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .context("no audio track found")?;

    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("creating decoder")?;

    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(symphonia::core::errors::Error::ResetRequired) => {
                warn!("decoder reset required");
                break;
            }
            Err(e) => bail!("packet error: {e}"),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet).context("decoding packet")?;
        push_samples(&decoded, &mut samples);
    }

    Ok((samples, sample_rate, channels))
}

fn push_samples(buf: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    use symphonia::core::audio::AudioBufferRef::*;
    match buf {
        F32(b) => {
            for frame in 0..b.frames() {
                for ch in 0..b.spec().channels.count() {
                    out.push(b.chan(ch)[frame]);
                }
            }
        }
        S16(b) => {
            for frame in 0..b.frames() {
                for ch in 0..b.spec().channels.count() {
                    out.push(b.chan(ch)[frame] as f32 / i16::MAX as f32);
                }
            }
        }
        S32(b) => {
            for frame in 0..b.frames() {
                for ch in 0..b.spec().channels.count() {
                    out.push(b.chan(ch)[frame] as f32 / i32::MAX as f32);
                }
            }
        }
        U8(b) => {
            for frame in 0..b.frames() {
                for ch in 0..b.spec().channels.count() {
                    out.push((b.chan(ch)[frame] as f32 / 127.5) - 1.0);
                }
            }
        }
        _ => {
            // S24, F64, etc. — convert to f32 via the sample trait
            warn!("unsupported audio buffer format — output may be silence");
        }
    }
}

// ── Nearest-neighbour resampler ───────────────────────────────────────────────
//
// Good enough for short click sounds; rubato can replace this if needed (D7).

fn resample(samples: &[f32], src_rate: u32, dst_rate: u32, channels: u16) -> Vec<f32> {
    if src_rate == dst_rate {
        return samples.to_vec();
    }
    let ch = channels as usize;
    let src_frames = samples.len() / ch;
    let ratio = src_rate as f64 / dst_rate as f64;
    let dst_frames = (src_frames as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(dst_frames * ch);
    for i in 0..dst_frames {
        let src_frame = ((i as f64 * ratio) as usize).min(src_frames - 1);
        for c in 0..ch {
            out.push(samples[src_frame * ch + c]);
        }
    }
    out
}
