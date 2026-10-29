//! The crates.io registry's public state (spec 021, 3.9; 058 3.3.1 and
//! 3.5.2): a crate version's checksum from the public sparse index, read
//! without a token. HTTP goes through the system `curl`; no HTTP crate
//! enters the repository's tools (spec 001, 3.4.2). Uploads are `cargo
//! publish` in `tagged`, with the trusted-publishing token the workflow
//! supplies.

use std::process::Command;

use serde::Deserialize;

use crate::lifecycle::{Lookup, Registry};

pub const INDEX: &str = "https://index.crates.io";
pub const USER_AGENT: &str = "rustev-release (https://github.com/statecrafting/rustev)";

/// The sparse-index path of a crate name.
pub fn index_path(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    match n.len() {
        1 => format!("1/{n}"),
        2 => format!("2/{n}"),
        3 => format!("3/{}/{n}", &n[..1]),
        _ => format!("{}/{}/{n}", &n[..2], &n[2..4]),
    }
}

#[derive(Debug, Deserialize)]
struct IndexLine {
    vers: String,
    cksum: String,
}

/// The checksum the index records for `version`, from the index file text.
pub fn index_checksum(text: &str, version: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<IndexLine>(l).ok())
        .find(|l| l.vers == version)
        .map(|l| l.cksum)
}

/// The public sparse index.
pub struct CratesIo;

fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-q", "-sS", "--max-time", "120", "-A", USER_AGENT]);
    c
}

impl Registry for CratesIo {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup {
        // A 404 is the only absence; any other failure is unknown state.
        let out = curl()
            .args(["-w", "\n%{http_code}"])
            .arg(format!("{INDEX}/{}", index_path(name)))
            .output();
        let o = match out {
            Ok(o) => o,
            Err(e) => {
                return Lookup::Unknown {
                    detail: format!("curl: {e}"),
                };
            }
        };
        if !o.status.success() {
            return Lookup::Unknown {
                detail: "index transport failed".into(),
            };
        }
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
        match code.trim() {
            "200" => match index_checksum(body, version) {
                Some(checksum) => Lookup::Visible { checksum },
                None => Lookup::Absent,
            },
            "404" => Lookup::Absent,
            other => Lookup::Unknown {
                detail: format!("index HTTP {other}"),
            },
        }
    }
}
