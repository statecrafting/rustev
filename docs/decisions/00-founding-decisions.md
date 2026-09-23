# 00: Founding decisions

What is **adopted**, what is **recommended and awaiting the owner**, and the
**facts** those rest on. A recommendation is not a decision until this file
moves it to the adopted table in its own change.

## Adopted

| Id | Decision | Date |
|---|---|---|
| D-01 | Rust, one Cargo workspace, one owning spec per crate. | 2026-09-23 |
| D-02 | Governed by spec-spine, pinned exactly (`required_version = "=0.24.0"` in `spec-spine.toml`), installed repository-locally into `.tooling/bin` by `make tools`. A binary elsewhere on `PATH` does not answer for this repository. | 2026-09-23 |
| D-03 | The repository was initialized by `statecraft-cli init apply` (built from its checkout at `8e22444`), which scaffolded governance through the spec-spine library and registered the project. It is registered and qualified, not armed. | 2026-09-23 |
| D-04 | Rustev is the decision component of the ecosystem: it compiles and executes typed decision plans. It does not own knowledge, authority, identity or agent workflows. Normative text: spec `001` once approved. | 2026-09-23 |
| D-05 | Registered decision plans first; dynamic questions later, as a separately evaluated class with narrower claims. | 2026-09-23 |
| D-06 | Pluggability is by Rust traits with explicit composition, plus a versioned protocol for remote adapters. No dynamic plugin loading. | 2026-09-23 |
| D-07 | Apache-2.0, as the repository was created. | 2026-09-23 |

D-04 to D-06 record the direction the owner committed to in the ecosystem
brief. Their exact wording becomes normative only through an approved spec.

## Recommended, awaiting the owner

The recommendations `R-01` to `R-06` are stated with their reasoning in
[`docs/design/001-decision-engine-architecture.md`](../design/001-decision-engine-architecture.md)
section 18. None is adopted.

## Facts

| Id | Fact | Source |
|---|---|---|
| C-01 | spec-spine 0.24.0 is released (`spec-spine-cli` on crates.io, tag `v0.24.0`). | `cargo search spec-spine-cli`; `git tag` in `~/DevWork/spec-spine` |
| C-02 | statecraft-cli links `spec-spine-core =0.23.0`, so its governance scaffold is produced by the 0.23.0 library even when the 0.24.0 CLI runs the corpus step. Its environment manifest recorded the CLI pin as `0.24.0`, observed from `PATH`. | `init plan` output ("producer spec-spine-core@0.23.0"); `.statecraft/environment.json` |
| C-03 | statecraft-cli resolves a bare `spec-spine` on `PATH` for the corpus step; it does not consult `.tooling/bin`. Initialization was run with `.tooling/bin` prepended to `PATH`. | statecraft-cli README, "What each step needs" |
| C-04 | The scaffold writes `spec-spine.toml` with the pin commented out and `specs/000-bootstrap/spec.md` with a placeholder date, and records both as `managed`. Customizing them, as the scaffold instructs, makes `statecraft-cli doctor` report both as `drifted`. A dogfood finding for statecraft-cli, not a defect here. | `doctor` output after pinning, 2026-09-23 |
| C-04a | The same scaffold set `[layout] derived_dir = ".statecraft/derived"` but left `[index] resolver_exclusions` at the old `.derived`, and did not exclude `.tooling`. Corrected here at adoption. | `spec-spine.toml` as scaffolded |
| C-05 | `.statecraft/derived/` is committed, so `check` is a freshness gate on the committed shards. | the managed `.gitignore` block |
| C-06 | Aicortex is specified and largely `pending`; Rahi is at a 0.2.0 candidate with an N=1 supported topology. No Rustev component may wait on either. | their READMEs, 2026-09-21 |
