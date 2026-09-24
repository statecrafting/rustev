//! Host inputs given by flags (spec 006, 3.1.3 and 3.1.4): the trusted
//! isolation scope and the wall-clock time. Both are checked with the other
//! usage rules, before any file is read.

use rustev_contract::scope::{Handle, Scope};
use rustev_contract::time::Timestamp;

use crate::args::{Parsed, Usage, uint, usage};

/// The scope flags, in the order [`scope`] reads them.
pub const SCOPE_FLAGS: [&str; 4] = ["scope", "tenant", "context-revision", "principal-scope"];

fn handle(p: &Parsed, flag: &str) -> Result<Handle, Usage> {
    let Some(v) = p.one(flag) else {
        return usage(format!("--{flag} is required"));
    };
    Handle::new(v).map_err(|e| Usage(format!("--{flag}: {e}")))
}

/// The caller's trusted scope. Principal mode is the default; a missing
/// principal scope is an error there, never a downgrade to tenant-only.
pub fn scope(p: &Parsed) -> Result<Scope, Usage> {
    let tenant = handle(p, "tenant")?;
    let revision = handle(p, "context-revision")?;
    match p.one("scope").unwrap_or("principal") {
        "principal" => {
            if !p.has("principal-scope") {
                return usage(
                    "--principal-scope is required in principal mode (the default); \
                     tenant-only must be chosen explicitly with --scope tenant-only",
                );
            }
            Ok(Scope::principal(
                tenant,
                revision,
                handle(p, "principal-scope")?,
            ))
        }
        "tenant-only" => {
            p.forbid(&["principal-scope"], "is not part of a tenant-only scope")?;
            Ok(Scope::tenant_only(tenant, revision))
        }
        other => usage(format!(
            "--scope is `principal` or `tenant-only`, not {other:?}"
        )),
    }
}

/// `--now-ms`: milliseconds since the Unix epoch, or `system`, which reads
/// the system clock once.
pub fn now(p: &Parsed) -> Result<Timestamp, Usage> {
    let v = p.req("now-ms");
    let ms = if v == "system" {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Usage("the system clock is before the Unix epoch".into()))?;
        i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
    } else {
        uint(v).and_then(|x| i64::try_from(x).ok()).ok_or_else(|| {
            Usage(format!(
                "--now-ms takes milliseconds or `system`, not {v:?}"
            ))
        })?
    };
    Timestamp::from_ms(ms).map_err(|_| Usage(format!("--now-ms {ms} is out of range")))
}

/// A timestamp flag in milliseconds since the Unix epoch.
pub fn timestamp(p: &Parsed, flag: &str) -> Result<Timestamp, Usage> {
    let v = p.req(flag);
    uint(v)
        .and_then(|x| i64::try_from(x).ok())
        .and_then(|x| Timestamp::from_ms(x).ok())
        .ok_or_else(|| {
            Usage(format!(
                "--{flag} takes milliseconds in the contract's range, not {v:?}"
            ))
        })
}
