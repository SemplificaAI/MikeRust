//! Embedding-pipeline diagnostic: load each cached model variant
//! and measure the time taken by every step of the pipeline.
//!
//! The user reported indefinite hang on chat after the
//! `[rag] building ONNX session` log. Both the FP32 intfloat model
//! and the INT8-dynamic Xenova model exhibited the same symptom on
//! a Snapdragon X Elite ARM64 setup after swapping in a clean base
//! CPU onnxruntime.dll. The chat-handler pathway doesn't surface
//! WHERE exactly it hangs because the stall is inside the
//! `spawn_blocking` that wraps fastembed's `try_new_from_user_defined`.
//!
//! This test bypasses the chat handler and exercises the same
//! fastembed call directly, with `eprintln!`-style timing at every
//! sub-step. The output identifies which model variant works on
//! this machine and how long each phase takes — concrete data to
//! replace guesswork.
//!
//! How to run:
//!   cargo test --test embedding_perf -- --ignored --nocapture
//!
//! Ignored by default because each variant takes 5-30 s and loads
//! the full 1.1 GB / 275 MB ONNX into RAM — too heavy for routine
//! CI. Run on-demand when diagnosing embedding-pipeline regressions.

#![cfg(feature = "rag")]

use std::path::PathBuf;
use std::time::Instant;

use fastembed::{
    InitOptionsUserDefined, Pooling, QuantizationMode, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};

/// Resolve a cached model directory under `<USERPROFILE>/mikerust-data/
/// fastembed/<subdir>/`. Skips the test (via `Result<…, &'static str>`)
/// when the directory is missing so the test is a no-op on machines
/// that haven't downloaded the model yet.
fn cache_dir(subdir: &str) -> Result<PathBuf, &'static str> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "no USERPROFILE or HOME env var")?;
    let p = PathBuf::from(home)
        .join("mikerust-data")
        .join("fastembed")
        .join(subdir);
    if !p.is_dir() {
        return Err("cache dir not present — run the app once to download the model");
    }
    Ok(p)
}

struct ModelFiles {
    onnx: Vec<u8>,
    tokenizer: Vec<u8>,
    config: Vec<u8>,
    special_tokens_map: Vec<u8>,
    tokenizer_config: Vec<u8>,
}

fn load_files(dir: &PathBuf, onnx_filename: &str) -> std::io::Result<ModelFiles> {
    Ok(ModelFiles {
        onnx: std::fs::read(dir.join(onnx_filename))?,
        tokenizer: std::fs::read(dir.join("tokenizer.json"))?,
        config: std::fs::read(dir.join("config.json"))?,
        special_tokens_map: std::fs::read(dir.join("special_tokens_map.json"))?,
        tokenizer_config: std::fs::read(dir.join("tokenizer_config.json"))?,
    })
}

/// Resolve and export `ORT_DYLIB_PATH` so fastembed/ort can find
/// the vendored runtime DLL even when run from cargo test (which
/// doesn't go through `lib::run_server_with_channels`, where the
/// service ordinarily sets this).
fn ensure_ort_dylib() {
    if std::env::var("ORT_DYLIB_PATH").is_ok() {
        eprintln!("[ort] ORT_DYLIB_PATH already set");
        return;
    }
    // Walk ancestors of the workspace looking for libs/onnxruntime/<sub>/<file>.
    let (sub, file) = if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        ("win-arm64", "onnxruntime.dll")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        ("win-x64", "onnxruntime.dll")
    } else if cfg!(target_os = "macos") {
        ("macos-arm64", "libonnxruntime.dylib")
    } else {
        ("linux-x64", "libonnxruntime.so")
    };
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest_dir
        .join("libs")
        .join("onnxruntime")
        .join(sub)
        .join(file);
    if candidate.is_file() {
        let bytes = std::fs::metadata(&candidate)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0);
        eprintln!("[ort] setting ORT_DYLIB_PATH={} ({} MB)", candidate.display(), bytes);
        // SAFETY: single-threaded test setup before any ort call.
        unsafe {
            std::env::set_var("ORT_DYLIB_PATH", &candidate);
        }
    } else {
        eprintln!("[ort] WARNING: {} not found — ort will fail to load", candidate.display());
    }
}

/// Heart of the diagnostic: load files, build the fastembed model,
/// time every step. Prints to stderr (visible with `--nocapture`).
/// Returns the embed query time so it can be compared across variants.
fn time_pipeline(label: &str, dir: &PathBuf, onnx_filename: &str) {
    eprintln!("\n===== {label} =====");
    ensure_ort_dylib();
    eprintln!("[t=0] cache dir: {}", dir.display());

    let t_io = Instant::now();
    let files = match load_files(dir, onnx_filename) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[SKIP] cannot read files: {e}");
            return;
        }
    };
    eprintln!(
        "[t={:>6} ms] file read complete (onnx={} MB, tokenizer={} KB, config={} B)",
        t_io.elapsed().as_millis(),
        files.onnx.len() / (1024 * 1024),
        files.tokenizer.len() / 1024,
        files.config.len(),
    );

    let t_struct = Instant::now();
    let model = UserDefinedEmbeddingModel {
        onnx_file: files.onnx,
        external_initializers: vec![],
        tokenizer_files: TokenizerFiles {
            tokenizer_file: files.tokenizer,
            config_file: files.config,
            special_tokens_map_file: files.special_tokens_map,
            tokenizer_config_file: files.tokenizer_config,
        },
        pooling: Some(Pooling::Mean),
        quantization: QuantizationMode::None,
        output_key: None,
    };
    let opts = InitOptionsUserDefined::new()
        .with_max_length(512)
        .with_execution_providers(vec![]);
    eprintln!(
        "[t={:>6} ms] UserDefinedEmbeddingModel + InitOptions assembled (now calling try_new)",
        t_struct.elapsed().as_millis(),
    );

    let t_session = Instant::now();
    let mut embedding = match TextEmbedding::try_new_from_user_defined(model, opts) {
        Ok(e) => {
            eprintln!(
                "[t={:>6} ms] try_new_from_user_defined: OK (session built)",
                t_session.elapsed().as_millis(),
            );
            e
        }
        Err(err) => {
            eprintln!(
                "[t={:>6} ms] try_new_from_user_defined: FAILED — {err}",
                t_session.elapsed().as_millis(),
            );
            return;
        }
    };

    // ── First-shot embed (single query, what the chat does on every turn).
    let t_q1 = Instant::now();
    let q1 = embedding.embed(vec!["query: polizza Allianz".to_string()], Some(1));
    match q1 {
        Ok(v) => {
            assert_eq!(v.len(), 1, "expected one vector");
            assert_eq!(v[0].len(), 768, "e5-base produces 768-d vectors");
            eprintln!(
                "[t={:>6} ms] first embed_query (cold ort path): OK, {} dims",
                t_q1.elapsed().as_millis(),
                v[0].len(),
            );
        }
        Err(e) => {
            eprintln!(
                "[t={:>6} ms] first embed_query: FAILED — {e}",
                t_q1.elapsed().as_millis(),
            );
            return;
        }
    }

    // ── Second-shot embed — measures warm/cached cost.
    let t_q2 = Instant::now();
    let _ = embedding.embed(vec!["query: contraente A.TEC. S.r.l.".to_string()], Some(1));
    eprintln!(
        "[t={:>6} ms] second embed_query (warm): OK",
        t_q2.elapsed().as_millis(),
    );

    // ── Batch embed of 16 passages — what indexing pays per chunk.
    let t_batch = Instant::now();
    let passages: Vec<String> = (0..16)
        .map(|i| format!("passage: clausola n. {i} della polizza Allianz applicazioni"))
        .collect();
    let _ = embedding.embed(passages, Some(16));
    eprintln!(
        "[t={:>6} ms] batch embed 16 passages: OK ({} ms per passage avg)",
        t_batch.elapsed().as_millis(),
        t_batch.elapsed().as_millis() / 16,
    );
}

#[test]
#[ignore = "loads heavy models; run on-demand with --ignored --nocapture"]
fn perf_fp32_intfloat() {
    let dir = match cache_dir("mike-e5-base") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[SKIP FP32] {e}");
            return;
        }
    };
    time_pipeline("FP32 (intfloat/multilingual-e5-base, model.onnx, ~1.1 GB)", &dir, "model.onnx");
}

#[test]
#[ignore = "loads heavy models; run on-demand with --ignored --nocapture"]
fn perf_int8_xenova() {
    let dir = match cache_dir("mike-e5-base-quantized") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[SKIP INT8] {e}");
            return;
        }
    };
    time_pipeline(
        "INT8 dynamic (Xenova/multilingual-e5-base, model_quantized.onnx, ~275 MB)",
        &dir,
        "model_quantized.onnx",
    );
}
