#!/bin/sh
# The selection gate tutorial, end to end, from the support-routing package's
# committed data. Every step is a `rustev` command; `sed` makes the plan
# variants and `jq` checks the result.
#
#   sh docs/features/selection-gate/example/run.sh [work-dir]
#
# RUSTEV names the binary (default: `rustev` on PATH). Run from the
# repository root. The work directory defaults to a new temporary one and
# must not exist.
set -eu

RUSTEV=${RUSTEV:-rustev}
HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
DATA=$(pwd -P)/packages/rustev-pkg-support-routing/data
[ -d "$DATA" ] || { echo "run from the repository root" >&2; exit 2; }
if [ $# -gt 0 ]; then
  [ ! -e "$1" ] || { echo "$1 already exists; give a new work directory" >&2; exit 2; }
  mkdir -p "$1"
  WORK=$1
else
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/rustev-selection-gate.XXXXXX")
fi
cd "$WORK"

# Decisions are evaluated at a fixed time, so the example gives the same
# verdicts on every run. The three handles stand in for a tenant, a context
# revision and a principal scope; replay bundles are bound to them.
NOW=1790164800000
HANDLE=sha256:0000000000000000000000000000000000000000000000000000000000000001

# 1. The baseline plan: the package's synthetic example definition, with
#    the rules program and topic calibration it is bound to.
cp "$DATA/synthetic-example.rules.json" rules.json
cp "$DATA/synthetic-example.calibration.json" topic.calibration.json
cp "$DATA/synthetic-example.definition.json" baseline.definition.json
"$RUSTEV" plan compile --definition baseline.definition.json --rules rules.json \
  --calibration topic.calibration.json --out baseline.plan.json > /dev/null

# 2. Decide every dataset case once with the baseline and capture a replay
#    bundle. Evaluation replays these bundles; no case is decided again.
mkdir -p bundles records
for snapshot in "$DATA"/snapshots/*.json; do
  id=$(basename "$snapshot" .json)
  "$RUSTEV" run --plan baseline.plan.json --rules rules.json \
    --calibration topic.calibration.json --snapshot "$snapshot" \
    --evaluation-time "$NOW" --decision-id "$id" --record-out "records/$id.json" \
    --capture-bytes 16777216 --bundle-out "bundles/$id.json" --retain embedded \
    --now-ms "$NOW" --tenant "$HANDLE" --context-revision "$HANDLE" \
    --principal-scope "$HANDLE" > /dev/null
done

# 3. Two candidates. The baseline escalates a ticket when the topic
#    classifier's top probability is below 0.6. `route-more` lowers that to
#    0.35 (route more tickets); `route-less` raises it to 0.8.
for variant in route-more:0.35 route-less:0.8; do
  name=${variant%%:*}
  threshold=${variant##*:}
  mkdir -p "variants/$name/rules" "variants/$name/calibrations"
  sed "s/\"threshold\":\"0.6\"/\"threshold\":\"$threshold\"/" \
    baseline.definition.json > "$name.definition.json"
  ! cmp -s baseline.definition.json "$name.definition.json" ||
    { echo "$name: the baseline threshold 0.6 was not found" >&2; exit 1; }
  "$RUSTEV" plan compile --definition "$name.definition.json" --rules rules.json \
    --calibration topic.calibration.json --out "variants/$name/plan.json" > /dev/null
  cp rules.json "variants/$name/rules/"
  cp topic.calibration.json "variants/$name/calibrations/"
done

# 4. Evaluate the baseline and both candidates on the model-selection split,
#    twice each, from the captured bundles.
cp "$HERE/repeat-config.json" repeat-config.json
mkdir -p comparison
"$RUSTEV" eval candidates --dataset "$DATA/queue.dataset.json" --split model_selection \
  --config "$DATA/queue.config.json" --adapter "$DATA/queue.adapter.json" \
  --bundles bundles --now-ms "$NOW" --tenant "$HANDLE" --context-revision "$HANDLE" \
  --principal-scope "$HANDLE" --repeat-config repeat-config.json --repeat 2 \
  --candidates variants --out comparison > /dev/null

# 5. Record the baseline's evaluation as the reference.
"$RUSTEV" baseline record --name production --eval comparison --now-ms "$NOW" \
  --out baseline.json > /dev/null

# 6. Select, with the gate and with a stricter one that also forbids any
#    ticket the baseline routed from being escalated by a candidate.
check_gate() {
  cp "$HERE/$1.json" "$1.json"
  status=0
  "$RUSTEV" gate select --selection-gate "$1.json" --baseline baseline.json \
    --baseline-eval comparison --candidates comparison --out "$1.selection.json" \
    > "$1.stdout.json" || status=$?
  echo "$1: gate select exit $status, $(jq -r .status "$1.stdout.json")"
  if [ "$status" -ne 0 ] || [ ! -f "$1.selection.json" ]; then
    echo "$1: gate select failed ($WORK)" >&2
    exit 1
  fi
  # Verdicts and rule results are compared, not bytes: run records carry
  # measured latency, so digests differ from run to run.
  jq '{ranked, unranked, candidates: [.candidates[] | {name, verdict,
        clauses: [.clauses[] | select(.clause | test("^require")) | {clause, result}]}]}' \
    "$1.selection.json" > "$1.verdicts.json"
  if ! cmp -s "$1.verdicts.json" "$HERE/expected-$1.json"; then
    diff "$HERE/expected-$1.json" "$1.verdicts.json" >&2 || true
    echo "$1: verdicts differ from expected-$1.json ($WORK)" >&2
    exit 1
  fi
}
check_gate gate
check_gate gate-strict
echo "verdicts match the expected files ($WORK)"
