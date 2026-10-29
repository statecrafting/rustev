//! Spec 021, 3.9, and 058 3.5.2: a crate version's public checksum is read
//! from the sparse index, without touching the network here.

use rustev_release::cratesio::{index_checksum, index_path};

#[test]
fn index_paths_follow_the_sparse_layout() {
    assert_eq!(index_path("a"), "1/a");
    assert_eq!(index_path("ab"), "2/ab");
    assert_eq!(index_path("abc"), "3/a/abc");
    assert_eq!(index_path("rustev-core"), "ru/st/rustev-core");
    assert_eq!(index_path("Serde"), "se/rd/serde");
}

#[test]
fn the_index_checksum_is_read_per_version() {
    let text = concat!(
        r#"{"name":"x","vers":"0.1.0","deps":[],"cksum":"aa","features":{},"yanked":false}"#,
        "\n",
        r#"{"name":"x","vers":"0.2.0","deps":[],"cksum":"bb","features":{},"yanked":false}"#,
        "\n"
    );
    assert_eq!(index_checksum(text, "0.2.0").as_deref(), Some("bb"));
    assert_eq!(index_checksum(text, "0.3.0"), None);
}
