//! The persistent spend journal that holds R-29's caps (spec 012, 3.8): a
//! shared cost ledger that outlives a run (spec 003, 3.5.2), kept by the
//! host outside any repository.
//!
//! Two budgets: `testing`, USD 10 in total (10,000,000,000 units of one
//! nano-USD, never reset; A-75 raised it from R-29's USD 5), and `production`, USD 25 per UTC calendar month
//! (25,000,000,000 units per month). Before dispatch the adapter reserves
//! its estimate; the journal refuses (a hard stop, nothing is sent) when
//! observed + estimated + liability + outstanding reservations + this
//! reservation would pass the cap. After the answer the reservation is
//! settled with the charge; an `unknown` charge keeps the reservation as
//! liability until a later observed charge reconciles it.
//!
//! **Format.** Append-only JSON lines, each written with one `write` and
//! flushed to disk with `fsync` before the call returns. The first line is
//! a header naming the schema, the budget and the cap; every later line is
//! one of `reserve`, `settle`, `reconcile` or `release`, keyed by a
//! reservation sequence number. The state is the replay of the lines. A
//! reservation is durable before its request can be sent.
//!
//! **Header migration (A-75).** A testing journal whose header names the
//! superseded cap ([`SUPERSEDED_TESTING_CAP_UNITS`]) is opened under the
//! lock and its header is replaced by the current one: a copy holding the
//! new header followed by every later byte unchanged is written beside the
//! journal, `fsync`ed and renamed over it, and the directory is `fsync`ed.
//! A crash leaves either the old file or the new one, never a mix. Entries,
//! sequence numbers and unsettled reservations are untouched. Any other
//! header that differs from the expected one is still refused.
//!
//! **Crash safety.** A final line without its newline was never completed:
//! it is cut off at open. Since a reservation is fsynced before sending, a
//! cut reservation was never sent; a cut settlement leaves its reservation
//! outstanding. A reservation still outstanding when the journal is opened
//! (its process ended before settling) counts as liability: its request may
//! have been sent. Every failure therefore errs toward counting more.
//!
//! **One writer.** A lock file beside the journal (`<journal>.lock`,
//! created exclusively) keeps a second process out. A process that dies
//! leaves it behind; the host removes it after checking that no process
//! uses the journal. Within a process, clones share one journal.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rustev_contract::execution::CostPolicy;
use rustev_contract::run::Charge;
use rustev_runtime::Ledger;
use serde::{Deserialize, Serialize};

pub const JOURNAL_SCHEMA: &str = "rustev-jev.spend-journal/1";
/// R-29 as raised by A-75 (2026-11-03): USD 10 in total for testing, in
/// nano-USD.
pub const TESTING_CAP_UNITS: u64 = 10_000_000_000;
/// R-29's original testing cap, USD 5, in nano-USD. A testing journal whose
/// header names it is migrated to [`TESTING_CAP_UNITS`] when opened.
pub const SUPERSEDED_TESTING_CAP_UNITS: u64 = 5_000_000_000;
/// R-29: USD 25 per calendar month for production, in nano-USD.
pub const PRODUCTION_CAP_UNITS: u64 = 25_000_000_000;

/// Which of R-29's caps a journal holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    /// Smoke, qualification and evaluation runs together; never resets.
    Testing,
    /// Production use; resets at each UTC calendar month.
    Production,
}

impl BudgetKind {
    pub const fn cap(self) -> u64 {
        match self {
            BudgetKind::Testing => TESTING_CAP_UNITS,
            BudgetKind::Production => PRODUCTION_CAP_UNITS,
        }
    }

    /// The period a reservation made at `now_ms` counts against: `total`
    /// for testing, the UTC month `YYYY-MM` for production.
    pub fn period(self, now_ms: i64) -> String {
        match self {
            BudgetKind::Testing => "total".to_string(),
            BudgetKind::Production => utc_month(now_ms),
        }
    }
}

/// `YYYY-MM` of a Unix time in milliseconds, in UTC.
pub fn utc_month(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

/// The time the journal reads to choose a period. Injected, so tests can
/// cross a month boundary.
pub trait SpendClock: Send + Sync {
    fn now_ms(&self) -> i64;
}

/// The system clock.
pub struct SystemClock;

impl SpendClock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}

/// A clock that reads what it was set to.
#[derive(Debug, Default)]
pub struct SetClock(AtomicI64);

impl SetClock {
    pub fn new(ms: i64) -> Self {
        SetClock(AtomicI64::new(ms))
    }
    pub fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl SpendClock for SetClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    budget: BudgetKind,
    cap: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    journal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at_ms: Option<i64>,
}

/// The persistent identity and this run's relationship to the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalIdentity {
    pub id: Option<String>,
    pub created_at_ms: Option<i64>,
    pub created: JournalCreated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalCreated {
    ThisRun,
    Earlier,
    Legacy,
}

fn digest(bytes: &[u8]) -> rustev_contract::ids::ContentDigest {
    rustev_contract::ids::ContentDigest::parse(&rustev_contract::canonical::tagged_digest(
        JOURNAL_SCHEMA,
        bytes,
    ))
    .expect("computed digest")
}

impl Header {
    fn new(kind: BudgetKind, created_at_ms: i64, run_id: &str) -> Self {
        let record = serde_json::json!({"created_at_ms": created_at_ms, "run_id": run_id});
        let bytes = rustev_contract::canonical::canonical_bytes(&record).expect("identity record");
        Self {
            schema: JOURNAL_SCHEMA.into(),
            budget: kind,
            cap: kind.cap(),
            journal_id: Some(rustev_contract::canonical::tagged_digest(
                "rustev.spend-journal-id/1",
                &bytes,
            )),
            created_at_ms: Some(created_at_ms),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(self).expect("header serializes");
        bytes.push(b'\n');
        bytes
    }

    fn parse(bytes: &[u8], kind: BudgetKind) -> Result<Self, JournalError> {
        let header: Self = serde_json::from_slice(bytes).map_err(|e| JournalError::Corrupt {
            line: 1,
            detail: format!("header: {e}"),
        })?;
        if !bytes.ends_with(b"\n")
            || header.journal_id.is_some() != header.created_at_ms.is_some()
            || header
                .journal_id
                .as_ref()
                .is_some_and(|id| rustev_contract::ids::ContentDigest::parse(id).is_err())
        {
            return Err(JournalError::Mismatch(
                "invalid journal identity or incomplete header".into(),
            ));
        }
        if header.schema != JOURNAL_SCHEMA
            || header.budget != kind
            || (header.cap != kind.cap()
                && !(kind == BudgetKind::Testing && header.cap == SUPERSEDED_TESTING_CAP_UNITS))
        {
            return Err(JournalError::Mismatch(
                "journal schema, budget or cap differs".into(),
            ));
        }
        Ok(header)
    }
}

fn read_header(path: &Path) -> Result<Option<Vec<u8>>, JournalError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
    };
    let mut bytes = Vec::new();
    BufReader::new(file)
        .read_until(b'\n', &mut bytes)
        .map_err(io)?;
    Ok(Some(bytes))
}

// Change only a plainly spelled top-level cap token in a validated header.
// Skip entire strings, including escaped quotes, so their contents cannot
// masquerade as member names. Escaped key spellings use the typed fallback.
fn with_cap(bytes: &[u8], cap: u64) -> Vec<u8> {
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
                if depth == 1 && &bytes[start..i] == br#""cap""# {
                    let at = |n: usize| bytes.get(n).copied().unwrap_or(b'}');
                    let mut next = i;
                    while at(next).is_ascii_whitespace() {
                        next += 1;
                    }
                    if at(next) == b':' {
                        next += 1;
                        while at(next).is_ascii_whitespace() {
                            next += 1;
                        }
                        let value_start = next;
                        while at(next).is_ascii_digit() {
                            next += 1;
                        }
                        let mut result = bytes[..value_start].to_vec();
                        result.extend_from_slice(cap.to_string().as_bytes());
                        result.extend_from_slice(&bytes[next..]);
                        return result;
                    }
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    let mut header: Header = serde_json::from_slice(bytes).expect("validated header");
    header.cap = cap;
    header.bytes()
}

/// Read-only journal preparation. Opening checks these bytes again under the lock.
#[derive(Clone)]
pub struct JournalOpening {
    path: PathBuf,
    kind: BudgetKind,
    clock: Arc<dyn SpendClock>,
    original: Option<Vec<u8>>,
    header: Header,
}

impl JournalOpening {
    pub fn prepare(
        path: impl AsRef<Path>,
        kind: BudgetKind,
        clock: Arc<dyn SpendClock>,
        run_id: &str,
        require_existing: bool,
    ) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        let original = read_header(&path)?;
        if require_existing && original.is_none() {
            return Err(JournalError::Missing);
        }
        let header = match &original {
            Some(bytes) if !bytes.is_empty() => Header::parse(bytes, kind)?,
            _ => Header::new(kind, clock.now_ms(), run_id),
        };
        Ok(Self {
            path,
            kind,
            clock,
            original,
            header,
        })
    }

    fn current_header_bytes(&self) -> Vec<u8> {
        match &self.original {
            Some(bytes) if bytes.is_empty() => self.header.bytes(),
            Some(bytes) if self.header.cap != self.kind.cap() => with_cap(bytes, self.kind.cap()),
            Some(bytes) => bytes.clone(),
            None => self.header.bytes(),
        }
    }

    pub fn header_digest(&self) -> rustev_contract::ids::ContentDigest {
        digest(&self.current_header_bytes())
    }

    pub fn superseded_header_digest(&self) -> rustev_contract::ids::ContentDigest {
        digest(&with_cap(
            &self.current_header_bytes(),
            SUPERSEDED_TESTING_CAP_UNITS,
        ))
    }

    pub fn identity(&self) -> JournalIdentity {
        JournalIdentity {
            id: self.header.journal_id.clone(),
            created_at_ms: self.header.created_at_ms,
            created: if self.header.journal_id.is_none() {
                JournalCreated::Legacy
            } else if self.original.as_ref().is_none_or(Vec::is_empty) {
                JournalCreated::ThisRun
            } else {
                JournalCreated::Earlier
            },
        }
    }

    pub fn open(&self) -> Result<SpendJournal, JournalError> {
        SpendJournal::open_prepared(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Entry {
    Reserve {
        seq: u64,
        period: String,
        attempt: String,
        units: u64,
        at_ms: i64,
    },
    Settle {
        seq: u64,
        charge: Charge,
    },
    Reconcile {
        seq: u64,
        units: u64,
    },
    Release {
        seq: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResState {
    Outstanding,
    Observed(u64),
    Estimated(u64),
    Liability,
    Unknown,
    Released,
}

#[derive(Debug, Clone)]
struct Res {
    attempt: String,
    period: String,
    units: u64,
    state: ResState,
}

/// A reservation made before dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reservation {
    pub seq: u64,
    pub units: u64,
}

/// The run's settled charges and exposure, across all periods.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSpend {
    pub observed: u64,
    pub estimated: u64,
    pub liability: u64,
    pub unknown: u64,
}

/// The journal's totals for one period, in units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpendSummary {
    pub budget: BudgetKind,
    pub period: String,
    pub cap: u64,
    pub observed: u64,
    pub estimated: u64,
    /// Reservations whose charge is unknown, including those a previous
    /// process left unsettled.
    pub liability: u64,
    /// Reservations of this process not yet settled.
    pub outstanding: u64,
    /// `cap - (observed + estimated + liability + outstanding)`, floored
    /// at zero.
    pub headroom: u64,
}

impl SpendSummary {
    pub fn committed(&self) -> u64 {
        self.observed
            .saturating_add(self.estimated)
            .saturating_add(self.liability)
            .saturating_add(self.outstanding)
    }
}

/// Why a journal could not be opened.
#[derive(Debug)]
pub enum JournalError {
    Missing,
    Changed,
    Io(String),
    /// Another writer holds `<journal>.lock`.
    Locked(PathBuf),
    /// The header names another budget, cap or schema.
    Mismatch(String),
    /// A complete line that does not parse, or refers to no reservation.
    Corrupt {
        line: usize,
        detail: String,
    },
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::Missing => f.write_str("journal-missing"),
            JournalError::Changed => {
                f.write_str("checkpoint-mismatch: journal changed after preparation")
            }
            JournalError::Io(e) => write!(f, "spend journal: {e}"),
            JournalError::Locked(p) => write!(
                f,
                "spend journal: {} exists; another process holds the journal, or one ended \
                 without releasing it (remove it only after checking)",
                p.display()
            ),
            JournalError::Mismatch(e) => write!(f, "spend journal: {e}"),
            JournalError::Corrupt { line, detail } => {
                write!(f, "spend journal: line {line}: {detail}")
            }
        }
    }
}

impl std::error::Error for JournalError {}

/// Why a reservation was refused. Nothing may be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpendRefusal {
    /// The live run has no room for this reservation.
    RunCap,
    /// The reservation would pass the cap.
    OverCap {
        budget: BudgetKind,
        period: String,
        cap: u64,
        committed: u64,
        requested: u64,
    },
    /// The reservation could not be made durable.
    Journal(String),
}

impl fmt::Display for SpendRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpendRefusal::RunCap => f.write_str("run-cap-exceeded"),
            SpendRefusal::OverCap {
                budget,
                period,
                cap,
                committed,
                requested,
            } => write!(
                f,
                "spend cap: {budget:?} {period} has {committed} of {cap} units committed; \
                 {requested} more would pass it; not sent"
            ),
            SpendRefusal::Journal(e) => write!(f, "spend journal unavailable ({e}); not sent"),
        }
    }
}

struct State {
    file: File,
    next_seq: u64,
    res: BTreeMap<u64, Res>,
}

struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Inner {
    identity: JournalIdentity,
    header_digest: rustev_contract::ids::ContentDigest,
    path: PathBuf,
    kind: BudgetKind,
    clock: Arc<dyn SpendClock>,
    state: Mutex<State>,
    write_failures: AtomicU64,
    // Dropped last: the lock outlives the file handle.
    _lock: Lock,
}

/// A persistent spend journal. Clones share it.
#[derive(Clone)]
pub struct SpendJournal {
    inner: Arc<Inner>,
}

impl fmt::Debug for SpendJournal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpendJournal")
            .field("path", &self.inner.path)
            .field("budget", &self.inner.kind)
            .finish()
    }
}

fn io(e: std::io::Error) -> JournalError {
    JournalError::Io(e.to_string())
}

fn lock_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

fn append(file: &mut File, line: &[u8]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line);
    buf.push(b'\n');
    #[cfg(feature = "fault-injection")]
    let entry: serde_json::Value = serde_json::from_slice(line).expect("journal entry");
    #[cfg(feature = "fault-injection")]
    let name = entry["op"].as_str().unwrap_or("header");
    #[cfg(feature = "fault-injection")]
    crate::fault_injection::point("journal", name, "before-write")?;
    file.write_all(&buf)?;
    #[cfg(feature = "fault-injection")]
    crate::fault_injection::point("journal", name, "before-sync")?;
    file.sync_data()?;
    #[cfg(feature = "fault-injection")]
    crate::fault_injection::point("journal", name, "after-sync")?;
    Ok(())
}

impl SpendJournal {
    /// Open the journal at `path`, creating it with a header for `kind` if
    /// it does not exist. Migrates a testing header that names the
    /// superseded cap (see the module documentation). Refuses a journal of
    /// another budget, cap or schema, a journal another writer holds, and
    /// complete lines that do not parse. Standalone callers use the creation
    /// name `standalone`; live runs use `PreparedRun::open_journal` to bind
    /// their own run id before opening.
    pub fn open(
        path: impl AsRef<Path>,
        kind: BudgetKind,
        clock: Arc<dyn SpendClock>,
    ) -> Result<Self, JournalError> {
        Self::open_for_run(path, kind, clock, "standalone")
    }

    /// Open a journal with the creating run's identity and injected clock.
    pub fn open_for_run(
        path: impl AsRef<Path>,
        kind: BudgetKind,
        clock: Arc<dyn SpendClock>,
        run_id: &str,
    ) -> Result<Self, JournalError> {
        JournalOpening::prepare(path, kind, clock, run_id, false)?.open()
    }

    fn open_prepared(opening: &JournalOpening) -> Result<Self, JournalError> {
        let path = opening.path.clone();
        let kind = opening.kind;
        let clock = opening.clock.clone();
        let lp = lock_path(&path);
        let mut lf = match OpenOptions::new().write(true).create_new(true).open(&lp) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(JournalError::Locked(lp));
            }
            Err(e) => return Err(io(e)),
        };
        let lock = Lock(lp);
        let _ = writeln!(lf, "{}", std::process::id());
        let _ = lf.sync_all();

        if read_header(&path)? != opening.original {
            return Err(JournalError::Changed);
        }
        let created = opening.original.is_none();
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create_new(created)
            .open(&path)
            .map_err(io)?;
        let header = Header {
            cap: kind.cap(),
            ..opening.header.clone()
        };
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(io)?;
        // An empty file can remain after interruption before the first header
        // write. It has no established identity or spend to preserve.
        if text.is_empty() {
            let line = serde_json::to_vec(&header).map_err(|e| JournalError::Io(e.to_string()))?;
            append(&mut file, &line).map_err(io)?;
            file.sync_all().map_err(io)?;
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
                && let Ok(d) = File::open(dir)
            {
                let _ = d.sync_all();
            }
            text = String::from_utf8(line).unwrap_or_default() + "\n";
        }
        // A final line without its newline was never completed: cut it.
        if !text.ends_with('\n') {
            let keep = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
            file.set_len(keep as u64).map_err(io)?;
            file.sync_all().map_err(io)?;
            text.truncate(keep);
        }
        file.seek(SeekFrom::End(0)).map_err(io)?;
        let first = text.lines().next().unwrap_or_default();
        let found = Header::parse(format!("{first}\n").as_bytes(), kind)?;
        let migrate = found.cap != kind.cap();
        let mut res = BTreeMap::new();
        let mut next_seq = 1;
        for (i, l) in text.lines().enumerate().skip(1) {
            let corrupt = |detail: String| JournalError::Corrupt {
                line: i + 1,
                detail,
            };
            let e: Entry = serde_json::from_str(l).map_err(|e| corrupt(e.to_string()))?;
            match e {
                Entry::Reserve {
                    seq,
                    period,
                    attempt,
                    units,
                    ..
                } => {
                    if res.contains_key(&seq) {
                        return Err(corrupt(format!("reservation {seq} made twice")));
                    }
                    next_seq = next_seq.max(seq + 1);
                    res.insert(
                        seq,
                        Res {
                            attempt,
                            period,
                            units,
                            state: ResState::Outstanding,
                        },
                    );
                }
                Entry::Settle { seq, charge } => {
                    let r = res
                        .get_mut(&seq)
                        .ok_or_else(|| corrupt(format!("no reservation {seq}")))?;
                    r.state = settled(charge);
                }
                Entry::Reconcile { seq, units } => {
                    let r = res
                        .get_mut(&seq)
                        .ok_or_else(|| corrupt(format!("no reservation {seq}")))?;
                    r.state = ResState::Observed(units);
                }
                Entry::Release { seq } => {
                    let r = res
                        .get_mut(&seq)
                        .ok_or_else(|| corrupt(format!("no reservation {seq}")))?;
                    r.state = ResState::Released;
                }
            }
        }
        // Every line replayed; only now is the superseded header replaced,
        // so a journal refused for a later line is left as it was.
        if migrate {
            let first_len = text.find('\n').map(|i| i + 1).unwrap_or(text.len());
            let mut bytes = opening.current_header_bytes();
            bytes.extend_from_slice(&text.as_bytes()[first_len..]);
            // Only the old file's handle is closed here: the writer lock is
            // `<journal>.lock`, held by `lock` until this journal drops, so
            // no other process can open the journal across the rename.
            drop(file);
            file = migrate_header(&path, &bytes).map_err(io)?;
        }
        // Left unsettled by a process that ended: the request may have been
        // sent, so it is liability until reconciled.
        for r in res.values_mut() {
            if r.state == ResState::Outstanding {
                r.state = ResState::Liability;
            }
        }
        Ok(SpendJournal {
            inner: Arc::new(Inner {
                identity: opening.identity(),
                header_digest: opening.header_digest(),
                path,
                kind,
                clock,
                state: Mutex::new(State {
                    file,
                    next_seq,
                    res,
                }),
                write_failures: AtomicU64::new(0),
                _lock: lock,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn identity(&self) -> &JournalIdentity {
        &self.inner.identity
    }

    pub fn header_digest(&self) -> &rustev_contract::ids::ContentDigest {
        &self.inner.header_digest
    }

    pub fn budget(&self) -> BudgetKind {
        self.inner.kind
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Lines that could not be written after a reservation (settlements,
    /// reconciliations, releases). Each leaves the journal counting more,
    /// never less.
    pub fn write_failures(&self) -> u64 {
        self.inner.write_failures.load(Ordering::SeqCst)
    }

    /// Reserve `units` for `attempt_id` in the current period, durably, or
    /// refuse. Refused means: do not send.
    pub fn reserve(&self, attempt_id: &str, units: u64) -> Result<Reservation, SpendRefusal> {
        self.reserve_checked(attempt_id, units, None)
    }

    pub(crate) fn reserve_checked(
        &self,
        attempt_id: &str,
        units: u64,
        run: Option<(&str, u64)>,
    ) -> Result<Reservation, SpendRefusal> {
        let now = self.inner.clock.now_ms();
        let kind = self.inner.kind;
        let period = kind.period(now);
        let mut s = self.lock();
        if let Some((run_id, cap)) = run {
            let committed = run_total(&s.res, run_id);
            if committed > cap || units > cap.saturating_sub(committed) {
                return Err(SpendRefusal::RunCap);
            }
        }
        let t = totals(&s.res, kind, &period);
        let committed = t.committed();
        if committed.saturating_add(units) > kind.cap() {
            return Err(SpendRefusal::OverCap {
                budget: kind,
                period,
                cap: kind.cap(),
                committed,
                requested: units,
            });
        }
        let seq = s.next_seq;
        let line = serde_json::to_vec(&Entry::Reserve {
            seq,
            period: period.clone(),
            attempt: attempt_id.to_string(),
            units,
            at_ms: now,
        })
        .map_err(|e| SpendRefusal::Journal(e.to_string()))?;
        append(&mut s.file, &line).map_err(|e| SpendRefusal::Journal(e.to_string()))?;
        s.next_seq += 1;
        s.res.insert(
            seq,
            Res {
                attempt: attempt_id.to_string(),
                period,
                units,
                state: ResState::Outstanding,
            },
        );
        Ok(Reservation { seq, units })
    }

    fn write(&self, s: &mut State, e: &Entry) -> bool {
        let ok = serde_json::to_vec(e)
            .ok()
            .is_some_and(|line| append(&mut s.file, &line).is_ok());
        if !ok {
            self.inner.write_failures.fetch_add(1, Ordering::SeqCst);
        }
        ok
    }

    /// Settle a reservation with the attempt's charge: observed and
    /// estimated charges replace it; `unknown` keeps it as liability.
    pub fn settle(&self, r: Reservation, charge: Charge) {
        let mut s = self.lock();
        let Some(state) = s.res.get(&r.seq).map(|x| x.state) else {
            return;
        };
        if state != ResState::Outstanding {
            return;
        }
        if !self.write(&mut s, &Entry::Settle { seq: r.seq, charge }) {
            return;
        }
        if let Some(x) = s.res.get_mut(&r.seq) {
            x.state = settled(charge);
        }
    }

    /// Release a reservation whose request was never sent.
    pub fn release(&self, r: Reservation) {
        let mut s = self.lock();
        if s.res.get(&r.seq).map(|x| x.state) != Some(ResState::Outstanding) {
            return;
        }
        if !self.write(&mut s, &Entry::Release { seq: r.seq }) {
            return;
        }
        if let Some(x) = s.res.get_mut(&r.seq) {
            x.state = ResState::Released;
        }
    }

    /// A later observed charge for a reservation held as liability (spec
    /// 003, 3.5.5). False when it holds none.
    pub fn reconcile(&self, seq: u64, observed_units: u64) -> bool {
        // Production liability needs a provider billing record (spec 022).
        if self.budget() == BudgetKind::Production {
            return false;
        }
        let mut s = self.lock();
        if s.res
            .get(&seq)
            .map(|x| x.state)
            .is_none_or(|s| !matches!(s, ResState::Liability | ResState::Unknown))
        {
            return false;
        }
        if !self.write(
            &mut s,
            &Entry::Reconcile {
                seq,
                units: observed_units,
            },
        ) {
            return false;
        }
        if let Some(x) = s.res.get_mut(&seq) {
            x.state = ResState::Observed(observed_units);
        }
        true
    }

    /// Spec 022: settle production liability only from a provider billing
    /// line for the journal's credential and the reservation's own month.
    /// The host must retain the billing line as evidence; no credential
    /// value is accepted or recorded here.
    pub fn reconcile_billing(
        &self,
        journal_credential_reference: &str,
        billing: &BillingSettlement,
    ) -> Result<(), Unresolved> {
        if self.budget() != BudgetKind::Production
            || journal_credential_reference.trim().is_empty()
            || journal_credential_reference != billing.credential_reference
            || billing.provider_record.trim().is_empty()
        {
            return Err(Unresolved(
                "billing record does not identify this production journal",
            ));
        }
        let mut state = self.lock();
        let reservation = state
            .res
            .get(&billing.reservation)
            .ok_or(Unresolved("billing reservation is missing"))?;
        if reservation.period != billing.period
            || !matches!(reservation.state, ResState::Liability | ResState::Unknown)
        {
            return Err(Unresolved(
                "billing month conflicts or reservation is not liability",
            ));
        }
        let line = serde_json::to_vec(&Entry::Reconcile {
            seq: billing.reservation,
            units: billing.observed_units,
        })
        .map_err(|_| Unresolved("billing settlement could not be serialized"))?;
        if append(&mut state.file, &line).is_err() {
            self.inner.write_failures.fetch_add(1, Ordering::SeqCst);
            return Err(Unresolved("billing settlement could not be recorded"));
        }
        state
            .res
            .get_mut(&billing.reservation)
            .expect("checked reservation")
            .state = ResState::Observed(billing.observed_units);
        Ok(())
    }

    /// The totals of the current period.
    pub fn summary(&self) -> SpendSummary {
        let kind = self.inner.kind;
        let period = kind.period(self.inner.clock.now_ms());
        totals(&self.lock().res, kind, &period)
    }

    /// Committed units for attempt ids starting with `live/<run_id>/`.
    /// Counts observed, estimated, liability and outstanding amounts across
    /// periods; released reservations contribute nothing. Replayed reservation
    /// state at open preserves this total across a restart.
    pub fn run_spend(&self, run_id: &str) -> u64 {
        run_total(&self.lock().res, run_id)
    }

    /// Runtime attempt ids in reservation sequence order, including releases.
    pub fn reservations_for(&self, decision_id: &str) -> Vec<String> {
        let prefix = format!(
            "{decision_id}/{}",
            rustev_contract::live::attempt_prefix(decision_id)
        );
        self.lock()
            .res
            .values()
            .filter(|r| r.attempt.starts_with(&prefix))
            .map(|r| r.attempt[decision_id.len() + 1..].to_string())
            .collect()
    }

    /// Full journal attempt keys with this prefix, including released and
    /// crash-liability reservations, in reservation sequence order.
    pub(crate) fn attempts_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.lock()
            .res
            .values()
            .filter(|r| r.attempt.starts_with(prefix))
            .map(|r| r.attempt.clone())
            .collect()
    }

    /// Reservation evidence for exactly this decision, in journal sequence order.
    /// Unsettled and crash liability remain unknown, never an inferred charge.
    pub fn reservations_detail(&self, decision_id: &str) -> Vec<crate::documents::CaseSpend> {
        use crate::documents::{CaseSettlement, CaseSpend};
        let prefix = format!(
            "{decision_id}/{}",
            rustev_contract::live::attempt_prefix(decision_id)
        );
        self.lock()
            .res
            .iter()
            .filter(|(_, r)| r.attempt.starts_with(&prefix))
            .map(|(seq, r)| CaseSpend::Reserved {
                attempt_id: r.attempt[decision_id.len() + 1..].to_string(),
                seq: *seq,
                reserved_units: r.units,
                settlement: match r.state {
                    ResState::Observed(units) => CaseSettlement::Observed { units },
                    ResState::Estimated(units) => CaseSettlement::Estimated { units },
                    ResState::Released => CaseSettlement::Released,
                    ResState::Unknown | ResState::Outstanding | ResState::Liability => {
                        CaseSettlement::Unknown
                    }
                },
            })
            .collect()
    }

    pub fn run_split(&self, run_id: &str) -> RunSpend {
        let prefix = format!("live/{run_id}/");
        let mut split = RunSpend::default();
        for r in self
            .lock()
            .res
            .values()
            .filter(|r| r.attempt.starts_with(&prefix))
        {
            let (total, units) = match r.state {
                ResState::Observed(u) => (&mut split.observed, u),
                ResState::Estimated(u) => (&mut split.estimated, u),
                ResState::Unknown => (&mut split.unknown, r.units),
                ResState::Outstanding | ResState::Liability => (&mut split.liability, r.units),
                ResState::Released => continue,
            };
            *total = total.saturating_add(units);
        }
        split
    }

    /// Schema-tagged digest of the serialized testing header, including newline.
    pub fn testing_header_digest() -> rustev_contract::ids::ContentDigest {
        header_digest(TESTING_CAP_UNITS)
    }

    /// [`Self::testing_header_digest`] of a testing journal under the
    /// superseded cap, before its header was migrated (A-75).
    pub fn superseded_testing_header_digest() -> rustev_contract::ids::ContentDigest {
        header_digest(SUPERSEDED_TESTING_CAP_UNITS)
    }

    /// Units still available in the current period.
    pub fn headroom(&self) -> u64 {
        self.summary().headroom
    }

    /// The cost policy for a runtime's shared ledger holding the current
    /// headroom: estimates are reserved, so an attempt that does not fit is
    /// recorded `budget_exhausted{cost}` by the runtime before it reaches
    /// the adapter (spec 003, 3.5.3). The journal's own check stays as the
    /// backstop.
    pub fn runtime_cost_policy(&self) -> CostPolicy {
        CostPolicy::Estimated {
            max_units: self.headroom(),
        }
    }

    /// A runtime shared ledger seeded with the current headroom, for
    /// `RuntimeBuilder::shared_ledger`.
    pub fn shared_runtime_ledger(&self) -> Arc<Ledger> {
        Arc::new(Ledger::new(self.runtime_cost_policy()))
    }
}

fn header_digest(cap: u64) -> rustev_contract::ids::ContentDigest {
    let header = Header {
        schema: JOURNAL_SCHEMA.into(),
        budget: BudgetKind::Testing,
        cap,
        journal_id: None,
        created_at_ms: None,
    };
    let mut bytes = serde_json::to_vec(&header).expect("header serializes");
    bytes.push(b'\n');
    rustev_contract::ids::ContentDigest::parse(&rustev_contract::canonical::tagged_digest(
        JOURNAL_SCHEMA,
        &bytes,
    ))
    .expect("computed digest")
}

/// Replace the journal at `path` with `bytes` atomically: write a sibling
/// file, `fsync` it, rename it over the journal (through a symbolic link,
/// to its target) and `fsync` the directory. The caller holds
/// `<journal>.lock`, which is a separate file and survives the rename.
/// Returns the new file opened for reading and appending.
fn migrate_header(path: &Path, bytes: &[u8]) -> std::io::Result<File> {
    let target = std::fs::canonicalize(path)?;
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".migrate");
    let tmp = PathBuf::from(tmp);
    let permissions = std::fs::metadata(&target)?.permissions();
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        f.set_permissions(permissions)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &target)?;
    if let Some(dir) = target.parent()
        && let Ok(d) = File::open(dir)
    {
        d.sync_all()?;
    }
    let mut file = OpenOptions::new().read(true).append(true).open(&target)?;
    file.seek(SeekFrom::End(0))?;
    Ok(file)
}

fn run_total(res: &BTreeMap<u64, Res>, run_id: &str) -> u64 {
    let prefix = format!("live/{run_id}/");
    res.values()
        .filter(|r| r.attempt.starts_with(&prefix))
        .fold(0_u64, |total, r| {
            let units = match r.state {
                ResState::Observed(u) | ResState::Estimated(u) => u,
                ResState::Outstanding | ResState::Liability | ResState::Unknown => r.units,
                ResState::Released => 0,
            };
            total.saturating_add(units)
        })
}

fn settled(charge: Charge) -> ResState {
    match charge {
        Charge::Observed { units } => ResState::Observed(units),
        Charge::Estimated { units } => ResState::Estimated(units),
        Charge::Unknown => ResState::Unknown,
    }
}

fn totals(res: &BTreeMap<u64, Res>, kind: BudgetKind, period: &str) -> SpendSummary {
    let mut t = SpendSummary {
        budget: kind,
        period: period.to_string(),
        cap: kind.cap(),
        observed: 0,
        estimated: 0,
        liability: 0,
        outstanding: 0,
        headroom: 0,
    };
    for r in res.values().filter(|r| r.period == period) {
        match r.state {
            ResState::Outstanding => t.outstanding = t.outstanding.saturating_add(r.units),
            ResState::Observed(u) => t.observed = t.observed.saturating_add(u),
            ResState::Estimated(u) => t.estimated = t.estimated.saturating_add(u),
            ResState::Liability | ResState::Unknown => {
                t.liability = t.liability.saturating_add(r.units)
            }
            ResState::Released => {}
        }
    }
    t.headroom = t.cap.saturating_sub(t.committed());
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_scanner_preserves_other_bytes_and_falls_back_for_escaped_keys() {
        for input in [
            "{ \"budget\": \"testing\", \"cap\" : 5000000000, \"schema\": \"rustev-jev.spend-journal/1\" }\n",
            r#"{"schema":"text \"cap\":5000000000 and \\","budget":"testing","cap":5000000000}"#,
            r#"{"schema":"rustev-jev.spend-journal/1","journal_id":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","created_at_ms":42,"cap":5000000000,"budget":"testing"}"#,
        ] {
            // These fixtures also exercise strings containing escaped quotes
            // and backslashes without treating their contents as keys.
            let _: Header = serde_json::from_str(input).unwrap();
            let at = input.rfind("5000000000").unwrap();
            let mut expected = input.to_owned();
            expected.replace_range(at..at + 10, "10000000000");
            assert_eq!(
                with_cap(input.as_bytes(), 10_000_000_000),
                expected.as_bytes()
            );
        }
        let escaped = br#"{ "schema":"rustev-jev.spend-journal/1", "budget":"testing", "c\u0061p":5000000000 }"#;
        let mut header: Header = serde_json::from_slice(escaped).unwrap();
        header.cap = 10_000_000_000;
        assert_eq!(with_cap(escaped, header.cap), header.bytes());
    }

    #[test]
    fn failed_settlement_and_release_keep_the_reservation() {
        let dir = std::env::temp_dir().join(format!(
            "rustev-journal-write-failure-{}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("journal");
        let journal =
            SpendJournal::open(&path, BudgetKind::Testing, Arc::new(SetClock::new(0))).unwrap();
        let ticket = journal.reserve("live/run/attempt", 10).unwrap();
        // A read-only handle deterministically makes append fail.
        journal.lock().file = File::open(&path).unwrap();
        journal.settle(ticket, Charge::Observed { units: 1 });
        assert_eq!(journal.run_spend("run"), 10);
        journal.release(ticket);
        assert_eq!(journal.run_spend("run"), 10);
        assert_eq!(journal.write_failures(), 2);
        drop(journal);
        let reopened =
            SpendJournal::open(&path, BudgetKind::Testing, Arc::new(SetClock::new(0))).unwrap();
        assert_eq!(reopened.summary().liability, 10);
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn utc_months() {
        assert_eq!(utc_month(0), "1970-01");
        // 2026-09-30T23:59:59.999Z and one millisecond later.
        assert_eq!(utc_month(1_790_812_799_999), "2026-09");
        assert_eq!(utc_month(1_790_812_800_000), "2026-10");
        assert_eq!(utc_month(951_782_400_000), "2000-02"); // 2000-02-29
        assert_eq!(utc_month(-1), "1969-12");
    }
}

// Shared by the provider integrations without depending on any of them.
/// Why a production fact, binding or billing settlement is unresolved
/// (spec 022); `production` re-exports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved(pub &'static str);
impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unresolved: {}", self.0)
    }
}
impl std::error::Error for Unresolved {}

/// A provider billing line settling one previously unknown attempt. This
/// is supplied by the host from the provider record, never inferred from a
/// timeout, a missing charge, or a late adapter response.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingSettlement {
    pub credential_reference: String,
    pub provider_record: String,
    pub period: String,
    pub reservation: u64,
    pub observed_units: u64,
}
