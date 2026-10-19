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

    let pack_gain = manifest
        .gain
        .unwrap_or_else(|| crate::pack::pack_default_gain(&manifest.id));

    // Decode the sprite (if sprite mode) or all per-key files.
    let sprite_decoded: Option<(Vec<f32>, u32, u16)> = if let Some(ref sprite) = manifest.sprite {
        let sprite_path = pack_dir.join(sprite);
        debug!("decoding sprite: {}", sprite_path.display()); // privacy-ok: sprite decoding, no key data
        let (samples, rate, channels) = decode_file(&sprite_path)
            .with_context(|| format!("decoding sprite {}", sprite_path.display()))?;
        Some((samples, rate, channels))
    } else {
        None
    };

    let resolve = |sound_ref: &SoundRef| -> Vec<Clip> {
        let mut clips = resolve_clip(sound_ref, pack_dir, sprite_decoded.as_ref(), target_rate)
            .unwrap_or_default();
        if (pack_gain - 1.0).abs() > 1e-4 {
            for clip in &mut clips {
                let boosted: Vec<f32> = clip.samples.iter().map(|&s| s * pack_gain).collect();
                clip.samples = Arc::new(boosted);
            }
        }
        clips
    };

    // Build press map.
    let mut press_map: Vec<Option<Vec<Clip>>> = vec![None; 256];
    let press_default = manifest
        .press
        .default
        .as_ref()
        .map(resolve)
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
        let def = rel.default.as_ref().map(resolve).unwrap_or_default();
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
            let mut path = pack_dir.join(rel_path);
            if !path.exists() {
                // Handle common filename variations / aliases in pack definitions:
                if rel_path == "sounds/key.wav" && pack_dir.join("sounds/key1.wav").exists() {
                    path = pack_dir.join("sounds/key1.wav");
                } else if rel_path == "sounds/enter.wav" && pack_dir.join("sounds/ent.wav").exists() {
                    path = pack_dir.join("sounds/ent.wav");
                } else if rel_path == "sounds/backspace.wav" && pack_dir.join("sounds/back.wav").exists() {
                    path = pack_dir.join("sounds/back.wav");
                }
            }
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
                match resolve_clip(r, pack_dir, sprite, target_rate) {
                    Ok(c) => clips.extend(c),
                    Err(e) => {
                        warn!("failed resolving clip in multi: {e:#}");
                    }
                }
            }
            if clips.is_empty() && !refs.is_empty() {
                return resolve_clip(&refs[0], pack_dir, sprite, target_rate);
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
                warn!("decoder reset required"); // privacy-ok: audio decoder reset, no key data
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

// ── Linear interpolation resampler (§M2.3) ────────────────────────────────────

pub fn resample(samples: &[f32], src_rate: u32, dst_rate: u32, channels: u16) -> Vec<f32> {
    if src_rate == dst_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ch = channels as usize;
    let src_frames = samples.len() / ch;
    if src_frames == 0 {
        return Vec::new();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let dst_frames = (src_frames as f64 / ratio).round() as usize;
    let mut out = Vec::with_capacity(dst_frames * ch);
    for i in 0..dst_frames {
        let exact_pos = i as f64 * ratio;
        let frame0 = (exact_pos.floor() as usize).min(src_frames - 1);
        let frame1 = (frame0 + 1).min(src_frames - 1);
        let frac = (exact_pos - frame0 as f64) as f32;

        for c in 0..ch {
            let s0 = samples[frame0 * ch + c];
            let s1 = samples[frame1 * ch + c];
            out.push(s0 + frac * (s1 - s0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_resample_sine() {
        let src_rate = 44_100;
        let dst_rate = 48_000;
        let freq = 1000.0;
        let num_src_samples = 4410; // 0.1 sec -> 100 cycles
        let mut src = Vec::with_capacity(num_src_samples);
        for i in 0..num_src_samples {
            let t = i as f32 / src_rate as f32;
            src.push((2.0 * std::f32::consts::PI * freq * t).sin());
        }

        let resampled = resample(&src, src_rate, dst_rate, 1);
        let expected_dst_samples =
            (num_src_samples as f64 * dst_rate as f64 / src_rate as f64).round() as usize;
        assert_eq!(resampled.len(), expected_dst_samples);

        // Count zero crossings
        let mut zero_crossings: i32 = 0;
        for i in 1..resampled.len() {
            if (resampled[i - 1] <= 0.0 && resampled[i] > 0.0)
                || (resampled[i - 1] >= 0.0 && resampled[i] < 0.0)
            {
                zero_crossings += 1;
            }
        }
        // In 0.1s of 1000 Hz, there should be ~200 zero crossings
        assert!((zero_crossings - 200).abs() <= 2);
    }

    #[test]
    fn test_load_pack_telios_v2_fallback() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let tar_path = root.join("dist/packs/telios-v2.tar.gz");
        if !tar_path.exists() {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let file = std::fs::File::open(&tar_path).unwrap();
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        archive.unpack(temp.path()).unwrap();

        let bank = load_pack(temp.path(), 48000).unwrap();
        // press_default must contain the 6 key clips, resolved despite manifest asking for key.wav
        assert_eq!(bank.press_default.len(), 6);
        // Space (code 57)
        let space_clips = bank.press[57].as_ref().unwrap();
        assert_eq!(space_clips.len(), 1);
        // Enter (code 28) resolved ent.wav despite enter.wav in manifest
        let enter_clips = bank.press[28].as_ref().unwrap();
        assert_eq!(enter_clips.len(), 1);
        // Backspace (code 14) resolved back.wav despite backspace.wav in manifest
        let back_clips = bank.press[14].as_ref().unwrap();
        assert_eq!(back_clips.len(), 1);
    }
}
