# Contract: normative summary

- Specs live under the configured specs directory, one `NNN-slug/spec.md` each;
the directory name equals the frontmatter `id`.
- `spec-spine compile` emits the registry; `spec-spine index` emits the codebase
index; `spec-spine lint` checks corpus conformance; `spec-spine couple` is the
PR-time gate.
- A changed code path must be accompanied by an authoring edit to a spec that
owns it, or a `Spec-Drift-Waiver:` line in the PR body.
- Read derived artifacts only through `spec-spine` subcommands; never parse the
JSON ad hoc.
- An `amends` edge is declared once, in the amending spec's frontmatter; the
amended `spec.md` is not edited to record it.
- The constitution is not amended by `amends` (its targets are spec ids). An
approved spec changes it by claiming the affected heading as a section unit
of that file; see the constitution's own Amendment section.

## Lifecycle as scheduling

Two frontmatter keys decide whether a spec is offered as work and how strictly
its claims are held. `status` is `draft` / `approved` / `superseded` /
`retired`; `implementation` is `pending` / `in-progress` / `complete` / `n-a` /
`deferred`, or absent.

| `status` | `implementation` | schedulable | unresolved unit is |
|---|---|---|---|
| `draft` | absent, `pending`, `in-progress` | yes | `W-001` warning |
| `approved` | `pending`, `in-progress` | yes | `W-001` warning |
| `approved` | absent | no (settled) | error |
| any | `complete` | no | error |
| any | `n-a`, `deferred` | no | takes its answer from `status` |
| `superseded`, `retired` | any | no | takes its answer from `status` |

- **`approved` plus `pending` is a work order.** It is the state a
specify-first corpus lives in for months, and the state `registry plan`
offers as ready.
- **`draft` is never a claim about code.** A draft's unresolved units are
expected, which is why they warn instead of refusing.
- **An absent `implementation` is not a third value.** It defers to `status`:
on a `draft` it reads as `pending`, on anything ratified as settled. That is
what keeps a bootstrap spec owning no code from being offered as ready
forever, and why `n-a` exists for a ratified spec that owns nothing.

## Extra keys

`frontmatter.extra_known_keys` in `spec-spine.toml` declares frontmatter keys
this corpus recognizes beyond the grammar. A declared key stops the lint
warning about it, and its value is preserved verbatim into the registry as
`extraFrontmatter`, so a consumer can read it.

The config lists the names and records nothing about what they mean. If you
declare keys, write down their semantics here or in your constitution, next to
the rest of what governs the corpus.
