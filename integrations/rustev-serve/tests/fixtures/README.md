These are public, synthetic test fixtures, never deployment credentials.

TLS tests use `tests/support/mod.rs` and its `TestPki` helper to generate a
fresh CA and localhost server and client certificates with rcgen at runtime.
PEM files live in a unique temporary directory owned by the helper and are
removed when it is dropped. No TLS private keys or certificates are committed.
The `config.json` fixture uses a Unix socket and needs no TLS paths.

The Sigstore artifact and v0.3 bundle are copied from sigstore-verify 0.15.0
`test_data/bundles/cosign-v3-blob.*`. The trusted root is copied from
sigstore-trust-root 0.15.0 `src/trusted_root.json`. Both upstream packages are
Apache-2.0 licensed. JSON is pretty-printed with non-ASCII characters escaped;
the decoded signed data is unchanged. These fixtures test offline verification
of an upstream artifact; they are not a signed Rustev plan bundle or deployment
trust configuration. No verification test fetches a root or contacts a log.


`synthetic-sigstore-register.json` is a real Rustev plan registration signed by a
local test leaf with an embedded SCT and a single-leaf transparency proof.
`synthetic-sigstore-root.json` contains its synthetic Fulcio, CT and Rekor trust.
The identity is `rustev@example.invalid` and issuer is
`https://issuer.example.invalid`. These roots are never deployment trust.
`regenerate_sigstore.py` uses local OpenSSL, discards private keys, and performs
no network calls. To replace these fixtures, run from this directory:

```
python3 regenerate_sigstore.py synthetic-sigstore-register.json .
```

This re-signs the unchanged bundle digest with newly generated test keys. If the
plan content changes, supply an unsigned registration and the exact
`PlanBundleSignature::signing_input()` bytes as the script's four-argument form.

`synthetic-sigstore-invalid-chain-root.json` keeps the same CT and Rekor keys
but replaces the Fulcio CA with another CA bearing the same subject and a
different key. The signed registration's artifact signature and log proof
remain valid, but its certificate chain cannot verify against this root.
Tests also use the valid registration with a different configured identity or
issuer to distinguish untrusted signers from invalid cryptography.
