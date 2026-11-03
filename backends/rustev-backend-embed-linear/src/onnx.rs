//! The ONNX encoder (spec 011, 3.6.4 and 3.9.1).
//!
//! One session over the bundle's verified graph bytes, CPU execution
//! provider only, with the manifest's optimization level and thread counts;
//! no profiling, custom operator library or execution-provider plugin. The
//! session is shared by the workers (Decision 14); `ort` runs need exclusive
//! access, so runs on it are serialized.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::task::{Context, Wake, Waker};

use ort::environment::Environment;
use ort::session::builder::{AutoDevicePolicy, GraphOptimizationLevel};
use ort::session::{RunOptions, Session, SessionInputValue};
use ort::value::{Tensor, TensorElementType, ValueType};
use rustev_core::seams::CancelSignal;

use crate::manifest::{EmbedManifest, GraphOptimization};
use crate::{EmbedSetupError, SetupCode};

const INPUT_IDS: &str = "input_ids";
const ATTENTION_MASK: &str = "attention_mask";
const TOKEN_TYPE_IDS: &str = "token_type_ids";

/// A session over a verified graph whose signature matches the manifest.
pub struct OnnxEncoder {
    session: Mutex<Session>,
    inputs: Vec<String>,
    output: String,
    hidden: usize,
}

impl OnnxEncoder {
    /// Create the session from the verified graph bytes and check its inputs
    /// and output against the manifest (3.7.1): `graph_rejected` when the
    /// runtime cannot create it, `graph_signature_mismatch` when a name,
    /// element type or the hidden size differs.
    pub fn new(
        environment: &Environment,
        graph: &[u8],
        manifest: &EmbedManifest,
    ) -> Result<Self, EmbedSetupError> {
        let rejected =
            |e: &dyn std::fmt::Display| refusal(SetupCode::GraphRejected, &e.to_string());
        let r = &manifest.runtime;
        let level = match r.graph_optimization {
            GraphOptimization::Disabled => GraphOptimizationLevel::Disable,
            GraphOptimization::Basic => GraphOptimizationLevel::Level1,
            GraphOptimization::Extended => GraphOptimizationLevel::Level2,
            GraphOptimization::All => GraphOptimizationLevel::All,
        };
        let session = Session::builder(environment)
            .map_err(|e| rejected(&e))?
            .with_no_environment_execution_providers()
            .map_err(|e| rejected(&e))?
            .with_auto_device(AutoDevicePolicy::PreferCPU)
            .map_err(|e| rejected(&e))?
            .with_intra_threads(r.intra_op_threads as usize)
            .map_err(|e| rejected(&e))?
            .with_inter_threads(r.inter_op_threads as usize)
            .map_err(|e| rejected(&e))?
            .with_parallel_execution(r.inter_op_threads > 1)
            .map_err(|e| rejected(&e))?
            .with_optimization_level(level)
            .map_err(|e| rejected(&e))?
            .commit_from_memory(graph)
            .map_err(|e| rejected(&e))?;
        let e = &manifest.encoder;
        let hidden = e.hidden_size as usize;
        check_signature(&session, &e.inputs, &e.output, hidden)?;
        Ok(Self {
            session: Mutex::new(session),
            inputs: e.inputs.clone(),
            output: e.output.clone(),
            hidden,
        })
    }

    /// One run over one unpadded sequence (3.9.1): `input_ids`,
    /// `attention_mask` and, when the graph declares it, `token_type_ids` of
    /// zeros. Returns the last hidden state, `ids.len() * hidden_size` f32
    /// values in token order. When `cancel` is raised during the run, its
    /// termination is requested through the run options (3.12.2) and the run
    /// fails.
    pub fn encode(
        &self,
        ids: &[u32],
        mask: &[u32],
        cancel: &CancelSignal,
    ) -> Result<Vec<f32>, EncodeError> {
        let n = ids.len();
        if n == 0 || mask.len() != n {
            return Err(EncodeError("input"));
        }
        let shape = [1, n];
        let mut values: Vec<(Cow<'_, str>, SessionInputValue<'_>)> = Vec::new();
        for name in &self.inputs {
            let data: Vec<i64> = match name.as_str() {
                INPUT_IDS => ids.iter().map(|&v| i64::from(v)).collect(),
                ATTENTION_MASK => mask.iter().map(|&v| i64::from(v)).collect(),
                TOKEN_TYPE_IDS => vec![0; n],
                _ => return Err(EncodeError("input")),
            };
            let tensor = Tensor::from_array((shape, data)).map_err(|_| EncodeError("runtime"))?;
            values.push((Cow::Owned(name.clone()), tensor.into()));
        }
        let options = Arc::new(RunOptions::new().map_err(|_| EncodeError("runtime"))?);
        let terminate = Waker::from(Arc::new(Terminate(options.clone())));
        if cancel
            .poll_raised(&mut Context::from_waker(&terminate))
            .is_ready()
        {
            return Err(EncodeError("cancelled"));
        }
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if cancel.is_raised() {
            return Err(EncodeError("cancelled"));
        }
        let outputs = session
            .run_with_options(values, &*options)
            .map_err(|_| EncodeError("runtime"))?;
        let (dims, data) = outputs
            .get(self.output.as_str())
            .ok_or(EncodeError("output"))?
            .try_extract_tensor::<f32>()
            .map_err(|_| EncodeError("runtime"))?;
        if dims.as_ref() != [1, n as i64, self.hidden as i64] || data.len() != n * self.hidden {
            return Err(EncodeError("output"));
        }
        Ok(data.to_vec())
    }
}

/// Why a run produced no hidden state: `input`, `cancelled`, `runtime` or
/// `output`. The backend reports each as `runtime_error`, or as cancelled
/// when its signal was raised (3.7.3, 3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeError(pub &'static str);

/// Requests termination of the run whose options it holds when the
/// attempt's signal is raised.
struct Terminate(Arc<RunOptions>);
impl Wake for Terminate {
    fn wake(self: Arc<Self>) {
        let _ = self.0.terminate();
    }
}

/// The manifest's inputs are exactly the graph's, each an int64 tensor of
/// the supported names; the output is a float tensor of rank 3 whose last
/// dimension is the hidden size.
fn check_signature(
    session: &Session,
    inputs: &[String],
    output: &str,
    hidden: usize,
) -> Result<(), EmbedSetupError> {
    let mismatch = |detail: &str| refusal(SetupCode::GraphSignatureMismatch, detail);
    let mut graph: Vec<&str> = session.inputs().iter().map(|i| i.name()).collect();
    let mut declared: Vec<&str> = inputs.iter().map(String::as_str).collect();
    graph.sort_unstable();
    declared.sort_unstable();
    if graph != declared {
        return Err(mismatch("the graph's inputs differ from encoder.inputs"));
    }
    for name in inputs {
        if !matches!(name.as_str(), INPUT_IDS | ATTENTION_MASK | TOKEN_TYPE_IDS) {
            return Err(mismatch(
                "an input is not input_ids, attention_mask or token_type_ids",
            ));
        }
    }
    if !inputs.iter().any(|i| i == INPUT_IDS) || !inputs.iter().any(|i| i == ATTENTION_MASK) {
        return Err(mismatch("input_ids and attention_mask are required"));
    }
    for input in session.inputs() {
        if !matches!(
            input.dtype(),
            ValueType::Tensor {
                ty: TensorElementType::Int64,
                ..
            }
        ) {
            return Err(mismatch("an input is not an int64 tensor"));
        }
    }
    let out = session
        .outputs()
        .iter()
        .find(|o| o.name() == output)
        .ok_or_else(|| mismatch("the graph has no encoder.output"))?;
    match out.dtype() {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape,
            ..
        } if shape.len() == 3 && shape[2] == hidden as i64 => Ok(()),
        _ => Err(mismatch(
            "encoder.output is not a float tensor [batch, tokens, hidden_size]",
        )),
    }
}

fn refusal(code: SetupCode, detail: &str) -> EmbedSetupError {
    EmbedSetupError {
        code,
        item: "encoder.graph".into(),
        detail: detail.into(),
    }
}
