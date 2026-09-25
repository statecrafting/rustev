//! The persistent spend journal that holds R-29's caps (spec 012, 3.8): a
//! shared cost ledger that outlives a run (spec 003, 3.5.2), kept by the
//! host outside any repository.
//!
//! Two budgets: `testing`, USD 5 in total (5,000,000,000 units of one
//! nano-USD, never reset), and `production`, USD 25 per UTC calendar month
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
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rustev_contract::execution::CostPolicy;
use rustev_contract::run::Charge;
use rustev_runtime::Ledger;
use serde::{Deserialize, Serialize};

pub const JOURNAL_SCHEMA: &str = "rustev-jev.spend-journal/1";
/// R-29: USD 5 in total for testing, in nano-USD.
pub const TESTING_CAP_UNITS: u64 = 5_000_000_000;
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
    Released,
}

#[derive(Debug, Clone)]
struct Res {
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

/// The journal's totals for one period, in units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    file.write_all(&buf)?;
    file.sync_data()
}

impl SpendJournal {
    /// Open the journal at `path`, creating it with a header for `kind` if
    /// it does not exist. Refuses a journal of another budget, a journal
    /// another writer holds, and complete lines that do not parse.
    pub fn open(
        path: impl AsRef<Path>,
        kind: BudgetKind,
        clock: Arc<dyn SpendClock>,
    ) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
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

        let created = !path.exists();
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(io)?;
        let header = Header {
            schema: JOURNAL_SCHEMA.to_string(),
            budget: kind,
            cap: kind.cap(),
        };
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(io)?;
        if created || text.is_empty() {
            let line = serde_json::to_vec(&header).map_err(|e| JournalError::Io(e.to_string()))?;
            append(&mut file, &line).map_err(io)?;
            file.sync_all().map_err(io)?;
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                if let Ok(d) = File::open(dir) {
                    let _ = d.sync_all();
                }
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
        let mut lines = text.lines().enumerate();
        let first = lines.next().map(|(_, l)| l).unwrap_or_default();
        let found: Header = serde_json::from_str(first).map_err(|e| JournalError::Corrupt {
            line: 1,
            detail: format!("header: {e}"),
        })?;
        if found != header {
            return Err(JournalError::Mismatch(format!(
                "the journal holds {:?} with cap {} under {}, not {:?} with cap {}",
                found.budget,
                found.cap,
                found.schema,
                kind,
                kind.cap()
            )));
        }
        let mut res = BTreeMap::new();
        let mut next_seq = 1;
        for (i, l) in lines {
            let corrupt = |detail: String| JournalError::Corrupt {
                line: i + 1,
                detail,
            };
            let e: Entry = serde_json::from_str(l).map_err(|e| corrupt(e.to_string()))?;
            match e {
                Entry::Reserve {
                    seq, period, units, ..
                } => {
                    if res.contains_key(&seq) {
                        return Err(corrupt(format!("reservation {seq} made twice")));
                    }
                    next_seq = next_seq.max(seq + 1);
                    res.insert(
                        seq,
                        Res {
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
        // Left unsettled by a process that ended: the request may have been
        // sent, so it is liability until reconciled.
        for r in res.values_mut() {
            if r.state == ResState::Outstanding {
                r.state = ResState::Liability;
            }
        }
        Ok(SpendJournal {
            inner: Arc::new(Inner {
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
        let now = self.inner.clock.now_ms();
        let kind = self.inner.kind;
        let period = kind.period(now);
        let mut s = self.lock();
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
                period,
                units,
                state: ResState::Outstanding,
            },
        );
        Ok(Reservation { seq, units })
    }

    fn write(&self, s: &mut State, e: &Entry) {
        let ok = serde_json::to_vec(e)
            .ok()
            .is_some_and(|line| append(&mut s.file, &line).is_ok());
        if !ok {
            self.inner.write_failures.fetch_add(1, Ordering::SeqCst);
        }
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
        self.write(&mut s, &Entry::Settle { seq: r.seq, charge });
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
        self.write(&mut s, &Entry::Release { seq: r.seq });
        if let Some(x) = s.res.get_mut(&r.seq) {
            x.state = ResState::Released;
        }
    }

    /// A later observed charge for a reservation held as liability (spec
    /// 003, 3.5.5). False when it holds none.
    pub fn reconcile(&self, seq: u64, observed_units: u64) -> bool {
        let mut s = self.lock();
        if s.res.get(&seq).map(|x| x.state) != Some(ResState::Liability) {
            return false;
        }
        self.write(
            &mut s,
            &Entry::Reconcile {
                seq,
                units: observed_units,
            },
        );
        if let Some(x) = s.res.get_mut(&seq) {
            x.state = ResState::Observed(observed_units);
        }
        true
    }

    /// The totals of the current period.
    pub fn summary(&self) -> SpendSummary {
        let kind = self.inner.kind;
        let period = kind.period(self.inner.clock.now_ms());
        totals(&self.lock().res, kind, &period)
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

fn settled(charge: Charge) -> ResState {
    match charge {
        Charge::Observed { units } => ResState::Observed(units),
        Charge::Estimated { units } => ResState::Estimated(units),
        Charge::Unknown => ResState::Liability,
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
            ResState::Liability => t.liability = t.liability.saturating_add(r.units),
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
    fn utc_months() {
        assert_eq!(utc_month(0), "1970-01");
        // 2026-09-30T23:59:59.999Z and one millisecond later.
        assert_eq!(utc_month(1_790_812_799_999), "2026-09");
        assert_eq!(utc_month(1_790_812_800_000), "2026-10");
        assert_eq!(utc_month(951_782_400_000), "2000-02"); // 2000-02-29
        assert_eq!(utc_month(-1), "1969-12");
    }
}
