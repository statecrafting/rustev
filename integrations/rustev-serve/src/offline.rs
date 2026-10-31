//! Bounded offline surface calls. Configured roots are read-only host inputs.
use crate::{Reply, Service};
use rustev_contract::{Document, serve::*};
use rustev_eval::{
    replay::ReplayConfig,
    resolve::{Resolution, Resolver},
};
use rustev_surface::host::{Host, IoFail, Memory, ReadFail};
use std::{collections::BTreeMap, io::Read, path::PathBuf, sync::Arc, time::Duration};

pub struct Roots {
    pub roots: BTreeMap<String, PathBuf>,
}
impl Resolver for Roots {
    fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution {
        let Ok(reference) = serde_json::from_value::<Reference>(serde_json::json!(reference))
        else {
            return Resolution::Inaccessible;
        };
        // A reference that parsed always has a prefix; a malformed one is
        // inaccessible rather than a panic on the blocking thread.
        let Some((prefix, relative)) = reference.as_str().split_once(':') else {
            return Resolution::Inaccessible;
        };
        let Some(root) = self.roots.get(prefix) else {
            return Resolution::Inaccessible;
        };
        let read = || -> std::io::Result<Vec<u8>> {
            let root = root.canonicalize()?;
            let path = root.join(relative).canonicalize()?;
            if !path.starts_with(&root) {
                return Err(std::io::Error::other("outside root"));
            }
            let file = std::fs::File::open(&path)?;
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("not a regular file"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let actual = file.metadata()?;
                let expected = std::fs::metadata(&path)?;
                if actual.dev() != expected.dev()
                    || actual.ino() != expected.ino()
                    || !path.canonicalize()?.starts_with(&root)
                {
                    return Err(std::io::Error::other("path changed"));
                }
            }
            let mut bytes = vec![];
            file.take(max_bytes as u64).read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        read().map_or(Resolution::Inaccessible, Resolution::Bytes)
    }
}
struct SurfaceHost {
    roots: Roots,
    memory: Memory,
}
impl Resolver for SurfaceHost {
    fn resolve(&self, reference: &str, max: usize) -> Resolution {
        if let Some(bytes) = self.memory.inputs().get(reference) {
            Resolution::Bytes(bytes[..bytes.len().min(max)].to_vec())
        } else {
            self.roots.resolve(reference, max)
        }
    }
}
impl Host for SurfaceHost {
    fn read_classified(&self, label: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        match self.resolve(label, max) {
            Resolution::Bytes(bytes) => Ok(Some(bytes)),
            Resolution::Missing => Ok(None),
            _ => Err(ReadFail::File(IoFail {
                path: label.into(),
                detail: "inaccessible".into(),
            })),
        }
    }
    fn ensure_absent(&self, label: &str) -> Result<(), IoFail> {
        self.memory.ensure_absent(label)
    }
    fn create(&self, label: &str, bytes: &[u8]) -> Result<(), IoFail> {
        self.memory.create(label, bytes)
    }
    fn write_all(&self, files: &[(String, Vec<u8>)]) -> Result<(), IoFail> {
        self.memory.write_all(files)
    }
    fn join(&self, dir: &str, name: &str) -> String {
        format!("{dir}/{name}")
    }
}
fn config(
    options: ReplayOptions,
    scope: rustev_contract::scope::Scope,
) -> Result<ReplayConfig, RefusalCode> {
    if options.scope != scope {
        return Err(RefusalCode::DecisionNotFound);
    }
    Ok(ReplayConfig {
        scope,
        now_ms: options.now_ms,
        host_cap_ms: match options.host_cap_ms {
            Maybe::None => None,
            Maybe::Some(n) => Some(n.get()),
        },
    })
}
fn bytes<T: Document>(input: DocumentInput<T>, roots: &Roots) -> Result<Vec<u8>, RefusalCode> {
    match input {
        DocumentInput::Document(doc) => Ok(doc.bytes().to_vec()),
        DocumentInput::Reference(r) => match roots.resolve(r.as_str(), T::LIMITS.max_bytes + 1) {
            Resolution::Bytes(b) => Ok(b),
            _ => Err(RefusalCode::DecisionNotFound),
        },
    }
}
/// Run replay, evaluation or explanation on the bounded blocking pool
/// (3.8.4). `redaction` is the policy the request's listener names (3.8.3);
/// only explanation reads it.
pub async fn run(
    service: Arc<Service>,
    operation: Operation,
    principal: Vec<u8>,
    body: Vec<u8>,
    deadline: Option<u64>,
    redaction: Option<Vec<u8>>,
) -> Reply {
    if service.is_stopping() {
        return Reply::refusal(RefusalCode::ShuttingDown);
    }
    let Some(scope) = service.replay_scope(&principal) else {
        return Reply::refusal(RefusalCode::DecisionNotFound);
    };
    let Ok(slot) = service.blocking_slots.clone().try_acquire_owned() else {
        return Reply::refusal(RefusalCode::Overloaded);
    };
    let limit = Duration::from_millis(deadline.unwrap_or(60000).min(600000));
    let started = tokio::time::Instant::now();
    let permit = match tokio::time::timeout(limit, service.blocking.clone().acquire_owned()).await {
        Ok(Ok(p)) => p,
        _ => return Reply::refusal(RefusalCode::QueueDeadline),
    };
    let roots = Roots {
        roots: service
            .config
            .resolver_roots
            .iter()
            .map(|(k, v)| (k.as_str().into(), PathBuf::from(v.as_str())))
            .collect(),
    };
    let work = tokio::task::spawn_blocking(move || {
        let (_permit, _slot) = (permit, slot);
        if operation == Operation::Explain {
            explain(&body, &roots, scope, redaction.as_deref())
        } else {
            execute(operation, body, roots, scope)
        }
        .unwrap_or_else(Reply::refusal)
    });
    match tokio::time::timeout(limit.saturating_sub(started.elapsed()), work).await {
        Ok(Ok(r)) => r,
        _ => Reply::refusal(RefusalCode::QueueDeadline),
    }
}
fn execute(
    operation: Operation,
    body: Vec<u8>,
    roots: Roots,
    scope: rustev_contract::scope::Scope,
) -> Result<Reply, RefusalCode> {
    let done = if operation == Operation::Replay {
        let request = ServeReplay::parse(&body).map_err(|_| RefusalCode::InvalidDocument)?;
        let config = config(request.config, scope)?;
        let mut inputs = BTreeMap::new();
        inputs.insert("bundle".into(), bytes(request.bundle, &roots)?);
        for item in request.inputs.as_slice() {
            let value = match item.input.clone() {
                ReplayInput::Plan(d) => bytes(d, &roots),
                ReplayInput::Snapshot(d) => bytes(d, &roots),
                ReplayInput::Run(d) => bytes(d, &roots),
                ReplayInput::Evidence(d) => bytes(d, &roots),
                ReplayInput::Judgment(d) => bytes(d, &roots),
                ReplayInput::BackendOutput(d) => bytes(d, &roots),
                ReplayInput::ExecutionPolicy(d) => bytes(d, &roots),
                ReplayInput::Calibration(d) => bytes(d, &roots),
            }?;
            if inputs
                .insert(item.reference.as_str().into(), value)
                .is_some()
            {
                return Err(RefusalCode::InvalidDocument);
            }
        }
        let host = SurfaceHost {
            roots,
            memory: Memory::new(inputs),
        };
        rustev_surface::replay::replay("replay", &host, "bundle", &host, &config)
    } else {
        let request = ServeEvaluate::parse(&body).map_err(|_| RefusalCode::InvalidDocument)?;
        if !matches!(request.repeat, Maybe::None) || !request.deps.rules.as_slice().is_empty() {
            return Err(RefusalCode::EndpointUnavailable);
        }
        let config = config(request.replay, scope)?;
        let host = SurfaceHost {
            roots,
            memory: Memory::default(),
        };
        let descriptors: Vec<_> = request
            .deps
            .descriptors
            .as_slice()
            .iter()
            .map(|r| r.as_str().to_owned())
            .collect();
        let calibrations: Vec<_> = request
            .deps
            .calibrations
            .as_slice()
            .iter()
            .map(|r| r.as_str().to_owned())
            .collect();
        let args = rustev_surface::eval::EvalArgs {
            split: request.split,
            replay: &config,
            out: "result",
            dataset: request.dataset.as_str(),
            config: request.config.as_str(),
            adapter: request.adapter.as_str(),
            bundles: request.bundles.as_str(),
            candidate_plan: match &request.candidate_plan {
                Maybe::None => None,
                Maybe::Some(r) => Some(r.as_str()),
            },
            deps: rustev_surface::deps::DepArgs {
                descriptors: &descriptors,
                calibrations: &calibrations,
                rules: &[],
            },
        };
        rustev_surface::eval::eval("eval", &host, args, &host, &mut |_, _| {
            Err("rules backend unavailable".into())
        })
    };
    let done = done.unwrap_or_else(|d| d);
    Ok(Reply {
        status: 200,
        body: done.out.bytes(),
        content_type: "application/json",
    })
}

/// One side's evidence as the surface's named byte inputs: each member is
/// embedded bytes or a reference resolved under the configured roots.
fn side(
    evidence: &ExplainEvidence,
    name: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
    roots: &Roots,
) -> Result<serde_json::Value, RefusalCode> {
    let mut put = |member: &str, bytes: Vec<u8>| {
        let key = format!("{name}.{member}");
        files.insert(key.clone(), bytes);
        key
    };
    Ok(match evidence {
        ExplainEvidence::Record(r) => {
            let record = put("record", bytes(r.record.clone(), roots)?);
            match &r.plan {
                None => serde_json::json!({ "record": record }),
                Some(plan) => {
                    let plan = put("plan", bytes(plan.clone(), roots)?);
                    serde_json::json!({ "record": record, "plan": plan })
                }
            }
        }
        ExplainEvidence::Bundle(b) => {
            serde_json::json!({ "bundle": put("bundle", bytes(b.bundle.clone(), roots)?) })
        }
    })
}

/// A spec 040 refusal (3.17) as `invalid_document` naming its code.
fn explain_refusal(code: rustev_contract::explanation::RefusalCode) -> Reply {
    let mut reply = Reply::refusal(RefusalCode::InvalidDocument);
    if let Ok(mut refusal) = ServeRefusal::parse(&reply.body) {
        refusal.detail = Text::new(code.as_str()).expect("a refusal code is short text");
        reply = Reply::document(reply.status, &refusal);
    }
    reply
}

/// Explanation (3.8.3): one call of the library through `rustev-surface`,
/// whose explanation bytes are carried unchanged with their Markdown.
fn explain(
    body: &[u8],
    roots: &Roots,
    scope: rustev_contract::scope::Scope,
    redaction: Option<&[u8]>,
) -> Result<Reply, RefusalCode> {
    let request = ServeExplain::parse(body).map_err(|_| RefusalCode::InvalidDocument)?;
    let bundle = request.has_bundle();
    // `scope` and `now_ms` exactly when a side is a bundle (spec 040
    // 3.16.2); the scope must be the caller's before anything is resolved.
    let now_ms = match (bundle, &request.options.scope, request.options.now_ms) {
        (true, Some(given), Some(now_ms)) if *given == scope => Some(now_ms),
        (true, Some(_), Some(_)) => return Err(RefusalCode::DecisionNotFound),
        (false, None, None) => None,
        _ => return Err(RefusalCode::InvalidRequest),
    };
    let mut files = BTreeMap::new();
    let evidence = side(&request.evidence, "a", &mut files, roots)?;
    let query = match &request.query {
        ExplainQuery::Diff(d) => {
            serde_json::json!({ "diff": { "other": side(&d.diff.other, "b", &mut files, roots)? } })
        }
        ExplainQuery::Query(q) => {
            serde_json::to_value(q).map_err(|_| RefusalCode::InvalidDocument)?
        }
    };
    let mut options = serde_json::Map::new();
    if let Some(now_ms) = now_ms {
        let scope = serde_json::to_value(&scope).map_err(|_| RefusalCode::InternalError)?;
        options.insert("scope".into(), scope);
        options.insert("nowMs".into(), now_ms.as_ms().into());
    }
    if let Some(policy) = redaction {
        files.insert("redaction".into(), policy.to_vec());
        options.insert("redaction".into(), "redaction".into());
    }
    let args = serde_json::json!({ "evidence": evidence, "query": query, "options": options });
    // A bundle's external items resolve only through the host's resolver.
    let resolver: Option<&dyn Resolver> = if bundle { Some(roots) } else { None };
    match rustev_surface::explain::explain(&files, &args, resolver) {
        Ok(done) => {
            let answer = ServeExplanation {
                schema: ExplanationSchema::V1,
                explanation: Embedded::parse(&done.explanation)
                    .map_err(|_| RefusalCode::InternalError)?,
                markdown: Text::new(done.markdown).map_err(|_| RefusalCode::InternalError)?,
            };
            if answer.record_canonical().is_err() {
                return Ok(explain_refusal(
                    rustev_contract::explanation::RefusalCode::ExplanationTooLarge,
                ));
            }
            Ok(Reply::document(200, &answer))
        }
        Err(e) => Ok(explain_refusal(e.code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Sink;
    impl rustev_core::seams::EvidenceSink for Sink {
        fn deliver<'a>(
            &'a self,
            _: &'a rustev_contract::run::RunRecord,
        ) -> rustev_core::seams::BoxFuture<'a, Result<String, String>> {
            Box::pin(async { Ok("synthetic".into()) })
        }
    }
    #[tokio::test]
    async fn blocking_pool_full_and_queue_timeout_are_bounded_refusals() {
        let config = ServeConfig::parse(include_bytes!("../tests/fixtures/config.json")).unwrap();
        let service = Service::new(
            config,
            crate::Host {
                backends: BTreeMap::new(),
                contexts: BTreeMap::new(),
                sink: Arc::new(Sink),
                replay_scopes: BTreeMap::from([(
                    b"a".to_vec(),
                    rustev_contract::scope::Scope::tenant_only(
                        rustev_contract::scope::Handle::new("synthetic").unwrap(),
                        rustev_contract::scope::Handle::new("1").unwrap(),
                    ),
                )]),
            },
        )
        .unwrap();
        let worker = service.blocking.acquire().await.unwrap();
        let slots = service.blocking_slots.acquire_many(2).await.unwrap();
        let refusal = run(
            service.clone(),
            Operation::Replay,
            b"a".to_vec(),
            vec![],
            None,
            None,
        )
        .await;
        assert_eq!(
            ServeRefusal::parse(&refusal.body).unwrap().code,
            RefusalCode::Overloaded
        );
        drop(slots);
        let refusal = run(
            service.clone(),
            Operation::Replay,
            b"a".to_vec(),
            vec![],
            Some(1),
            None,
        )
        .await;
        assert_eq!(
            ServeRefusal::parse(&refusal.body).unwrap().code,
            RefusalCode::QueueDeadline
        );
        assert_eq!(service.blocking_slots.available_permits(), 2);
        drop(worker);
    }
}
