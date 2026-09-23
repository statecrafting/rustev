# Constitution (tier 2)

Durable principles that govern this corpus. This document is **tier 2**: it is
subordinate to the bootstrap spec, whose `unamendable` anchors it may not
contradict, and it governs all ordinary specs.

**Normative hierarchy (highest wins):**

1. the bootstrap spec (`000`): non-overridable.
2. this constitution.
3. the contract: a normative summary of the bootstrap spec.
4. ordinary specs: feature-level claims within this envelope.

When two specs conflict, resolve in this order, then by the typed authority
graph.

---

## I. Markdown-only authored truth

Authored truth lives only in markdown with YAML frontmatter. If a fact governs
the system, it is written in a `spec.md` (or a standards document), never in a
derived artifact.

## II. Compiler-owned JSON machine truth

Machine-consumable truth is emitted by the compiler into the derived tree and is
read only through `spec-spine` subcommands. Hand-editing a derived artifact is a
workflow violation; ad-hoc parsing of one (`jq`/`awk`/`sed`) is equally
forbidden, because a typed read fails at the deserializer instead of silently
encoding a stale assumption.

## III. Spec-first development

A change to behavior begins with a change to a spec: the spec declares the units
it owns and the typed edges to its neighbours before the code is written. The
coupling gate enforces this at PR time. The escape valve is a named, scoped
waiver in the PR body, never a silent edit to an owner spec.

## IV. Determinism and validation

Every artifact-producing function is a pure function of (config, file contents):
the same inputs produce byte-identical output. Validation is mechanical, so
staleness is detectable by content-hash comparison alone.

## V. Legacy as evidence

Code that predates a governing spec is evidence, not a violation: a spec
claiming it declares `origin.retroactive: true` rather than masquerading as a
fresh `establishes` claim. Code adopted from outside the corpus is specced **as
found**, and the behavior the adopting spec would not have chosen is recorded
under a `## Known defects` heading. A defect recorded there is not thereby
blessed: it is what a later spec is written against.

---

## VI onward: the principles of the system you are specifying

Principles I through V govern the corpus and come from spec-spine. Principles
VI onward govern Rustev, the system this corpus describes, and bind every spec
equally. They are claimed by spec `001-boundaries-and-authority`. None is
frozen in the bootstrap's `unamendable` list.

### VI. Proposals, never grants

Rustev produces proposals and unresolved outcomes, never authorization. The
application validates the requested action, resource, parameters, principal,
scope and current revision before any effect. Model output cannot create or
broaden permissions. Keeping judgments out of authority signatures is a design
restriction, not a proof that model-derived values cannot be laundered into
other inputs; provenance (X) is what lets an application refuse them.

### VII. Values carry their semantics

Scores, distributions, calibrated probabilities, selected labels, ordinal
levels and rank positions are distinct kinds that never convert silently. A
conversion is an explicit, identified function or it does not exist.

### VIII. Unresolved is an answer

Missing, stale, conflicting or invalid evidence, invalid backend output and
exhausted budgets produce a typed unresolved outcome, never a default. A
selection policy handles each unresolved outcome it can meet explicitly.

### IX. Capabilities are disclosed, not assumed

A backend states what it returns. A plan whose requirement no bound backend
meets is refused, or uses a fallback the definition declares.

### X. Every decision is identified, attributed and evidenced

Definitions, plans, artifacts, calibrations, snapshots and datasets are
content-identified. Every value carries its derivation class (exact-derived,
model-derived, mixed-derived) and its lineage. A decision is evidence that the
system produced it, never independent evidence that the preference or fact it
was computed from is true.

### XI. Measured, not asserted

Quality, calibration and latency claims name the dataset, split, artifact and
conditions that measured them; an absent measurement is unknown. Applying a
calibration artifact establishes which transformation was applied and to what
it is bound, not that the output is calibrated on current data.

### XII. Partial adoption

Each major part is usable without the others and replaceable behind its seam.
Rustev is usable without Aicortex, Rahi, statecraft-cli or spec-spine at run
time.

### XIII. Determinism is scoped

Byte determinism is promised for canonical compilation (definitions, plans and
their identities) and for explicitly defined exact computation. Backend
numerical repeatability is a measured, per-artifact tolerance. Runtime
observations (latency, cost, scheduling) are recorded, not reproduced.
Principle IV governs this corpus's artifacts, not Rustev's runtime outputs.

---

## Amendment

This constitution is changed by an ordinary spec that is `approved`, **claims
the affected text as an authority unit**, and contradicts no `unamendable`
anchor of the bootstrap spec.

The claim uses the ordinary ownership vocabulary over a section unit of this
file: `establishes` for a principle the spec adds, `refines` (with a named
`aspect`) for one it tightens, `co_authority` for one genuinely shared.

```yaml
refines:
  - aspect: "legacy-as-evidence"
    unit: { kind: section, file: "standards/spec/constitution.md", anchor: "v-legacy-as-evidence" }
```

The anchor is the heading slug, so `## V. Legacy as evidence` is
`v-legacy-as-evidence`. `amends` is **not** the instrument: its targets are spec
ids, and this file is not a spec.

Unlike an amended `spec.md`, which is a record of what the corpus held when it
was ratified and is therefore never edited to mention its successors, this
document is a standing statement of what is true now. It is edited in place, and
its history lives in the specs that claimed each section, and in git.
