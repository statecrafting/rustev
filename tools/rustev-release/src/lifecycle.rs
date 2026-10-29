//! The release lifecycle of spec 021 3.1, as spec 058 3.6 observes it for a
//! tagged run.
//!
//! The lifecycle is a state machine over the named states of 3.1; no event
//! skips a state, and none reports one state as another (I-1). Since spec
//! 058 the verified signed tag, which the run's manifest names by object id,
//! is the owner's instruction (3.1.2): no instruction file and no
//! publication journal is read. [`Lookup`] and [`Registry`] are the public
//! registry state the publication steps of `tagged` read.

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

/// A registry's public state for a name and version.
pub trait Registry {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup;
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
