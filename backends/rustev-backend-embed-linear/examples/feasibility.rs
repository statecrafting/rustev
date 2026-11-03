//! Offline encoder probe, not a backend or a platform qualification claim.
//! Run after stages 1 and 2 pass. Arguments: model dir, absolute runtime path,
//! stage-2 reference JSON. Use an optimized build and qualification/measure.py
//! to capture child process peak RSS. JSON goes to stdout; no download is used.
use std::{error::Error, fs, path::Path, time::Instant};

use ort::{
    session::{
        Session,
        builder::{AutoDevicePolicy, GraphOptimizationLevel},
    },
    value::Tensor,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokenizers::pipeline::{EncodeOptions, Override, PipelineTokenizer};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[cfg(target_os = "macos")]
fn runtime_version(path: &str) -> Result<String> {
    use std::ffi::{CStr, CString, c_char, c_void};
    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> i32;
    }
    let path = CString::new(path)?;
    // The absolute library path was hashed before this call. Only the stable
    // ONNX Runtime C API entry point is read, while the handle is alive.
    unsafe {
        let handle = dlopen(path.as_ptr(), 2);
        if handle.is_null() {
            return Err("dlopen failed".into());
        }
        let symbol = dlsym(handle, c"OrtGetApiBase".as_ptr());
        if symbol.is_null() {
            dlclose(handle);
            return Err("OrtGetApiBase missing".into());
        }
        let get_base: unsafe extern "system" fn() -> *const ort::sys::OrtApiBase =
            std::mem::transmute(symbol);
        let base = get_base();
        if base.is_null() {
            dlclose(handle);
            return Err("null OrtApiBase".into());
        }
        let version = CStr::from_ptr(((*base).GetVersionString)())
            .to_str()?
            .to_owned();
        dlclose(handle);
        Ok(version)
    }
}

#[cfg(not(target_os = "macos"))]
fn runtime_version(_: &str) -> Result<String> {
    Err("this probe's runtime-version reader is macOS only".into())
}

fn tokens(tokenizer: &PipelineTokenizer, text: &str) -> Result<Vec<u32>> {
    let options = EncodeOptions {
        padding: Override::Off,
        truncation: Override::Off,
        ..EncodeOptions::default()
    };
    let encodings = tokenizer.encode(text, &options).wait()?;
    let encoding = encodings.first().ok_or("no encoding")?;
    Ok(encoding.ids().iter().map(|token| token.id()).collect())
}

fn embedding(session: &mut Session, ids: &[u32]) -> Result<Vec<f32>> {
    let shape = [1, ids.len()];
    let ids = Tensor::from_array((shape, ids.iter().map(|&v| i64::from(v)).collect::<Vec<_>>()))?;
    let mask = Tensor::from_array((shape, vec![1_i64; shape[1]]))?;
    let types = Tensor::from_array((shape, vec![0_i64; shape[1]]))?;
    let outputs = session.run(ort::inputs![
        "input_ids" => ids, "attention_mask" => mask, "token_type_ids" => types
    ])?;
    let (dimensions, values) = outputs[0].try_extract_tensor::<f32>()?;
    if dimensions.as_ref() != [1, shape[1] as i64, 384] {
        return Err("unexpected graph output shape".into());
    }
    let mut pooled = vec![0.0_f32; 384];
    for token in values.as_chunks::<384>().0 {
        for (sum, value) in pooled.iter_mut().zip(token) {
            *sum += value;
        }
    }
    for value in &mut pooled {
        *value /= shape[1] as f32;
    }
    let norm = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err("non_finite".into());
    }
    for value in &mut pooled {
        *value /= norm;
        if !value.is_finite() {
            return Err("non_finite".into());
        }
    }
    Ok(pooled)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 || !Path::new(&args[1]).is_absolute() || !Path::new(&args[2]).is_absolute() {
        return Err("usage: feasibility ABS_MODEL_DIR ABS_RUNTIME_PATH REFERENCE_JSON".into());
    }
    let reference: Value = serde_json::from_slice(&fs::read(&args[3])?)?;
    if reference["verdict"] != "pass" {
        return Err("stage 2 did not pass".into());
    }
    let start = Instant::now();
    let graph = fs::read(Path::new(&args[1]).join("onnx/model.onnx"))?;
    let graph_sha = format!("{:x}", Sha256::digest(&graph));
    if reference["graph_sha256"] != graph_sha {
        return Err("graph digest differs from stage 2".into());
    }
    let runtime_sha = format!("{:x}", Sha256::digest(fs::read(&args[2])?));
    if runtime_sha != "2a50a63e7f41f8d4d551d3ee47248df3864f6540109fce8ad520ca3b51af4b70" {
        return Err("runtime_digest_mismatch".into());
    }
    let tokenizer_bytes = fs::read_to_string(Path::new(&args[1]).join("tokenizer.json"))?;
    let tokenizer_sha = format!("{:x}", Sha256::digest(tokenizer_bytes.as_bytes()));
    if reference["tokenizer_sha256"] != tokenizer_sha {
        return Err("tokenizer digest differs from stage 2".into());
    }
    // The pinned crate provides this legacy-format reader upgrade. Keep the
    // source artifact untouched and verify every fixture's token ids below.
    let canonical_tokenizer = tokenizers::canonicalize_str(&tokenizer_bytes)?;
    let canonical_tokenizer_sha = format!("{:x}", Sha256::digest(canonical_tokenizer.as_bytes()));
    let tokenizer = tokenizers::from_json(&canonical_tokenizer)?;
    let version = runtime_version(&args[2])?;
    if version != "1.31.0" {
        return Err("runtime_version_mismatch".into());
    }
    let environment = ort::init_from(&args[2])?.with_telemetry(false).build()?;
    let mut session = Session::builder(&environment)?
        .with_auto_device(AutoDevicePolicy::PreferCPU)
        .map_err(|e| e.to_string())?
        .with_intra_threads(1)
        .map_err(|e| e.to_string())?
        .with_inter_threads(1)
        .map_err(|e| e.to_string())?
        .with_parallel_execution(false)
        .map_err(|e| e.to_string())?
        .with_no_environment_execution_providers()
        .map_err(|e| e.to_string())?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        .commit_from_memory(&graph)?;
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0;
    let fixtures = reference["fixtures"].as_array().ok_or("missing fixtures")?;
    let mut comparisons = Vec::new();
    let mut longest = None;
    for fixture in fixtures {
        let text = fixture["text"].as_str().ok_or("missing text")?;
        let ids = tokens(&tokenizer, text)?;
        if json!(&ids) != fixture["token_ids"] {
            return Err("token ids differ from reference".into());
        }
        if text.trim().is_empty() || ids.len() > 256 {
            continue;
        }
        let first = embedding(&mut session, &ids)?;
        let second = embedding(&mut session, &ids)?;
        let expected = fixture["reference_embedding"]
            .as_array()
            .ok_or("missing embedding")?;
        let mut max_abs = 0.0_f64;
        let mut agrees = expected.len() == first.len();
        let mut dot = 0.0_f64;
        let mut aa = 0.0_f64;
        let mut bb = 0.0_f64;
        for (&a, b) in first.iter().zip(expected) {
            let a = f64::from(a);
            let b = b.as_f64().ok_or("invalid reference value")?;
            let difference = (a - b).abs();
            max_abs = max_abs.max(difference);
            agrees &= difference <= 0.00001 || difference <= 0.0001 * a.abs().max(b.abs());
            dot += a * b;
            aa += a * a;
            bb += b * b;
        }
        comparisons.push(
            json!({"text":text, "max_abs_difference":max_abs.to_string(),
            "cosine":(dot / (aa * bb).sqrt()).to_string(), "agrees":agrees,
            "bitwise_repeatable":first.iter().zip(&second).all(|(a,b)|a.to_bits()==b.to_bits())}),
        );
        if ids.len() == 256 {
            longest = Some(ids.to_vec());
        }
    }
    let ids = longest.ok_or("no max_tokens fixture")?;
    for _ in 0..50 {
        std::hint::black_box(embedding(&mut session, &ids)?);
    }
    let mut timings = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let start = Instant::now();
        let ids = tokens(&tokenizer, &"hello ".repeat(254))?;
        std::hint::black_box(embedding(&mut session, &ids)?);
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let raw_ms: Vec<_> = timings.iter().map(ToString::to_string).collect();
    timings.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "cold_setup_ms":cold_ms.to_string(), "p95_ms":timings[949].to_string(),
            "p99_ms":timings[989].to_string(), "requests":1000,"warm_up":50,
            "warm_request_ms":raw_ms,"graph_sha256":graph_sha,"runtime_sha256":runtime_sha,
            "runtime_build_info":ort::info(), "comparisons":comparisons,
            "runtime_reported_version":version, "tokenizer_source_sha256":tokenizer_sha,
            "provider":"cpu", "auto_device":"PreferCPU", "intra_op_threads":1,
            "inter_op_threads":1, "execution":"sequential", "graph_optimization":"all",
            "tokenizer_in_memory_canonical_sha256":canonical_tokenizer_sha,
            "tokenizer_reader":"tokenizers::canonicalize_str then from_json; source unchanged",
            "scope":"encoder probe only; no heads, backend setup, plans or Linux comparison"
        }))?
    );
    Ok(())
}
