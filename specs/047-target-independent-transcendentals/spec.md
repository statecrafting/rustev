---
id: "047-target-independent-transcendentals"
title: "One implementation of exp and ln on every target (amends 002 and 027)"
status: approved
implementation: pending
created: "2026-10-21"
summary: >
  Approved (A-44). The remedy spec 027 3.11.5 names for
  a cross-target floating-point divergence. The pure crates' four
  transcendental call sites (`exp` in core's softmax, `ln` in core's
  calibration, `ln` in evaluation's log loss and temperature fit) call the
  platform's math library on native targets and Rust's bundled one on
  WebAssembly; measured on 2026-10-21 they differ by one unit in the last
  place on 0.08% to 7.1% of a million inputs per input domain (macOS
  arm64 against `wasm32-unknown-unknown`). This spec routes all four through one
  pure Rust implementation, the `libm` crate exact-pinned at 0.2.16 without
  default features, exposed as `rustev_core::math`, so each produces the
  same bits on every target; no existing golden changes. Ratified by the
  owner on 2026-10-21 (A-44). Approval does not authorize a release.
amends:
  - "002-decision-contract-and-pure-core"
  - "027-node-and-wasm-bindings"
depends_on:
  - "002-decision-contract-and-pure-core"
  - "004-evaluation-and-replay"
  - "027-node-and-wasm-bindings"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "Every exp and ln reachable from a pure command is computed by rustev_core::math, one pure Rust implementation (libm =0.2.16, default features off) on every target; no pure crate calls f64::exp, f64::ln or another platform math function, and a test fails when one does."
    anchor: "3-one-implementation"
  - id: "I-1"
    kind: invariant
    text: "Changing the implementation's pin is a reviewed change that runs spec 027's differential corpus and every golden on every target; it never happens through an unpinned update."
    anchor: "3-one-implementation"
---

# 047: One implementation of exp and ln on every target (amends 002 and 027)

Approved: ratified by the owner on 2026-10-21 (decision A-44).
The approved texts of specs 002 and 027 are not edited; this spec records
what changes in them.

## 1. Purpose

Spec 002 3.4.6 excludes `exp` and `ln` from exact computation: "those are
backend-output transformations with numerical, not byte, repeatability".
Spec 027 3.11.2 lists where they occur in the pure crates (`exp` in core's
distribution normalization, `ln` in core's calibration, `ln` in
evaluation's log loss and temperature fitting) and observes that on native
targets they come from the platform's math library and on
`wasm32-unknown-unknown` from Rust's bundled implementation, so results
may differ in the last bit. Spec 027 3.11.5 then rules: any bit difference
in a conformance case or the differential corpus fails the parity job, the
remedy is "routing those functions through one pure Rust implementation on
every target", and "until it lands the affected surface is not released".

Measured on 2026-10-21 (Rust 1.98.1, release builds), macOS arm64 native
against `wasm32-unknown-unknown` in Node, with a million inputs per input
domain drawn as the call sites see them. The two evaluation sites apply the
same function (`ln`) to the same domain (a probability in (0, 1]), so one
input set measured both, and their rows share its result:

| Call site | Domain | Inputs differing | Largest difference |
|---|---|---|---|
| `exp` in core's softmax | x ≤ 0 | 829 (0.08%) | 1 ulp |
| `ln` in core's calibration | finite x > 0 | 1,080 (0.11%) | 1 ulp |
| `ln` in evaluation's log loss (`metrics.rs`) | 0 < x ≤ 1 | 71,134 (7.1%), the input set shared with the next row | 1 ulp |
| `ln` in evaluation's temperature fit (`fit.rs`) | 0 < x ≤ 1 | 71,134 (7.1%), the input set shared with the previous row | 1 ulp |

The differential corpus of 027 3.11.3 would therefore fail on its first
run, and neither binding package could be released. Native targets also
differ from one another (glibc, musl, Apple and Microsoft each have their
own `exp` and `log`), so the native package is affected as well.

A probe before this spec (macOS arm64, 2026-10-21, not the
implementation) switched the four call sites to `libm` directly: the full
workspace test suite then passed with no golden changed, except
evaluation's dependency allowlist test, which the core module of 3.2
avoids. That is an observation, not acceptance evidence; the
implementation's own run is recorded in its implementation record.

## 2. Territory

The four call sites in `crates/rustev-core/src/kinds.rs` (softmax),
`crates/rustev-core/src/calibrate.rs` (the log of a distribution mass),
`crates/rustev-eval/src/metrics.rs` (log loss) and
`crates/rustev-eval/src/fit.rs` (temperature fit); a new public module
`rustev_core::math`; the workspace dependency `libm`. Spec 027 3.11's
promise, which this spec strengthens.

## 3. One implementation

1. `rustev_core::math` exposes `exp(f64) -> f64` and `ln(f64) -> f64`,
   implemented by the `libm` crate, exact-pinned at `=0.2.16` with
   `default-features = false`. `libm` is pure Rust (`no_std`, MIT,
   maintained by the Rust project; Rust's own standard library uses it on
   targets without a system math library), performs no I/O and reads no
   clock, so core stays pure under principle VI onward and spec 001's
   boundary check. Without default features it takes no
   architecture-specific path (its `arch` feature selects hardware
   instructions on some targets): every target runs the same integer and
   IEEE 754 basic operations, which agree bit for bit.
2. The four call sites call `rustev_core::math::exp` and
   `rustev_core::math::ln`. Evaluation reaches them through core, so its
   normal dependencies stay contract, core and serde (spec 004 2).
3. A test in core fails when a source file of `rustev-contract`,
   `rustev-core`, `rustev-eval` or `rustev-surface` calls a platform math
   method on `f64` instead of
   `rustev_core::math`. The methods are every `f64` method the
   standard library computes through a math library: `exp`, `exp2`,
   `exp_m1`, `ln`, `ln_1p`, `log`, `log2`, `log10`, `powf`, `cbrt`,
   `hypot`, `sin`, `cos`, `tan`, `sin_cos`, `asin`, `acos`, `atan`,
   `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh` and `atanh`, each in
   method form (`x.exp2()`) and path form (`f64::exp2(x)`). `sqrt` and
   `powi` are not covered: `sqrt` is
   correctly rounded by IEEE 754, and none of the four crates uses either
   in a pure command's path today; adding one is reviewed against this
   spec.
4. Changing the pin is an ordinary reviewed dependency change that must
   run spec 027's differential corpus and every golden on every target.
   An update that changes a bit fails them and needs its own golden
   migration.

### What changes in spec 002

3.4.6's last sentence reads, as amended: `exp` and `ln`, which logit
normalization and temperature calibration use, are computed by one pure
Rust implementation (`rustev_core::math`) on every target. They are not
correctly rounded, so they stay outside exact computation (their result is
an approximation whose error the implementation bounds, not a defined
exact value), but for a given pin they produce the same bits on every
target.

### What changes in spec 027

1. 3.11.2 reads, as amended: the pure paths' transcendental calls (the
   four of section 2) are computed by `rustev_core::math` on every target,
   native and WebAssembly alike; the platform's math library is not used.
2. 3.11.3 and 3.11.5 are unchanged: the differential corpus still runs
   each call site on every target and any bit difference still fails the
   parity job. This spec is the remedy 3.11.5 names, so it lands before
   the corpus job runs.
3. 3.11.4 is unchanged: the bindings still claim nothing beyond what the
   goldens and the corpus measure.

## 4. Out of scope

Correct rounding (a correctly rounded `exp` and `ln` would change no
promise this spec makes and costs more code); any change to plan, identity,
canonical or evidence bytes; backend crates, whose numerical code is
backend output under spec 002 3.4.6; `sqrt` and `powi`.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A pure crate calls `f64::exp` or `f64::ln` directly | The core test of 3.3 fails, naming the file and line. |
| The corpus finds one bit differing between two targets | Spec 027's parity job fails, as before. |
| `libm` is updated without its pin changing in review | Impossible: the pin is exact and `Cargo.lock` is checked. |

## Acceptance

- The four call sites use `rustev_core::math`; the test of 3.3 passes and
  fails when a direct call is seeded.
- Every existing test and golden passes unchanged on every target CI runs.
- The release audit (cargo-deny bans, licenses and sources over the
  release set) passes with `libm`.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation. Spec 027's increment 7 runs the differential
corpus that shows the result on every target.

## Implementation record

Not started.
