//! Spec 004, 3.1.3 to 3.1.5 and the acceptance isolation matrix:
//! tenant-only scope permits otherwise identical requests from different
//! principals within one tenant and context revision, and refuses another
//! tenant or revision; principal scope also refuses another principal
//! scope. Both directions of mode mismatch, absent modes or handles,
//! mixed-variant fields and a relabeled (downgraded) capture are refused
//! before resolution. SYNTHETIC fixtures only (R-04).

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::replay::{ItemLocation, ReplayBundle};
use rustev_contract::scope::Scope;
use rustev_eval::assemble::Content;
use rustev_eval::replay::{Incomparable, Inconsistency, reproduce};

async fn captured_as(principal_handle: &[u8], s: Scope) -> Retained {
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    let mut req = decision("i-1", support_snapshot("pro", &[2, 5], 0));
    req.principal_handle = principal_handle.to_vec();
    run_and_capture(&[&p], f, req, s).await
}

fn ids(r: &Retained) -> Vec<rustev_contract::ids::RequestId> {
    r.capture
        .supplies
        .iter()
        .map(|s| s.request.clone())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn tenant_only_reuses_across_principals_within_tenant_and_revision() {
    let s = tenant("tenant-a", "rev-1");
    let alice = captured_as(b"alice", s.clone()).await;
    let bob = captured_as(b"bob", s.clone()).await;
    assert_eq!(ids(&alice), ids(&bob), "one identity across principals");
    // One reader configuration replays both.
    for r in [&alice, &bob] {
        reproduced(reproduce(
            &embedded(r),
            &rustev_eval::resolve::NoExternal,
            &config_for(&s),
        ));
    }
    // Never across tenants or revisions.
    let b = embedded(&alice);
    for other in [tenant("tenant-b", "rev-1"), tenant("tenant-a", "rev-2")] {
        assert_eq!(
            incomparable(reproduce(
                &b,
                &rustev_eval::resolve::NoExternal,
                &config_for(&other)
            )),
            Incomparable::ScopeMismatch
        );
    }
    // Another tenant's identical request has another identity.
    let other = captured_as(b"alice", tenant("tenant-b", "rev-1")).await;
    assert_ne!(ids(&alice), ids(&other));
    let newer = captured_as(b"alice", tenant("tenant-a", "rev-2")).await;
    assert_ne!(ids(&alice), ids(&newer));
}

#[tokio::test(flavor = "current_thread")]
async fn principal_scope_also_isolates_principal_scopes() {
    let p1 = captured_as(b"alice", principal("tenant-a", "rev-1", "scope-1")).await;
    let p2 = captured_as(b"bob", principal("tenant-a", "rev-1", "scope-2")).await;
    assert_ne!(ids(&p1), ids(&p2));
    let b = embedded(&p1);
    reproduced(replay(&b));
    assert_eq!(
        incomparable(reproduce(
            &b,
            &rustev_eval::resolve::NoExternal,
            &config_for(&principal("tenant-a", "rev-1", "scope-2"))
        )),
        Incomparable::ScopeMismatch
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_mode_mismatch_is_refused_in_both_directions_before_resolution() {
    let t = captured_as(b"alice", tenant("tenant-a", "rev-1")).await;
    let p = captured_as(b"alice", principal("tenant-a", "rev-1", "scope-1")).await;
    assert_ne!(ids(&t), ids(&p), "modes never share identities");
    let refs = |loc: ItemLocation, _: &[u8]| format!("ref:{loc}");
    for (r, reader) in [
        (&t, principal("tenant-a", "rev-1", "scope-1")),
        (&p, tenant("tenant-a", "rev-1")),
    ] {
        let b = bundle_with(r, Content::External(&refs));
        let store = Store::default();
        assert_eq!(
            incomparable(reproduce(&b, &store, &config_for(&reader))),
            Incomparable::ScopeMismatch
        );
        assert!(store.asked.lock().unwrap().is_empty(), "nothing resolved");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_relabeled_capture_cannot_be_downgraded_or_upgraded() {
    // Relabel a principal-scoped bundle as tenant-only: its entries were
    // identified under principal scope, so reproduction refuses them.
    let p = captured_as(b"alice", principal("tenant-a", "rev-1", "scope-1")).await;
    let mut b = embedded(&p);
    b.scope = tenant("tenant-a", "rev-1");
    assert_eq!(
        incomparable(reproduce(
            &b,
            &rustev_eval::resolve::NoExternal,
            &config_for(&b.scope)
        )),
        Incomparable::Inconsistent {
            location: ItemLocation::Supply(0),
            what: Inconsistency::RequestIdentity,
        }
    );
    // And the reverse.
    let t = captured_as(b"alice", tenant("tenant-a", "rev-1")).await;
    let mut b = embedded(&t);
    b.scope = principal("tenant-a", "rev-1", "scope-1");
    assert!(matches!(
        incomparable(reproduce(
            &b,
            &rustev_eval::resolve::NoExternal,
            &config_for(&b.scope)
        )),
        Incomparable::Inconsistent {
            what: Inconsistency::RequestIdentity,
            ..
        }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn absent_or_mixed_scope_fields_never_parse() {
    let t = captured_as(b"alice", principal("tenant-a", "rev-1", "scope-1")).await;
    let text = String::from_utf8(embedded(&t).record_canonical().unwrap()).unwrap();
    let scope_json = r#""scope":{"context_revision":"rev-1","mode":"principal","principal_scope":"scope-1","tenant":"tenant-a"}"#;
    assert!(text.contains(scope_json), "canonical scope layout");
    for bad in [
        // No mode.
        r#""scope":{"context_revision":"rev-1","principal_scope":"scope-1","tenant":"tenant-a"}"#,
        // No principal handle in principal mode: never a downgrade.
        r#""scope":{"context_revision":"rev-1","mode":"principal","tenant":"tenant-a"}"#,
        // Mixed variants.
        r#""scope":{"context_revision":"rev-1","mode":"tenant_only","principal_scope":"scope-1","tenant":"tenant-a"}"#,
        // Empty handle.
        r#""scope":{"context_revision":"","mode":"principal","principal_scope":"scope-1","tenant":"tenant-a"}"#,
        // No scope at all.
        "",
    ] {
        let replaced = if bad.is_empty() {
            text.replacen(&format!("{scope_json},"), "", 1)
        } else {
            text.replacen(scope_json, bad, 1)
        };
        assert_ne!(replaced, text);
        assert!(ReplayBundle::parse(replaced.as_bytes()).is_err(), "{bad}");
    }
    assert!(ReplayBundle::parse(text.as_bytes()).is_ok());
}
