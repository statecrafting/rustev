# Evaluation diff

## all

| Metric | Baseline | Run | Delta | Baseline interval | Run interval | Overlap | Within noise |
| --- | --- | --- | --- | --- | --- | --- | --- |
| coverage | 0.5 | 0.5 | 0.0 | [0.25, 0.75] | unknown: not-case-decomposable | unknown: run interval: not-case-decomposable | true |

## Changed cases

Total: 1; truncated: false

### case-0

Baseline:

- Outcome a: 1
- Top-k \["a","b"\]: 1
- Run records: \["sha256:0000000000000000000000000000000000000000000000000000000000000002"\]

Run:

- Outcome b: 1
- Top-k null: 1
- Run records: \["sha256:0000000000000000000000000000000000000000000000000000000000000002"\]

