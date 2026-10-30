//! Deterministic generated spend histories, SYNTHETIC only.
mod support;

use std::sync::Mutex;

use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_live::{JournalGate, journal::SpendJournal};
use rustev_runtime::{
    LiveRequest, LiveSpend, SpendAttempt, SpendBinding, SpendGate, SpendRefusal, SpendTicket,
    SpendUnit,
};
use support::{Dir, Host, common::*, spec};

struct Rng(u64);
impl Rng {
    fn pick(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

// Observe the real runtime gate calls, including retries. Refusal bytes are
// compared at the call itself, before another attempt can change the journal.
struct AuditGate {
    inner: JournalGate,
    journal: SpendJournal,
    cap: u64,
    excess: Mutex<u64>,
    accepted: Mutex<Vec<String>>,
    refused: Mutex<Vec<(String, String)>>,
}
impl AuditGate {
    fn check(&self) {
        let total = self.journal.summary();
        let split = self.journal.run_split("run");
        assert_eq!(
            split.observed + split.estimated + split.liability + split.unknown,
            self.journal.run_spend("run")
        );
        // Unknown includes unknown settled charges; run_split's liability also
        // includes outstanding reservations. summary distinguishes the two.
        assert_eq!(
            total.committed(),
            total.observed + total.estimated + total.liability + total.outstanding
        );
        let excess = *self.excess.lock().unwrap();
        assert!(self.journal.run_spend("run") <= self.cap + excess);
        assert!(total.committed() <= total.cap + excess);
        assert_eq!(self.journal.write_failures(), 0);
    }
}
impl SpendGate for AuditGate {
    fn reserve(&self, a: &SpendAttempt) -> Result<SpendTicket, SpendRefusal> {
        let before = std::fs::read(self.journal.path()).unwrap();
        match self.inner.reserve(a) {
            Ok(ticket) => {
                // Admission itself must obey both caps, even after an overrun.
                assert!(self.journal.run_spend("run") <= self.cap);
                assert!(self.journal.summary().committed() <= self.journal.summary().cap);
                self.accepted.lock().unwrap().push(a.attempt_id.clone());
                let bound = match a.bound {
                    CostBound::Bounded { max_units } => max_units,
                    CostBound::Estimated { units } => units,
                    CostBound::Unknown => panic!("unknown exposure admitted"),
                };
                self.check();
                Ok(Box::new((ticket, bound)))
            }
            Err(e) => {
                assert_eq!(std::fs::read(self.journal.path()).unwrap(), before);
                self.refused
                    .lock()
                    .unwrap()
                    .push((a.attempt_id.clone(), e.code.clone()));
                self.check();
                Err(e)
            }
        }
    }
    fn settle(&self, ticket: SpendTicket, charge: &Charge) {
        let (ticket, bound) = *ticket.downcast::<(SpendTicket, u64)>().unwrap();
        if let Charge::Observed { units } | Charge::Estimated { units } = charge {
            *self.excess.lock().unwrap() += units.saturating_sub(bound);
        }
        self.inner.settle(ticket, charge);
        self.check();
    }
    fn release(&self, ticket: SpendTicket) {
        let (ticket, _) = *ticket.downcast::<(SpendTicket, u64)>().unwrap();
        self.inner.release(ticket);
        self.check();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn generated_bounds_charges_retries_caps_and_headroom() {
    let mut unknowns = 0;
    let mut retried = 0;
    let mut overruns = 0;
    let mut refusals = std::collections::BTreeSet::new();
    let plans: Vec<_> = (0..3)
        .map(|retries| {
            let policy = execution(
                CostPolicy::Unlimited,
                vec![step_exec(
                    "topic",
                    retries + 1,
                    if retries == 0 {
                        &[]
                    } else {
                        &[FailureClass::Transient]
                    },
                    Delay::None,
                    &[],
                    &[],
                )],
            );
            support_compiled(Some(&policy))
        })
        .collect();
    let s = spec();
    for seed in 1..=200 {
        let mut rng = Rng(seed);
        let units = 1 + rng.pick(10);
        let kind = rng.pick(5);
        let bound = match kind {
            0 => CostBound::Unknown,
            1 | 2 => CostBound::Estimated { units },
            _ => CostBound::Bounded { max_units: units },
        };
        let model = if kind == 1 || kind == 2 {
            CostModel::Estimated
        } else {
            CostModel::Bounded
        };
        let charge = match rng.pick(5) {
            0 => Charge::Observed {
                units: rng.pick(units),
            },
            1 => Charge::Observed { units },
            2 => Charge::Observed {
                units: units + 1 + rng.pick(10),
            },
            3 => Charge::Estimated {
                units: rng.pick(units + 1),
            },
            _ => Charge::Unknown,
        };
        let retries = rng.pick(3) as u32;
        let cap = rng.pick(90);
        let headroom = rng.pick(90);
        let dir = Dir::new();
        let j = dir.journal();
        let pre = j.summary().cap - headroom;
        let t = j.reserve("previous-run", pre).unwrap();
        j.settle(t, Charge::Observed { units: pre });
        let gate = AuditGate {
            inner: JournalGate::new(j.clone(), "run", cap).unwrap(),
            journal: j.clone(),
            cap,
            excess: Mutex::new(0),
            accepted: Mutex::new(vec![]),
            refused: Mutex::new(vec![]),
        };
        let head = Scripted::new(linear_head(), move |c| {
            if c.task() == "support.topic" && c.n <= retries {
                Answer::Fail(AdapterFailure::Transient, "SYNTHETIC retry".into(), charge)
            } else {
                Answer::Output(support_output(c.task()), charge)
            }
        })
        .with_cost(model, bound);
        let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
        let plan = r.rt.prepare(plans[retries as usize].clone()).unwrap();
        let base = request("live/run/a/1/x");
        let req = LiveRequest {
            decision_id: base.decision_id,
            snapshot: base.snapshot,
            expected_snapshot_id: None,
            evaluation_time: base.evaluation_time,
            deadline_ms: None,
            principal_handle: base.principal_handle,
            subject: s.subject.clone(),
            scope: s.scope.clone(),
            capture: None,
            historical: None,
        };
        let d = linear_head();
        let bindings = [SpendBinding {
            backend_id: d.backend_id,
            artifact: d.artifact,
            cost_unit: SpendUnit::NanoUsd,
        }];
        // Use the gated runtime entry directly: the dataset probe rejects an
        // unknown cost model earlier as spend-unit (043 3.6.3). Here a known
        // model with unknown per-attempt exposure exercises the spend gate.
        let result =
            r.rt.reexecute_live_with_gate(
                &plan,
                req,
                &Host(None),
                &CancelSignal::new(),
                LiveSpend {
                    gate: &gate,
                    bindings: &bindings,
                },
            )
            .await
            .unwrap();
        gate.check();
        let sends = head.dispatched();
        assert_eq!(
            *gate.accepted.lock().unwrap(),
            sends,
            "seed {seed}: every send reserved anew"
        );
        for (id, code) in gate.refused.lock().unwrap().iter() {
            assert!(!sends.contains(id), "seed {seed}: refused attempt sent");
            refusals.insert(code.clone());
        }
        if kind == 0 {
            unknowns += 1;
            assert!(sends.is_empty(), "seed {seed}");
            assert_eq!(result.spend_refusal.unwrap().code, "spend-exposure-unknown");
            assert_eq!(j.run_spend("run"), 0);
        }
        retried += usize::from(
            sends
                .iter()
                .any(|id| id.contains("/topic/") && id.ends_with("/2")),
        );
        overruns +=
            usize::from(j.run_spend("run") > cap || j.summary().committed() > j.summary().cap);
        if *gate.excess.lock().unwrap() == 0 {
            assert!(j.run_spend("run") <= cap, "seed {seed}");
            assert!(j.summary().committed() <= j.summary().cap, "seed {seed}");
        }
        let per_attempt = match charge {
            Charge::Observed { units } | Charge::Estimated { units } => units,
            Charge::Unknown => units,
        };
        assert_eq!(
            j.run_spend("run"),
            sends.len() as u64 * per_attempt,
            "seed {seed}: full charges retained"
        );
    }
    assert!(
        unknowns > 30 && retried > 10 && overruns > 0,
        "coverage: unknowns={unknowns}, retried={retried}, overruns={overruns}"
    );
    for code in [
        "spend-exposure-unknown",
        "run-cap-exceeded",
        "spend-cap-exceeded",
    ] {
        assert!(
            refusals.contains(code),
            "missing generated coverage: {code}"
        );
    }
}
