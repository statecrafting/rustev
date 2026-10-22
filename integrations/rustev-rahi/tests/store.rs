//! The sink against a real single-voter Rahi 0.7.1 store in a temporary
//! directory on loopback (binding record item 7). No live service: the node
//! runs in this process and is stopped at the end of each test.

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use rahi_store::{
    EncKey, EncKeys, MigrationSet, Store, StoreConfig, StoreHandle, StoreSecrets, TxnBuilder,
    Value, coordination_set, receipt_set,
};
use rahi_types::UnixSeconds;
use rustev_contract::Document;
use rustev_contract::run::RunRecord;
use rustev_core::seams::EvidenceSink;
use rustev_rahi::{
    Delivery, Durability, MAX_RECORD_BYTES, RahiEvidenceSink, RahiSinkConfig, Rejection,
    migration_set, stage_erasure,
};

fn free_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
    listener.local_addr().expect("its address")
}

fn scratch_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "rustev-rahi-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("a scratch directory");
    d
}

fn config(dir: &std::path::Path) -> StoreConfig {
    StoreConfig {
        node_id: 1,
        nodes: Vec::new(),
        data_dir: dir.join("hiqlite"),
        raft_addr: free_addr(),
        api_addr: free_addr(),
        secrets: StoreSecrets {
            secret_raft: "raft-secret-for-rustev-tests".to_owned(),
            secret_api: "api-secret-for-rustev-tests".to_owned(),
            enc_keys: EncKeys {
                active: "test".to_owned(),
                keys: vec![EncKey {
                    id: "test".to_owned(),
                    key: vec![9u8; 32],
                }],
            },
        },
        backup_keep_days: 1,
        s3: None,
    }
}

/// A single voter with every set the sink needs migrated.
async fn open() -> (Store, PathBuf) {
    open_with(&[
        coordination_set(),
        receipt_set(),
        migration_set().expect("the rustev set"),
    ])
    .await
}

/// A single voter with `sets` migrated. A port taken between allocation and
/// bind is retried in a fresh directory, a bounded number of times.
async fn open_with(sets: &[MigrationSet]) -> (Store, PathBuf) {
    let mut last = String::new();
    for _ in 0..5 {
        let dir = scratch_dir();
        match Store::open(&config(&dir)).await {
            Ok(store) => {
                store
                    .handle()
                    .migrate_sets(&[], sets)
                    .await
                    .expect("the sets migrate");
                return (store, dir);
            }
            Err(e) if e.message().contains("Address already in use") => last = e.to_string(),
            Err(e) => panic!("a single voter opens: {e}"),
        }
    }
    panic!("a single voter opens after bounded port retries: {last}")
}

fn record() -> RunRecord {
    RunRecord::parse(include_bytes!("fixtures/run.record.json").trim_ascii_end()).unwrap()
}

fn sink(store: &StoreHandle, tenant: &str) -> RahiEvidenceSink {
    RahiEvidenceSink::new(
        store.clone(),
        RahiSinkConfig {
            tenant: tenant.to_owned(),
            durability: Durability::RaftLogAsync,
            max_record_bytes: MAX_RECORD_BYTES,
            clock: Arc::new(|| Some(UnixSeconds::new(1_800_000_000))),
        },
    )
    .unwrap()
}

#[derive(serde::Deserialize)]
struct Row {
    record: String,
    digest: String,
}

async fn stored(store: &StoreHandle) -> Vec<Row> {
    store
        .query_consistent(
            "SELECT record, digest FROM rustev_run_record ORDER BY key_digest",
            Vec::new(),
        )
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_duplicate_conflict_and_erased_against_a_real_store() {
    let (store, dir) = open().await;
    let h = store.handle();
    let s = sink(&h, "tenant-a");
    let r = record();
    let digest = r.record_digest().unwrap().to_string();

    // Accepted, with the record canonical bytes stored beside the receipt.
    let receipt = match s.deliver_typed(&r).await {
        Delivery::Accepted(receipt) => receipt,
        other => panic!("{other:?}"),
    };
    assert_eq!(receipt.accepted_digest, digest);
    let external_id = receipt.external_id.clone();
    assert!(external_id.ends_with("#1"), "{external_id}");
    assert_eq!(
        receipt.text(),
        format!("rahi:raft-log-async:{external_id}:{digest}")
    );
    let rows = stored(&h).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].record.as_bytes(),
        r.record_canonical().unwrap().as_slice()
    );
    assert_eq!(rows[0].digest, digest);

    // The same record again: a duplicate, through the seam as well.
    assert!(matches!(s.deliver_typed(&r).await, Delivery::Duplicate(_)));
    assert_eq!(
        s.deliver(&r).await.unwrap(),
        format!("rahi:duplicate:{external_id}:{digest}")
    );

    // The same decision id with another record: a conflict, and the stored
    // record is untouched (section 5).
    let mut v: serde_json::Value = serde_json::from_slice(&r.record_canonical().unwrap()).unwrap();
    v["plan_id"] = serde_json::Value::String(format!("sha256:{}", "0".repeat(64)));
    let other = RunRecord::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(other.decision_id, r.decision_id);
    match s.deliver_typed(&other).await {
        Delivery::Conflict { stored } => assert_eq!(stored.as_deref(), Some(digest.as_str())),
        d => panic!("{d:?}"),
    }
    assert!(s.deliver(&other).await.is_err());
    let rows = stored(&h).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].digest, digest);

    // Another tenant is another identity.
    let t2 = sink(&h, "tenant-b");
    assert!(matches!(t2.deliver_typed(&r).await, Delivery::Accepted(_)));
    assert_eq!(stored(&h).await.len(), 2);

    // Erasure removes the record and refuses the decision id from then on.
    let mut txn = TxnBuilder::new();
    stage_erasure(
        &mut txn,
        "tenant-a",
        &r.decision_id,
        UnixSeconds::new(1_800_000_100),
    )
    .unwrap();
    h.txn(txn.into_statements()).await.unwrap();
    assert_eq!(stored(&h).await.len(), 1);
    assert_eq!(
        s.deliver_typed(&r).await,
        Delivery::Rejected(Rejection::Erased)
    );
    assert_eq!(stored(&h).await.len(), 1);

    store.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_store_without_the_rustev_set_refuses_and_accepts_nothing() {
    // A host that forgot the migration set: the read works, the transaction
    // is refused and rolled back, and the receipt is not left behind.
    let (store, dir) = open_with(&[coordination_set(), receipt_set()]).await;
    let h = store.handle();
    let s = sink(&h, "tenant-a");
    let d = s.deliver_typed(&record()).await;
    assert!(
        matches!(d, Delivery::Rejected(Rejection::Refused(_))),
        "{d:?}"
    );
    #[derive(serde::Deserialize)]
    struct Count {
        n: i64,
    }
    let heads: Vec<Count> = h
        .query_consistent(
            "SELECT count(*) AS n FROM rahi_receipt_head",
            Vec::<Value>::new(),
        )
        .await
        .unwrap();
    assert_eq!(heads[0].n, 0);
    store.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
