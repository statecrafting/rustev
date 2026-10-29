//! The release lifecycle of spec 021 3.1, as spec 058 3.6 observes it for a
//! tagged run.
//!
//! The lifecycle is a state machine over the named states of 3.1; no event
//! skips a state, and none reports one state as another (I-1). Since spec
//! 058 the verified signed tag, which the run's manifest names by object id,
//! is the owner's instruction (3.1.2): no instruction file and no
//! publication journal is read. [`Lookup`] and [`Registry`] are the public
//! registry state the publication steps of `tagged` read.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::manifest::ReleaseManifest;

/// Spec 031 3.11.1: the first-release instruction authorized exactly 0.1.0,
/// and that authority is spent (058 3.6); no tagged run publishes 0.1.0.
pub const FIRST_RELEASE_VERSION: &str = "0.1.0";

/// The named states of 3.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Specified,
    Implemented,
    CandidateVerified,
    PublicationAuthorized,
    PartiallyPublished,
    Published,
    Released,
    Qualified,
}

/// What can happen to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The implementation and repository gates pass.
    GatesPass,
    /// Candidate evidence passed at this exact commit (3.7).
    CandidateVerified { commit: String },
    /// Spec 058 3.1.2: the verified signed tag of a tagged run, which the
    /// run's manifest names by object id, is the owner's instruction.
    TagAuthorized { tag_object: String },
    /// The publisher observed these packages visible with their expected
    /// checksums; `complete` when that is every package of the set.
    Observed { complete: bool },
    /// The signed tag and hosted release exist with every artifact (3.8).
    ReleaseCreated,
    /// The fresh registry-only consumer passed (3.10).
    ConsumerQualified,
}

/// Why an event does not apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

/// The lifecycle of one release identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    pub state: State,
    pub manifest: ReleaseManifest,
    pub candidate_commit: Option<String>,
    // Set only by `tagged::qualify` once every artifact layer is recorded
    // public and the fresh-consumer evidence is validated; until then a
    // manifest with artifacts cannot reach `released` or `qualified`, the
    // safe direction.
    pub(crate) family_observed: bool,
    pub(crate) consumer_recorded: bool,
}

impl Lifecycle {
    pub fn new(manifest: ReleaseManifest) -> Self {
        Lifecycle {
            state: State::Specified,
            manifest,
            candidate_commit: None,
            family_observed: false,
            consumer_recorded: false,
        }
    }

    /// Apply one event, or refuse it and leave the state unchanged.
    pub fn apply(&mut self, event: Event) -> Result<State, Refusal> {
        use State::*;
        let refuse = |why: String| Err(Refusal(why));
        let next = match (self.state, event) {
            (Specified, Event::GatesPass) => Implemented,
            (Implemented, Event::CandidateVerified { commit }) => {
                if commit != self.manifest.commit {
                    return refuse(format!(
                        "candidate evidence is for {commit}, the manifest names {}",
                        self.manifest.commit
                    ));
                }
                self.candidate_commit = Some(commit);
                CandidateVerified
            }
            (CandidateVerified, Event::TagAuthorized { tag_object }) => {
                // 0.1.0's authority is spent (031 3.11.1); a tagged run's
                // manifest names its tag object (058 3.2.4).
                if self.manifest.version == FIRST_RELEASE_VERSION
                    || self.manifest.tag_object.as_deref() != Some(tag_object.as_str())
                {
                    return refuse(
                        "058 3.1.2: the manifest does not name this verified tag object".into(),
                    );
                }
                PublicationAuthorized
            }
            (PublicationAuthorized | PartiallyPublished, Event::Observed { complete }) => {
                if complete && !self.manifest.artifacts.is_empty() && !self.family_observed {
                    return refuse("family layers not observed".into());
                }
                if complete {
                    Published
                } else {
                    PartiallyPublished
                }
            }
            (Published, Event::ReleaseCreated) => {
                if !self.manifest.artifacts.is_empty() && !self.family_observed {
                    return refuse("layer 8 not observed".into());
                }
                Released
            }
            (Released, Event::ConsumerQualified) => {
                if !self.manifest.artifacts.is_empty() && !self.consumer_recorded {
                    return refuse("fresh-consumer evidence not recorded".into());
                }
                Qualified
            }
            (s, e) => return refuse(format!("{e:?} does not apply in state {s:?}")),
        };
        self.state = next;
        Ok(next)
    }

    /// Only `qualified` supports a claim that the release is publicly
    /// accessible (3.1).
    pub fn publicly_accessible(&self) -> bool {
        self.state == State::Qualified
    }
}

/// What a registry reports for a name and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lookup {
    Absent,
    Visible {
        checksum: String,
    },
    /// Public state could not be read (a network error, a server error, an
    /// unreadable index). It is never taken for absence.
    Unknown {
        detail: String,
    },
}

/// What an upload returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Upload {
    Accepted,
    Refused {
        reason: String,
    },
    /// The outcome is unknown: a timeout, a lost response, a server error.
    Ambiguous {
        detail: String,
    },
}

/// A registry: its public state, and the uploads of the fake below.
pub trait Registry {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup;
    fn upload(&mut self, name: &str, version: &str, archive: &[u8]) -> Upload;
    /// The public owner logins of `name`; empty when no such name exists.
    /// An error is unknown state, never taken for no owners.
    fn owners(&mut self, name: &str) -> Result<Vec<String>, String>;
    /// Let time pass between visibility queries (a fake advances its clock).
    fn wait(&mut self);
}

/// Registry-supplied text with every token shape of the archive scan
/// (`rustev.archive-scan/1`) replaced by `[redacted]`, so a credential a
/// registry echoes back never enters a log or the publication record
/// (3.9; 058 3.5.4).
// The splices below index `text` by the scan's byte offsets: each rule's
// match begins with an ASCII byte, and a token ends at an ASCII delimiter, a
// footer or the end of the text, so every offset is a character boundary.
pub fn redact(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for f in crate::archive::scan(bytes) {
        if f.offset < at {
            continue;
        }
        let end = if f.rule == "private-key" {
            // A key's header is followed by its body: redact through the
            // matching footer, or to the end of the text.
            let rest = &text[f.offset..];
            rest.find("-----END ")
                .and_then(|e| rest[e..].find("PRIVATE KEY-----").map(|k| e + k + 16))
                .map_or(text.len(), |n| f.offset + n)
        } else {
            f.offset
                + bytes[f.offset..]
                    .iter()
                    // Structural delimiters end a token so the text around
                    // it survives; `:` does not, since `user:secret` would
                    // leak.
                    .take_while(|b| {
                        !b.is_ascii_whitespace()
                            && !matches!(
                                b,
                                b'"' | b'\'' | b',' | b'}' | b']' | b')' | b'<' | b'>' | b';'
                            )
                    })
                    .count()
        };
        // Every rule's match begins with a token character, so a token is
        // never empty; were one empty, the rest of the text is redacted
        // rather than re-emitted.
        let end = if end > f.offset { end } else { text.len() };
        out.push_str(&text[at..f.offset]);
        out.push_str("[redacted]");
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// The login a [`FakeRegistry`] records as the owner of what it accepts.
pub const FAKE_ACCOUNT: &str = "rustev-fixture";

/// A scripted in-memory registry for the integration tests and dry runs.
/// It is public because `tests/` and the dry-run harness are separate
/// crates; this tool crate is never published (an exclusion of the release
/// set), so no consumer ever links it.
#[derive(Debug, Default)]
pub struct FakeRegistry {
    /// Published versions: (name, version) -> (checksum, visible at tick).
    pub index: BTreeMap<(String, String), (String, u64)>,
    /// Scripted outcomes per package name; `Accepted` when absent.
    pub script: BTreeMap<String, Upload>,
    /// Ambiguous uploads that were in fact accepted.
    pub ambiguous_accepts: Vec<String>,
    /// Ticks between acceptance and visibility.
    pub lag: u64,
    pub now: u64,
    pub uploads: Vec<String>,
    /// Owner logins per name; an accepted upload adds `uploader`.
    pub owners: BTreeMap<String, Vec<String>>,
    /// The account uploads are made as; [`FAKE_ACCOUNT`] when `None`.
    pub uploader: Option<String>,
}

impl Registry for FakeRegistry {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup {
        match self.index.get(&(name.to_string(), version.to_string())) {
            Some((checksum, at)) if *at <= self.now => Lookup::Visible {
                checksum: checksum.clone(),
            },
            _ => Lookup::Absent,
        }
    }

    fn upload(&mut self, name: &str, version: &str, archive: &[u8]) -> Upload {
        self.uploads.push(name.to_string());
        let key = (name.to_string(), version.to_string());
        if self.index.contains_key(&key) {
            return Upload::Refused {
                reason: format!("{name} {version} already exists"),
            };
        }
        let outcome = self.script.get(name).cloned().unwrap_or(Upload::Accepted);
        let accepted = match &outcome {
            Upload::Accepted => true,
            Upload::Ambiguous { .. } => self.ambiguous_accepts.iter().any(|n| n == name),
            Upload::Refused { .. } => false,
        };
        if accepted {
            self.index.insert(
                key,
                (crate::archive::sha256_hex(archive), self.now + self.lag),
            );
            let uploader = self.uploader.as_deref().unwrap_or(FAKE_ACCOUNT);
            let owners = self.owners.entry(name.to_string()).or_default();
            if !owners.iter().any(|o| o == uploader) {
                owners.push(uploader.to_string());
            }
        }
        outcome
    }

    fn owners(&mut self, name: &str) -> Result<Vec<String>, String> {
        Ok(self.owners.get(name).cloned().unwrap_or_default())
    }

    fn wait(&mut self) {
        self.now += 1;
    }
}
