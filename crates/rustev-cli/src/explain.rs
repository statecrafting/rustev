//! File host for the pure decision explainer (spec 040, 3.16).
use rustev_contract::{
    Document,
    explanation::{Availability, Item, Query},
    limits::{DESCRIPTOR_V1, PLAN_V1, RECORD_V1, REPLAY_V1},
    redaction::RedactionPolicy,
};
use rustev_eval::explain::{
    Evidence, ExplainError, ExplainOptions, explain, explain_diff, render_markdown,
};

use crate::{
    args::{Parsed, Usage, usage},
    host,
    io::Reads,
    out::{Done, Out, Res, code},
    replay,
};

fn query(p: &Parsed) -> Result<Query, Usage> {
    let item = |flag| Item::String(p.req(flag).into());
    let q = match p.spec.path[1] {
        "why-selected" => Query::WhySelected { item: item("item") },
        "why-excluded" => Query::WhyExcluded { item: item("item") },
        "why-order" => Query::WhyOrder {
            a: item("a"),
            b: item("b"),
        },
        "why-outcome" => Query::WhyOutcome,
        "text-origin" => Query::TextOrigin,
        "models" => Query::Models,
        "diff" => Query::Diff,
        _ => return usage("unknown explanation query"),
    };
    q.validate()
        .map_err(|_| Usage("items must be 1 to 256 bytes; --a and --b must differ".into()))?;
    Ok(q)
}

fn evidence_flags(p: &Parsed, record: &str, bundle: &str, plan: &str) -> Result<(), Usage> {
    if p.has(record) == p.has(bundle) {
        return usage(format!(
            "exactly one of --{record} or --{bundle} is required"
        ));
    }
    if p.has(bundle) {
        p.forbid(&[plan], &format!("cannot accompany --{bundle}"))?;
    }
    Ok(())
}

pub fn run(p: &Parsed) -> Result<Res, Usage> {
    let q = query(p)?;
    evidence_flags(p, "record", "bundle", "plan")?;
    if q == Query::Diff {
        evidence_flags(p, "other-record", "other-bundle", "other-plan")?;
    }
    let config = if p.has("bundle") || p.has("other-bundle") {
        p.need(&["now-ms"], "with any bundle side")?;
        Some(replay::replay_config(p)?)
    } else {
        p.forbid(&host::SCOPE_FLAGS, "requires a bundle side")?;
        p.forbid(&["now-ms", "store"], "requires a bundle side")?;
        None
    };
    Ok(execute(p, &q, config.as_ref()))
}

struct Input {
    bytes: Vec<u8>,
    plan: Option<Vec<u8>>,
    bundle: bool,
    path: String,
    plan_path: Option<String>,
}
impl Input {
    fn read(p: &Parsed, reads: &Reads, other: bool) -> Result<Self, Done> {
        let (record, bundle, plan) = if other {
            ("other-record", "other-bundle", "other-plan")
        } else {
            ("record", "bundle", "plan")
        };
        let is_bundle = p.has(bundle);
        let path = p.req(if is_bundle { bundle } else { record });
        let read = |path, limit| reads.read(path, limit).map_err(|e| Done::io(&p.name(), e));
        Ok(Self {
            bytes: read(
                path,
                if is_bundle {
                    REPLAY_V1.max_bytes
                } else {
                    RECORD_V1.max_bytes
                },
            )?,
            plan: p
                .one(plan)
                .map(|f| read(f, PLAN_V1.max_bytes))
                .transpose()?,
            bundle: is_bundle,
            path: path.into(),
            plan_path: p.one(plan).map(str::to_string),
        })
    }
    fn evidence<'a>(
        &'a self,
        config: Option<&'a rustev_eval::replay::ReplayConfig>,
        resolver: &'a dyn rustev_eval::resolve::Resolver,
    ) -> Evidence<'a> {
        if self.bundle {
            let config = config.expect("bundle flags checked before reading");
            Evidence::Bundle {
                bundle: &self.bytes,
                scope: &config.scope,
                now_ms: config.now_ms,
                resolver,
            }
        } else {
            Evidence::Record {
                record: &self.bytes,
                plan: self.plan.as_deref(),
            }
        }
    }
    fn refusal(&self, command: &str, e: ExplainError) -> Done {
        let path = if e.subject == "plan" {
            self.plan_path.as_deref().unwrap_or(&self.path)
        } else {
            &self.path
        };
        refusal(command, path, &e.subject, e.code.as_str(), &e.detail)
    }
}

fn refusal(command: &str, path: &str, item: &str, reason: &str, detail: &str) -> Done {
    Done::new(
        code::INVALID_INPUT,
        Out::new(command, "invalid_input")
            .set("input", path)
            .set("item", item)
            .set("reason", reason)
            .set("detail", detail),
    )
}

fn execute(p: &Parsed, q: &Query, config: Option<&rustev_eval::replay::ReplayConfig>) -> Res {
    let command = p.name();
    let reads = Reads::default();
    for path in [p.one("out"), p.one("markdown")].into_iter().flatten() {
        crate::io::ensure_absent(path).map_err(|e| Done::io(&command, e))?;
    }
    let policy = p
        .one("redaction")
        .map(|path| {
            let bytes = reads
                .read(path, DESCRIPTOR_V1.max_bytes)
                .map_err(|e| Done::io(&command, e))?;
            RedactionPolicy::parse(&bytes)
                .map_err(|e| refusal(&command, path, "redaction", "parse", &e.to_string()))
        })
        .transpose()?;
    let options = ExplainOptions {
        redaction: policy.as_ref(),
    };
    let a = Input::read(p, &reads, false)?;
    let resolver = replay::resolver(p);
    let ae = a.evidence(config, resolver.as_ref());
    let result = if *q == Query::Diff {
        let b = Input::read(p, &reads, true)?;
        let be = b.evidence(config, resolver.as_ref());
        explain_diff(&ae, &be, &options).map_err(|e| {
            // The library's refusal names a role, not a diff side. On a
            // refusal only, determine which file supplied the invalid set.
            if explain(&ae, &Query::Models, &options).is_err() {
                a.refusal(&command, e)
            } else {
                b.refusal(&command, e)
            }
        })?
    } else {
        explain(&ae, q, &options).map_err(|e| a.refusal(&command, e))?
    };
    // The explainer only returns documents its bounds admit, so encoding
    // fails only past those bounds: 3.17's code for that, as the library
    // itself reports it.
    let too_large = |e: rustev_contract::canonical::CanonicalError| {
        refusal(
            &command,
            p.req("out"),
            "explanation",
            "explanation-too-large",
            &e.to_string(),
        )
    };
    let bytes = result.record_canonical().map_err(too_large)?;
    let digest = result.record_digest().map_err(too_large)?;
    let answer = serde_json::to_value(&result.answer).expect("answer serializes");
    let count = result
        .facts
        .iter()
        .filter(|f| f.availability != Availability::Available)
        .count();
    let mut files = vec![(p.req("out"), bytes)];
    if let Some(path) = p.one("markdown") {
        files.push((path, render_markdown(&result).into_bytes()));
    }
    rustev_surface::eval::write_all(&command, &reads, "", files)?;
    Ok(Done::ok(
        Out::new(&command, "explained")
            .set("explanation", &digest)
            .set("answer", &answer["kind"])
            .set("not_available", &count),
    ))
}
