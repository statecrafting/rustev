// Spec 063, 3.9: the repeatability commands on the native package, each
// with its happy path and its refusals. Outcomes are values (`code` and
// `out` as the CLI prints them); usage rules throw `InvalidDocument` before
// any input is read (3.9.2). SYNTHETIC fixtures: the `repeat-*` conformance
// cases' pinned inputs and goldens, and spec 061's synthetic host runs
// (`tools/rustev-conformance/fixtures/host-import`) with the CLI's goldens.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkg = join(here, '..');
const root = join(pkg, '../..');
const rustev = createRequire(import.meta.url)(pkg);

const read = (p) => readFileSync(join(root, p));
/** A harvested case: its inputs by name, and its goldens by output. */
function harvested(id) {
  const c = JSON.parse(read(`tools/rustev-conformance/cases/cli/${id}.json`));
  const inputs = Object.fromEntries(c.inputs.map((i) => [i.name, read(i.path)]));
  const goldens = Object.fromEntries(c.outputs.map((o) => [o.stream ?? o.file, read(o.golden)]));
  return { inputs, goldens, exit: c.exit };
}
/** The result against the case: exit code, printed bytes and artifacts. */
function matches(r, c) {
  assert.equal(r.code, c.exit);
  for (const [name, want] of Object.entries(c.goldens)) {
    const got = name === 'stdout' ? r.out : r.artifacts[name];
    assert.ok(got, `no ${name}`);
    assert.ok(Buffer.from(got).equals(want), `${name} differs`);
  }
}
const out = (r) => JSON.parse(Buffer.from(r.out).toString('utf8'));
const usage = (rule) => ({ name: 'InvalidDocument', message: new RegExp(rule) });

const engine = await rustev.Engine.create({
  runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 4, sinkTimeoutMs: 1000 },
  clock: 'manual',
  sink: { queueCapacity: 1, deliver: async () => 'ack' },
});
test.after(() => engine.shutdown());

const SCOPE = { mode: 'principal', tenant: 'tenant-a', context_revision: 'rev-1', principal_scope: 'scope-p' };
const evalArgs = {
  dataset: 'support.dataset.json',
  split: 'calibration',
  config: 'support.config.json',
  adapter: 'support.adapter.json',
  repeatConfig: 'repeat-config.json',
  bundles: 'bundles',
  repeat: 2,
  nowMs: 1_800_003_600_000,
  scope: SCOPE,
};

test('evaluate with repeatConfig is eval --repeat-config', async () => {
  const c = harvested('repeat-reported.stdout');
  matches(await engine.evaluate(c.inputs, { ...evalArgs, out: 'b' }), c);
  // The same captured runs as a list of one prefix.
  matches(await engine.evaluate(c.inputs, { ...evalArgs, bundles: ['bundles'], out: 'b' }), c);
});

test('evaluate with repeatConfig: usage rules throw, refusals are results', async () => {
  const { inputs } = harvested('repeat-reported.stdout');
  const a = { ...evalArgs, out: 'b' };
  await assert.rejects(engine.evaluate(inputs, { ...a, repeatConfig: undefined }), usage('repeat needs repeatConfig'));
  await assert.rejects(engine.evaluate(inputs, { ...a, repeat: undefined }), usage('one bundles prefix needs repeat'));
  await assert.rejects(engine.evaluate(inputs, { ...a, repeat: 65 }), usage('repeat is an integer'));
  await assert.rejects(engine.evaluate(inputs, { ...a, ledgerOut: 'l.json' }), usage('ledgerOut is only for final_test'));
  await assert.rejects(engine.evaluate(inputs, { ...a, ledgerNew: true }), usage('ledgerNew is only for final_test'));
  await assert.rejects(engine.evaluate(inputs, { ...a, purpose: 'tuning' }), usage('purpose is measurement or optimization'));
  await assert.rejects(engine.evaluate(inputs, { ...a, rules: ['r.json'] }), usage('rules needs candidatePlan'));
  await assert.rejects(engine.evaluate(inputs, { ...a, scores: ['s.json'] }), usage('scores is not used with repeatConfig'));
  await assert.rejects(engine.evaluate(inputs, { ...a, bundles: ['b1', 'b2'], repeat: 3 }), usage('captured runs need'));
  await assert.rejects(
    engine.evaluate(inputs, { ...a, split: 'final_test', purpose: 'optimization' }),
    usage('optimization cannot use final_test'),
  );
  await assert.rejects(
    engine.evaluate(inputs, { ...a, split: 'final_test', lineage: 'l', ledgerOut: 'l.json' }),
    usage('give exactly one of ledger and ledgerNew'),
  );
  // No captured runs: the live source stays refused, as in the CLI.
  const live = await engine.evaluate(inputs, { ...a, bundles: [] });
  assert.equal(live.code, 4);
  assert.equal(out(live).detail, 'live-source-unavailable');
  assert.deepEqual(live.artifacts, {});
  // A missing input is a result (the CLI's `io_error`), and nothing is
  // written.
  const { 'support.dataset.json': _, ...missing } = inputs;
  const r = await engine.evaluate(missing, a);
  assert.equal(r.code, 3);
  assert.equal(out(r).status, 'io_error');
  assert.deepEqual(r.artifacts, {});
});

test('evalCandidates compares candidates over repeated runs', async () => {
  const c = harvested('repeat-compared.stdout');
  matches(await engine.evalCandidates(c.inputs, { ...evalArgs, candidates: 'variants', out: 'comparison' }), c);
  await assert.rejects(
    engine.evalCandidates(c.inputs, { ...evalArgs, candidates: 'variants', candidatePlan: 'p.json', out: 'comparison' }),
    usage('evalCandidates takes no candidatePlan'),
  );
  await assert.rejects(engine.evalCandidates(c.inputs, { ...evalArgs, out: 'comparison' }), usage('candidates must be a string'));
  const none = await engine.evalCandidates(c.inputs, { ...evalArgs, candidates: 'absent', out: 'comparison' });
  assert.equal(none.code, 4);
  assert.equal(out(none).status, 'invalid_input');
  assert.deepEqual(none.artifacts, {});
});

test('baselineRecord records a baseline at nowMs', async () => {
  const c = harvested('repeat-recorded.stdout');
  const args = { name: 'base', eval: 'base', nowMs: 0, out: 'recorded.json' };
  const r = await engine.baselineRecord(c.inputs, args);
  matches(r, c);
  assert.ok(r.artifacts['recorded.json']);
  await assert.rejects(engine.baselineRecord(c.inputs, { ...args, nowMs: undefined }), usage('nowMs is required'));
  await assert.rejects(engine.baselineRecord(c.inputs, { ...args, nowMs: -1 }), usage('nowMs must be'));
  const missing = await engine.baselineRecord(c.inputs, { ...args, eval: 'absent' });
  assert.equal(missing.code, 3);
  assert.deepEqual(missing.artifacts, {});
});

test('evalReproducibility: reproducible, not_reproducible and unknown are results', async () => {
  const args = { baseline: 'baseline.json', baselineEval: 'base', run: 'run' };
  for (const id of ['repeat-reproducible.stdout', 'repeat-drift.stdout', 'repeat-unknown.stdout']) {
    const c = harvested(id);
    matches(await engine.evalReproducibility(c.inputs, args), c);
  }
  const { inputs } = harvested('repeat-reproducible.stdout');
  await assert.rejects(engine.evalReproducibility(inputs, { ...args, run: undefined }), usage('run must be a string'));
  const missing = await engine.evalReproducibility(inputs, { ...args, baseline: 'absent.json' });
  assert.equal(missing.code, 3);
  assert.equal(out(missing).status, 'io_error');
});

test('evalDiff writes its diff under out', async () => {
  const args = { baseline: 'baseline.json', baselineEval: 'base', run: 'run', out: 'diff' };
  for (const id of ['repeat-diff.stdout', 'repeat-diff-unknown.md']) {
    const c = harvested(id);
    matches(await engine.evalDiff(c.inputs, args), c);
  }
  const { inputs } = harvested('repeat-diff.stdout');
  await assert.rejects(engine.evalDiff(inputs, { ...args, out: undefined }), usage('out must be a string'));
  // An output is never written over an input: all or none.
  const clash = await engine.evalDiff({ ...inputs, 'diff/diff.json': Buffer.from('{}') }, args);
  assert.equal(clash.code, 3);
  assert.deepEqual(clash.artifacts, {});
});

test('gateSelect: qualifies, none_qualified and unknown are results', async () => {
  const args = { selectionGate: 'gate.json', baseline: 'baseline.json', baselineEval: 'base', candidates: 'candidates' };
  for (const [id, file] of [
    ['repeat-selection-qualifies.stdout', 'qualifies.json'],
    ['repeat-selection-reject.stdout', 'reject.json'],
    ['repeat-selection-unknown.stdout', 'unknown.json'],
  ]) {
    const c = harvested(id);
    matches(await engine.gateSelect(c.inputs, { ...args, out: file }), c);
  }
  const { inputs } = harvested('repeat-selection-unknown.stdout');
  const unknown = await engine.gateSelect(inputs, { ...args, out: 'unknown.json' });
  assert.equal(unknown.code, 12);
  assert.equal(out(unknown).status, 'unknown');
  await assert.rejects(engine.gateSelect(inputs, { ...args, out: 'u.json', selectionGate: 7 }), usage('selectionGate must be a string'));
});

/** Spec 061's three synthetic host runs, with the configurations. */
function hostRuns() {
  const base = join(root, 'tools/rustev-conformance/fixtures/host-import');
  const inputs = {};
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else inputs[relative(base, p).split('\\').join('/')] = readFileSync(p);
    }
  };
  walk(base);
  return inputs;
}
const host = (n) => read(`crates/rustev-cli/tests/golden/host/${n}`);
const importArgs = { repeatConfig: 'repeat-config.json', config: 'config.json', runs: ['in/run-0', 'in/run-1', 'in/run-2'], out: 'out' };

test('evalRepeatImport imports host runs with the CLI bytes', async () => {
  const r = await engine.evalRepeatImport(hostRuns(), importArgs);
  assert.equal(r.code, 0);
  assert.ok(Buffer.from(r.out).equals(host('import.stdout')));
  assert.ok(Buffer.from(r.artifacts['out/repeat.json']).equals(host('import.repeat.json')));
  assert.ok(Buffer.from(r.artifacts['out/run-1.cases.json']).equals(host('import.run-1.cases.json')));
  assert.equal(out(r).assertion, 'host-asserted');
});

test('evalRepeatImport refusals', async () => {
  const inputs = hostRuns();
  await assert.rejects(engine.evalRepeatImport(inputs, { ...importArgs, runs: [] }), usage('runs lists 1 to 64'));
  await assert.rejects(
    engine.evalRepeatImport(inputs, { ...importArgs, runs: Array(65).fill('in/run-0') }),
    usage('runs lists 1 to 64'),
  );
  await assert.rejects(engine.evalRepeatImport(inputs, { ...importArgs, runs: 'in/run-0' }), usage('runs lists 1 to 64'));
  // An existing output refuses the whole set; nothing is written.
  const clash = await engine.evalRepeatImport({ ...inputs, 'out/repeat.json': Buffer.from('{}') }, importArgs);
  assert.equal(clash.code, 3);
  assert.deepEqual(clash.artifacts, {});
  const missing = await engine.evalRepeatImport(inputs, { ...importArgs, runs: ['in/run-0', 'in/absent'] });
  assert.equal(missing.code, 3);
  assert.deepEqual(missing.artifacts, {});
});

test('host and Rustev sets are never mixed (061 I-1)', async () => {
  // A host baseline: imported, then recorded, each command's artifacts
  // the next one's inputs.
  const imported = await engine.evalRepeatImport(hostRuns(), { ...importArgs, out: 'base' });
  assert.equal(imported.code, 0);
  const recorded = await engine.baselineRecord(imported.artifacts, { name: 'base', eval: 'base', nowMs: 0, out: 'baseline.json' });
  assert.equal(recorded.code, 0);
  const gate = {
    schema: 'rustev.selection-gate/1', gate: 'host-flow', per: 'variant',
    metrics: { 'rubric/brevity': 'higher_is_better' },
    min_comparable_coverage: '0.8', min_labeled_coverage: '0.4',
    require: [{ all_slices: { metric: 'rubric/brevity', not_worse_than: 'baseline', within: 'noise' } }],
    rank_by: ['rubric/brevity'],
  };
  const inputs = {
    ...imported.artifacts,
    ...recorded.artifacts,
    'gate.json': Buffer.from(JSON.stringify(gate)),
    'rustev-cands/a/repeat.json': Buffer.from('{"schema":"rustev.repeat-report/1"}'),
  };
  const r = await engine.gateSelect(inputs, {
    selectionGate: 'gate.json', baseline: 'baseline.json', baselineEval: 'base', candidates: 'rustev-cands', out: 'sel.json',
  });
  assert.equal(r.code, 4);
  assert.match(Buffer.from(r.out).toString('utf8'), /mixed-provenance/);
  assert.deepEqual(r.artifacts, {});
});

test('the commands join command()', async () => {
  const c = harvested('repeat-drift.stdout');
  const r = await engine.command('evalReproducibility', c.inputs, { baseline: 'baseline.json', baselineEval: 'base', run: 'run' });
  matches(r, c);
  assert.equal(r.code, 13);
  assert.equal(out(r).status, 'not_reproducible');
});
