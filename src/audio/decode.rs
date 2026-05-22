//! Decode an audio file (mp3/ogg/wav/flac/m4a/aac/…) into the f32 mono
//! 16 kHz PCM stream whisper.cpp expects.
//!
//! `symphonia` does container/codec detection from the magic bytes; we
//! pass a hint based on the file extension as a tie-breaker but never
//! trust it (PCM wrapped in the wrong container is more common than
//! one might expect). The decoded sample stream is converted to f32,
//! down-mixed to mono if multi-channel, then resampled to 16 kHz via
//! `rubato`'s SincFixedIn (high-quality polyphase filter — good enough
//! for speech, fast enough that decoding stays I/O bound).

use anyhow::{anyhow, Context, Result};
use std::io::Cursor;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

/// whisper.cpp requires exactly this. Bypassing it (e.g. running at 8
/// kHz) silently degrades quality; running at higher rates wastes CPU.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Decoded PCM stream ready for whisper.cpp.
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub duration_ms: u64,
}

/// Top-level decode entry point used by `transcribe::transcribe_audio`.
/// `ext` is the lower-cased file extension (no dot) — used as a hint
/// for the probe, not as a trust anchor.
pub fn decode_to_pcm_16khz_mono(bytes: &[u8], ext: &str) -> Result<DecodedAudio> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());

    let mut hint = Hint::new();
    if !ext.is_empty() {
        hint.with_extension(ext);
    }

    let probe = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .context("symphonia probe failed (unsupported or corrupt container)")?;

    let mut format = probe.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("no default audio track"))?;
    let track_id = track.id;

    let codec_params = track.codec_params.clone();
    let source_rate = codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("audio track has no declared sample rate"))?;
    let source_channels = codec_params
        .channels
        .ok_or_else(|| anyhow!("audio track has no declared channel layout"))?
        .count();

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .context("symphonia codec init failed")?;

    // Accumulate mono f32 at the source sample rate. We resample once
    // at the end rather than per-packet because rubato's polyphase
    // filter is materially more accurate on a single contiguous run.
    let mut mono: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                // Stream reset (chained ogg, mid-stream sample-rate
                // change). For Phase 1 we bail — supporting it would
                // require re-probing and re-emitting a fresh resampler.
                return Err(anyhow!("audio stream reset mid-decode (chained streams not supported)"));
            }
            Err(e) => return Err(anyhow::Error::new(e).context("packet read failed")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => continue, // skip a bad frame
            Err(e) => return Err(anyhow::Error::new(e).context("frame decode failed")),
        };
        append_mono_f32(&decoded, source_channels, &mut mono);
    }

    if mono.is_empty() {
        return Err(anyhow!("decoded audio is empty"));
    }

    let samples = if source_rate == WHISPER_SAMPLE_RATE {
        mono
    } else {
        resample(&mono, source_rate, WHISPER_SAMPLE_RATE)?
    };
    let duration_ms = (samples.len() as u64 * 1000) / WHISPER_SAMPLE_RATE as u64;

    Ok(DecodedAudio {
        samples,
        sample_rate: WHISPER_SAMPLE_RATE,
        duration_ms,
    })
}

/// Take a `symphonia` `AudioBufferRef`, down-mix to mono, convert to
/// f32 in [-1.0, 1.0], and append to `out`. Handles the seven sample
/// formats symphonia surfaces (the union of every decoder we enable).
fn append_mono_f32(buf: &AudioBufferRef<'_>, channels: usize, out: &mut Vec<f32>) {
    macro_rules! mix {
        ($buf:expr, $to_f32:expr) => {{
            let frames = $buf.frames();
            out.reserve(frames);
            if channels == 1 {
                for i in 0..frames {
                    out.push($to_f32($buf.chan(0)[i]));
                }
            } else {
                let inv = 1.0 / channels as f32;
                for i in 0..frames {
                    let mut acc = 0.0;
                    for ch in 0..channels {
                        acc += $to_f32($buf.chan(ch)[i]);
                    }
                    out.push(acc * inv);
                }
            }
        }};
    }
    match buf {
        AudioBufferRef::U8(b)  => mix!(b, |s: u8|  (s as f32 - 128.0) / 128.0),
        AudioBufferRef::U16(b) => mix!(b, |s: u16| (s as f32 - 32768.0) / 32768.0),
        AudioBufferRef::U24(b) => mix!(b, |s: symphonia::core::sample::u24| (s.inner() as f32 - 8_388_608.0) / 8_388_608.0),
        AudioBufferRef::U32(b) => mix!(b, |s: u32| (s as f32 - 2_147_483_648.0) / 2_147_483_648.0),
        AudioBufferRef::S8(b)  => mix!(b, |s: i8|  s as f32 / 128.0),
        AudioBufferRef::S16(b) => mix!(b, |s: i16| s as f32 / 32768.0),
        AudioBufferRef::S24(b) => mix!(b, |s: symphonia::core::sample::i24| s.inner() as f32 / 8_388_608.0),
        AudioBufferRef::S32(b) => mix!(b, |s: i32| s as f32 / 2_147_483_648.0),
        AudioBufferRef::F32(b) => mix!(b, |s: f32| s),
        AudioBufferRef::F64(b) => mix!(b, |s: f64| s as f32),
    }
}

/// Polyphase resampler — rubato `SincFixedIn` with 64-tap window.
/// One-shot: we pre-have the entire input mono stream, so a single
/// `process` call (with the last chunk padded) is sufficient.
fn resample(input: &[f32], from_hz: u32, to_hz: u32) -> Result<Vec<f32>> {
    let ratio = to_hz as f64 / from_hz as f64;
    let params = SincInterpolationParameters {
        sinc_len: 64,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let chunk_size = 1024usize;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 1.0, params, chunk_size, 1)
        .context("rubato resampler init failed")?;

    let mut out = Vec::with_capacity((input.len() as f64 * ratio) as usize + chunk_size);
    let mut pos = 0usize;
    while pos + chunk_size <= input.len() {
        let frame = &[&input[pos..pos + chunk_size]];
        let processed = resampler
            .process(frame, None)
            .map_err(|e| anyhow!("rubato process: {e}"))?;
        out.extend_from_slice(&processed[0]);
        pos += chunk_size;
    }
    // Tail: zero-pad the last partial chunk.
    if pos < input.len() {
        let mut tail = vec![0.0f32; chunk_size];
        let rem = input.len() - pos;
        tail[..rem].copy_from_slice(&input[pos..]);
        let frame = &[&tail[..]];
        let processed = resampler
            .process(frame, None)
            .map_err(|e| anyhow!("rubato tail process: {e}"))?;
        let valid = (rem as f64 * ratio) as usize;
        out.extend_from_slice(&processed[0][..valid.min(processed[0].len())]);
    }
    Ok(out)
}
