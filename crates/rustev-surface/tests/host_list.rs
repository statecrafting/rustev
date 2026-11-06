//! Directory prefixes have the same immediate names on both hosts.
use rustev_surface::host::{EntryKind, Host, Memory};

// The filesystem-host test in the CLI checks the same listing contract.
pub fn listing_contract(host: &dyn Host, dir: &str) {
    assert_eq!(host.list(dir, 0).unwrap(), Vec::<String>::new());
    assert_eq!(host.list(dir, 2).unwrap(), ["B", "a"]);
    assert_eq!(host.list(dir, 4).unwrap(), ["B", "a", "z", "é"]);
    assert_eq!(host.list(dir, 100).unwrap(), ["B", "a", "z", "é"]);
    assert_eq!(
        host.entry_kind(&host.join(dir, "a")).unwrap(),
        EntryKind::Directory
    );
    assert!(host.list(&host.join(dir, "missing"), 2).is_err());
}

#[test]
fn memory_lists_immediate_input_names_in_byte_order_under_the_cap() {
    let host = Memory::new(
        [
            ("in/z".into(), vec![]),
            ("in/é".into(), vec![]),
            ("in/B".into(), vec![]),
            ("in/a/one".into(), vec![]),
            ("in/a/two/deep".into(), vec![]),
            ("inside/not-an-entry".into(), vec![]),
        ]
        .into(),
    );
    listing_contract(&host, "in");
    assert_eq!(host.list("in/", 2).unwrap(), ["B", "a"]);
    host.create("in/output", b"output").unwrap();
    assert_eq!(host.list("in", 100).unwrap(), ["B", "a", "z", "é"]);
    assert!(host.ensure_absent("in/a").is_err());
}
