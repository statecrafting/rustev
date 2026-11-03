# SYNTHETIC support-routing bundle

Mechanics only: these heads and the package's unfitted temperature calibration
are not evidence of routing quality, usefulness or calibration quality.

Hidden size is 8. The nine-token WordPiece vocabulary comes from the existing
`tests/bundle.rs::tokenizer_bytes` generator. Unknown words remain unknown tokens.
There is no checkpoint, network access or ONNX Runtime dependency at execution.

For each head, `synthetic::head_weights(seed, options, 8)` emits option-major
weights followed by biases as little-endian f32. Value k is
`((splitmix64(seed + k) >> 40) as f32) / 8388608 - 1`.
The unsigned 64-bit additions and multiplications wrap. SplitMix64 adds
0x9e3779b97f4a7c15, applies `(z ^ (z >> 30)) * 0xbf58476d1ce4e5b9`, then
`(z ^ (z >> 27)) * 0x94d049bb133111eb`, then `z ^ (z >> 31)`.

| Task | Operation | Options | Seed |
|---|---|---|---|
| support.topic | classify | billing, integration_defect, account_access, other | 11 |
| support.frustration | rubric | calm, frustrated, very_angry | 12 |
| support.deadline | proposition | false, true | 13 |

The generated manifest binds every payload by byte length and SHA-256. Head
questions and options come from the package's reference definition. The only
projected field is ticket.message. The graph file is a synthetic marker.
`identities.json` pins the descriptor and compiled plan with the package's
SYNTHETIC temperature 1.5 calibration rebound to this bundle's ArtifactId.
The existing rules PlanId is checked against the package's committed example
judgment, never re-blessed here.

Normal tests regenerate and compare every file byte and the exact file list;
they never rewrite fixtures. After reviewing an intentional formula change,
re-bless from the workspace root with:

```
cargo test -p rustev-backend-embed-linear --locked --test bundle support_routing::fixtures::bless_support_routing_fixtures -- --ignored --exact
```

Review the resulting diff and run the normal tests. The README itself is
emitted from `tests/support_routing/fixtures.rs::README`.
