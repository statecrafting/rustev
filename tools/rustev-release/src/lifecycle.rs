//! The release lifecycle, the publication journal and the publisher (spec
//! 021, 3.1 and 3.9), driven against any [`Registry`].
//!
//! The lifecycle is a state machine over the named states of 3.1; no event
//! skips a state, and none reports one state as another (I-1). The journal
//! is append-only: each attempt is written before the upload and its result
//! after it, with the observed checksum; a package whose earlier attempt was
//! accepted or ambiguous is never uploaded again without a recorded query of
//! public state first (3.9, I-3). The publisher uploads the exact verified
//! archives in the manifest's order, waits for each dependency to be visible
//! with its expected checksum, and stops on a refusal or an ambiguity.
//!
//! Tests drive it with [`FakeRegistry`]; no credential, network write or tag
//! is involved. The real registry adapter belongs to the publication step.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::manifest::ReleaseManifest;

/// Spec 031 3.11.1: the first-release instruction authorizes exactly 0.1.0.
/// Any other version requires a fresh owner instruction (031 3.11.4).
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

/// The owner's publication instruction (I-4): it names the version, the
/// exact candidate commit, the registry and the package set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instruction {
    pub version: String,
    pub commit: String,
    pub registry: String,
    pub packages: Vec<String>,
    #[serde(default)]
    pub registries: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub continuation: bool,
    #[serde(default = "publish_action")]
    pub action: String,
}

fn publish_action() -> String {
    "publish".into()
}

/// What can happen to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The implementation and repository gates pass.
    GatesPass,
    /// Candidate evidence passed at this exact commit (3.7).
    CandidateVerified { commit: String },
    /// The owner's instruction (I-4).
    Authorized(Instruction),
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
    pub instruction: Option<Instruction>,
    // Set only by the family lifecycle (`family_lifecycle`, increment 9 part
    // 1b-ii) once every artifact layer is observed public and the
    // fresh-consumer evidence is recorded; until then a manifest with
    // artifacts cannot reach `released` or `qualified`, the safe direction.
    pub(crate) family_observed: bool,
    pub(crate) consumer_recorded: bool,
}

impl Lifecycle {
    pub fn new(manifest: ReleaseManifest) -> Self {
        Lifecycle {
            state: State::Specified,
            manifest,
            candidate_commit: None,
            instruction: None,
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
            (CandidateVerified, Event::Authorized(i)) => {
                let m = &self.manifest;
                if i.version != m.version
                    || i.commit != m.commit
                    || i.registry != m.registry
                    || i.packages != m.upload_order
                    || i.action != "publish"
                    // 3.11.1 "authorizes exactly one release, family version
                    // `0.1.0`"; section 4 leaves "any release after `0.1.0`"
                    // to a fresh owner act, which this tool does not define.
                    || i.version != FIRST_RELEASE_VERSION
                    || (!m.artifacts.is_empty()
                        && (i.registries != m.registries
                            || i.artifacts
                                != m.artifacts
                                    .iter()
                                    .map(|a| a.name.clone())
                                    .collect::<Vec<_>>()))
                {
                    return refuse(
                        "the instruction does not name exactly this version, commit, registry and package set (I-4)"
                            .into(),
                    );
                }
                self.instruction = Some(i);
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

/// The registry the publisher drives.
pub trait Registry {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup;
    fn upload(&mut self, name: &str, version: &str, archive: &[u8]) -> Upload;
    /// Let time pass between visibility queries (a fake advances its clock).
    fn wait(&mut self);
}

/// Registry-supplied text with every token shape of the archive scan
/// (`rustev.archive-scan/1`) replaced by `[redacted]`, so a credential a
/// registry echoes back never enters the journal (3.9).
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

fn redacted(outcome: Upload) -> Upload {
    match outcome {
        Upload::Refused { reason } => Upload::Refused {
            reason: redact(&reason),
        },
        Upload::Ambiguous { detail } => Upload::Ambiguous {
            detail: redact(&detail),
        },
        Upload::Accepted => Upload::Accepted,
    }
}

/// One append-only journal entry (3.9). Secrets never enter it: registry
/// text passes through [`redact`] first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum Entry {
    /// Spec 031 3.11.2: exact signed bytes, before any upload.
    SignedManifest {
        manifest: String,
        signature: String,
    },
    /// Spec 031 3.1 layer 8: freeze the summary before uploading it.
    /// Written by layer 8's hosted-release adapter (increment 9 part 2).
    Summary {
        text: String,
    },
    /// Spec 031 3.9.7: immutable fresh-consumer evidence digests.
    Qualification {
        evidence: BTreeMap<String, String>,
    },
    Attempt {
        package: String,
        version: String,
        sha256: String,
        registry: String,
    },
    Result {
        package: String,
        outcome: Upload,
    },
    Query {
        package: String,
        observed: Lookup,
    },
    Stopped {
        reason: String,
    },
    /// What the operator verified before the first upload (3.9).
    Preflight {
        account: String,
        commit: String,
        tag: String,
        manifest_sha256: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Journal {
    pub entries: Vec<Entry>,
    /// When set, every entry is appended to this file as it is recorded,
    /// so an attempt is on disk before its upload starts (3.9).
    pub file: Option<std::path::PathBuf>,
    /// Set when a write to `file` failed; nothing is uploaded after it.
    pub failed: bool,
}

impl Journal {
    /// Record one entry, appending it to the journal file first when set.
    /// A journal that cannot be written stops nothing silently: the entry
    /// is kept in memory and the failure is recorded as a stop reason.
    pub fn record(&mut self, e: Entry) {
        if let Some(path) = &self.file {
            use std::io::Write;
            let line = serde_json::to_string(&e).expect("entries serialize") + "\n";
            let written = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(line.as_bytes()).and_then(|()| f.sync_all()));
            if let Err(err) = written {
                let failure = format!("journal {} not writable: {err}", path.display());
                // One `Stopped` entry, whatever was being recorded: a stop
                // that could not be written carries the write failure.
                match e {
                    Entry::Stopped { reason } => self.entries.push(Entry::Stopped {
                        reason: format!("{reason}; {failure}"),
                    }),
                    other => {
                        self.entries.push(other);
                        self.entries.push(Entry::Stopped { reason: failure });
                    }
                }
                self.file = None;
                self.failed = true;
                return;
            }
        }
        self.entries.push(e);
    }

    /// Whether the journal file failed; the publisher stops before any
    /// further upload when it did.
    pub fn broken(&self) -> bool {
        self.failed
    }

    /// JSON lines, one entry each.
    pub fn to_lines(&self) -> String {
        self.entries
            .iter()
            .map(|e| serde_json::to_string(e).expect("entries serialize") + "\n")
            .collect()
    }

    pub fn from_lines(text: &str) -> Result<Self, String> {
        let entries = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).map_err(|e| format!("journal line {l:?}: {e}")))
            .collect::<Result<_, _>>()?;
        Ok(Journal {
            entries,
            file: None,
            failed: false,
        })
    }

    /// May `package` be uploaded now? Not after an accepted or ambiguous
    /// result unless a later query of public state found it absent.
    pub(crate) fn may_attempt(&self, package: &str) -> Result<(), String> {
        // Sticky states: an accepted result blocks every later upload, and
        // an ambiguous one blocks until a later query of public state found
        // the package absent; a later refusal clears neither.
        let mut accepted = false;
        let mut ambiguous = false;
        let mut pending = false;
        for e in &self.entries {
            match e {
                Entry::Attempt { package: p, .. } if p == package => pending = true,
                Entry::Result {
                    package: p,
                    outcome,
                } if p == package => {
                    pending = false;
                    match outcome {
                        Upload::Accepted => accepted = true,
                        Upload::Ambiguous { .. } => ambiguous = true,
                        Upload::Refused { .. } => {}
                    }
                }
                Entry::Query {
                    package: p,
                    observed: Lookup::Absent,
                } if p == package => {
                    ambiguous = false;
                    pending = false;
                }
                _ => {}
            }
        }
        if accepted {
            Err(format!(
                "{package} was already accepted; never upload twice"
            ))
        } else if ambiguous {
            Err(format!(
                "{package} has an ambiguous attempt; query public state first"
            ))
        } else if pending {
            Err(format!(
                "{package} has an attempt without a recorded result; query public state first"
            ))
        } else {
            Ok(())
        }
    }
}

/// How long the publisher waits for a dependency to become visible.
pub const VISIBILITY_POLLS: u32 = 30;

/// The publisher's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every package is visible with its expected checksum.
    Complete,
    /// The run stopped; the journal says why and what is public.
    Stopped { reason: String },
}

/// Upload the manifest's archives in its order (3.9). `archives` maps each
/// archive file name to its exact verified bytes. The lifecycle must be
/// `publication-authorized` or `partially-published`.
pub fn publish(
    life: &mut Lifecycle,
    archives: &BTreeMap<String, Vec<u8>>,
    registry: &mut dyn Registry,
    journal: &mut Journal,
) -> Outcome {
    // A journal whose write failed already ends with that failure as its
    // one `Stopped` entry; a later stop reports its reason without a second.
    let stop = |journal: &mut Journal, reason: String| {
        if !journal.broken() {
            journal.record(Entry::Stopped {
                reason: reason.clone(),
            });
        }
        Outcome::Stopped { reason }
    };
    if !matches!(
        life.state,
        State::PublicationAuthorized | State::PartiallyPublished
    ) {
        return stop(
            journal,
            format!(
                "no publication without the owner's instruction (state {:?})",
                life.state
            ),
        );
    }
    let m = life.manifest.clone();
    if !m.artifacts.is_empty() {
        if journal
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Stopped { .. }))
            && !life.instruction.as_ref().is_some_and(|i| i.continuation)
        {
            return stop(
                journal,
                "partial publication needs fresh continuation instruction".into(),
            );
        }
        if m.limitations
            .iter()
            .any(|l| l == crate::family_artifacts::MISSING)
        {
            return stop(
                journal,
                "missing rehearsal artifacts; not publishable".into(),
            );
        }
    }
    if m.packages.is_empty() {
        return stop(journal, "an empty package set publishes nothing".into());
    }
    // `Observed` applies in both states checked above, and a run applies it
    // once, immediately before returning, so it is never refused; a refusal
    // would be a defect in this state machine and fails loudly in debug.
    let mark = |life: &mut Lifecycle, complete: bool| {
        // Observed follows an accepted upload from an authorized or
        // partially published state, which the machine always allows; a
        // refusal is a defect, and stopping the publisher before any
        // further upload is the only safe answer, in every build.
        life.apply(Event::Observed { complete })
            .expect("an accepted upload is always observable");
    };
    let mut visible = 0usize;
    let observe = |life: &mut Lifecycle, visible: usize| {
        if visible > 0 {
            mark(life, visible == m.packages.len() && m.artifacts.is_empty());
        }
    };
    for p in &m.packages {
        let Some(bytes) = archives.get(&p.crate_file) else {
            observe(life, visible);
            return stop(
                journal,
                format!("{} is not among the verified archives", p.crate_file),
            );
        };
        if crate::archive::sha256_hex(bytes) != p.sha256 {
            observe(life, visible);
            return stop(
                journal,
                format!(
                    "{} is not the verified archive; never upload a rebuild",
                    p.crate_file
                ),
            );
        }
        // Already public from an earlier run: the same bytes are not uploaded
        // again, and different bytes under this version are a stop.
        match registry.lookup(&p.name, &m.version) {
            Lookup::Visible { checksum } if checksum == p.sha256 => {
                journal.record(Entry::Query {
                    package: p.name.clone(),
                    observed: Lookup::Visible { checksum },
                });
                visible += 1;
                continue;
            }
            Lookup::Visible { checksum } => {
                journal.record(Entry::Query {
                    package: p.name.clone(),
                    observed: Lookup::Visible { checksum },
                });
                observe(life, visible);
                return stop(
                    journal,
                    format!(
                        "{} {} is public with another checksum; the version is spent",
                        p.name, m.version
                    ),
                );
            }
            Lookup::Absent => {}
            Lookup::Unknown { detail } => {
                journal.record(Entry::Query {
                    package: p.name.clone(),
                    observed: Lookup::Unknown {
                        detail: detail.clone(),
                    },
                });
                observe(life, visible);
                return stop(
                    journal,
                    format!(
                        "public state of {} is unknown ({detail}); nothing uploaded",
                        p.name
                    ),
                );
            }
        }
        if let Err(e) = journal.may_attempt(&p.name) {
            observe(life, visible);
            return stop(journal, e);
        }
        if journal.broken() {
            observe(life, visible);
            return Outcome::Stopped {
                reason: "the journal cannot be written".into(),
            };
        }
        journal.record(Entry::Attempt {
            package: p.name.clone(),
            version: m.version.clone(),
            sha256: p.sha256.clone(),
            registry: m.registry.clone(),
        });
        if journal.broken() {
            observe(life, visible);
            return Outcome::Stopped {
                reason: "the attempt could not be journaled; not uploaded".into(),
            };
        }
        let outcome = redacted(registry.upload(&p.name, &m.version, bytes));
        journal.record(Entry::Result {
            package: p.name.clone(),
            outcome: outcome.clone(),
        });
        match outcome {
            Upload::Refused { reason } => {
                observe(life, visible);
                return stop(journal, format!("{} refused: {reason}", p.name));
            }
            Upload::Ambiguous { detail } => {
                // Query public state first; record it; never retry blindly.
                let observed = registry.lookup(&p.name, &m.version);
                journal.record(Entry::Query {
                    package: p.name.clone(),
                    observed: observed.clone(),
                });
                // Any visible version, with these bytes or others, means
                // something of this release is public: the run stops as
                // partially published, never complete, whichever bytes the
                // registry holds (the journal records which).
                let public = visible > 0 || matches!(observed, Lookup::Visible { .. });
                if public {
                    mark(life, false);
                }
                return stop(
                    journal,
                    format!(
                        "{} ambiguous: {detail}; the owner decides how to continue",
                        p.name
                    ),
                );
            }
            Upload::Accepted => {}
        }
        // Wait until it is visible with the expected checksum before the
        // next package, which may depend on it.
        let mut seen = None;
        // The last observation, journaled as it was if the package never
        // appears (absent and unknown are recorded as themselves).
        let mut last = Lookup::Absent;
        for _ in 0..VISIBILITY_POLLS {
            match registry.lookup(&p.name, &m.version) {
                l @ (Lookup::Absent | Lookup::Unknown { .. }) => {
                    last = l;
                    registry.wait();
                }
                v => {
                    seen = Some(v);
                    break;
                }
            }
        }
        match seen {
            Some(Lookup::Visible { checksum }) if checksum == p.sha256 => {
                journal.record(Entry::Query {
                    package: p.name.clone(),
                    observed: Lookup::Visible { checksum },
                });
                visible += 1;
            }
            Some(other) => {
                journal.record(Entry::Query {
                    package: p.name.clone(),
                    observed: other,
                });
                // Reached only after this package's upload was accepted above,
                // and 3.11.4 makes any stop after an acceptance `partially
                // published`: the package is now public (with other bytes)
                // even when nothing earlier is, so the release is never
                // complete; `observe` counts only packages confirmed with
                // their expected checksum.
                mark(life, false);
                return stop(
                    journal,
                    format!("{} is visible with a different checksum", p.name),
                );
            }
            None => {
                journal.record(Entry::Query {
                    package: p.name.clone(),
                    observed: last,
                });
                mark(life, false);
                return stop(
                    journal,
                    format!(
                        "{} accepted but not visible after {VISIBILITY_POLLS} polls",
                        p.name
                    ),
                );
            }
        }
    }
    observe(life, visible);
    Outcome::Complete
}

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
        }
        outcome
    }

    fn wait(&mut self) {
        self.now += 1;
    }
}
