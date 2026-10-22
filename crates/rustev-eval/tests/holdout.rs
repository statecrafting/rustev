//! Spec 029, 3.7.3 to 3.7.5 and section 5: the held-out ledger counts by
//! lineage, plan and dataset; refuses a broken chain, another dataset, a
//! malformed entry and its bound; and never modifies its input.

use rustev_contract::Document;
use rustev_contract::ids::{ContentDigest, DatasetId, PlanId};
use rustev_contract::scope::Handle;
use rustev_eval::holdout::{
    Append, HoldoutLedger, LedgerEntry, LedgerError, MAX_ENTRIES, entry_digest,
};

fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

fn dataset(c: char) -> DatasetId {
    DatasetId::parse(&digest(c)).unwrap()
}

fn plan(c: char) -> PlanId {
    PlanId::parse(&digest(c)).unwrap()
}

fn append(lineage: &str, p: char) -> Append {
    Append {
        lineage: Handle::new(lineage).unwrap(),
        plan: plan(p),
        repeat_config: ContentDigest::parse(&digest('c')).unwrap(),
        runs: vec![ContentDigest::parse(&digest('d')).unwrap()],
        recorded_at_ms: 1_000,
    }
}

#[test]
fn appends_count_by_lineage_plan_and_dataset_and_chain_the_heads() {
    let ds = dataset('a');
    let empty = HoldoutLedger::new(ds.clone());
    let (one, r1) = empty.append(&ds, append("lineage-x", '1')).unwrap();
    assert_eq!(empty.entries.len(), 0, "the input is never modified");
    assert_eq!((r1.count, r1.plan_count, r1.dataset_count), (1, 1, 1));
    assert_eq!(r1.ledger_head_before, None);
    assert_eq!(Some(r1.ledger_head_after.clone()), one.head().unwrap());
    let before = one.clone();
    let (two, r2) = one.append(&ds, append("lineage-x", '2')).unwrap();
    assert_eq!(one, before, "the input is never modified");
    assert_eq!((r2.count, r2.plan_count, r2.dataset_count), (2, 1, 2));
    assert_eq!(r2.ledger_head_before, Some(r1.ledger_head_after.clone()));
    let (three, r3) = two.append(&ds, append("lineage-y", '2')).unwrap();
    assert_eq!((r3.count, r3.plan_count, r3.dataset_count), (1, 2, 3));
    three.verify(&ds).unwrap();
    assert_eq!(three.entries[2].prev, Some(r2.ledger_head_after));
    // The document round-trips and keeps its identity.
    let bytes = three.canonical().unwrap();
    let parsed = HoldoutLedger::parse(&bytes).unwrap();
    assert_eq!(parsed, three);
    assert_eq!(
        entry_digest(&parsed.entries[0]).unwrap(),
        r1.ledger_head_after
    );
}

#[test]
fn a_broken_chain_another_dataset_or_a_malformed_entry_is_refused() {
    let ds = dataset('a');
    let (l, _) = HoldoutLedger::new(ds.clone())
        .append(&ds, append("x", '1'))
        .unwrap();
    let (l, _) = l.append(&ds, append("x", '2')).unwrap();
    let mut broken = l.clone();
    broken.entries[1].prev = Some(ContentDigest::parse(&digest('e')).unwrap());
    assert_eq!(broken.verify(&ds), Err(LedgerError::BrokenChain(1)));
    assert!(broken.append(&ds, append("x", '3')).is_err());
    // An edited earlier entry breaks the link after it.
    let mut edited = l.clone();
    edited.entries[0].recorded_at_ms += 1;
    assert_eq!(edited.verify(&ds), Err(LedgerError::BrokenChain(1)));
    let mut first = l.clone();
    first.entries[0].prev = Some(ContentDigest::parse(&digest('e')).unwrap());
    assert_eq!(first.verify(&ds), Err(LedgerError::BrokenChain(0)));
    // A truncated ledger still verifies: the chain proves consistency, not
    // completeness (3.7.5); the heads recorded in reports expose it.
    let mut truncated = l.clone();
    truncated.entries.pop();
    truncated.verify(&ds).unwrap();
    assert_eq!(l.verify(&dataset('b')), Err(LedgerError::OtherDataset));
    assert_eq!(
        l.append(&dataset('b'), append("x", '3')).unwrap_err(),
        LedgerError::OtherDataset
    );
    let mut no_runs = append("x", '3');
    no_runs.runs.clear();
    assert_eq!(
        l.append(&ds, no_runs).unwrap_err(),
        LedgerError::MalformedEntry(2)
    );
    let mut too_many = append("x", '3');
    too_many.runs = vec![ContentDigest::parse(&digest('d')).unwrap(); 65];
    assert_eq!(
        l.append(&ds, too_many).unwrap_err(),
        LedgerError::MalformedEntry(2)
    );
    let mut empty_entry = l.clone();
    empty_entry.entries[0].runs.clear();
    assert_eq!(empty_entry.verify(&ds), Err(LedgerError::MalformedEntry(0)));
    let mut schema = l.clone();
    schema.schema = "rustev.holdout-ledger/2".into();
    assert_eq!(schema.verify(&ds), Err(LedgerError::Schema));
    // Unknown fields and an invalid lineage handle are refused on parse.
    let mut v = serde_json::to_value(&l).unwrap();
    v["entries"][0]["extra"] = serde_json::json!(1);
    assert!(HoldoutLedger::parse(v.to_string().as_bytes()).is_err());
    let mut v = serde_json::to_value(&l).unwrap();
    v["entries"][0]["lineage"] = serde_json::json!("");
    assert!(HoldoutLedger::parse(v.to_string().as_bytes()).is_err());
}

#[test]
fn the_entry_bound_is_enforced() {
    let ds = dataset('a');
    // A valid chain of exactly MAX_ENTRIES entries, built directly.
    let mut entries: Vec<LedgerEntry> = Vec::with_capacity(MAX_ENTRIES + 1);
    let mut prev = None;
    for i in 0..MAX_ENTRIES {
        let a = append("x", '1');
        let e = LedgerEntry {
            lineage: a.lineage,
            plan: a.plan,
            repeat_config: a.repeat_config,
            runs: a.runs,
            recorded_at_ms: i as i64,
            prev: prev.clone(),
        };
        prev = Some(entry_digest(&e).unwrap());
        entries.push(e);
    }
    let full = HoldoutLedger {
        schema: rustev_eval::holdout::HOLDOUT_LEDGER.into(),
        dataset: ds.clone(),
        entries,
    };
    full.verify(&ds).unwrap();
    assert_eq!(
        full.append(&ds, append("x", '1')).unwrap_err(),
        LedgerError::TooManyEntries(MAX_ENTRIES + 1)
    );
    let mut over = full.clone();
    over.entries.push(over.entries[0].clone());
    assert_eq!(
        over.verify(&ds),
        Err(LedgerError::TooManyEntries(MAX_ENTRIES + 1))
    );
}
