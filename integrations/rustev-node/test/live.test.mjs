// SYNTHETIC live runs only, using the existing runtime conformance fixtures.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const pkg = join(dirname(fileURLToPath(import.meta.url)), '..');
const rustev = createRequire(import.meta.url)(pkg);
const fx = n => readFileSync(join(pkg, '../../tools/rustev-conformance/fixtures/runtime', n));
const canonical = x => JSON.stringify(x, (_, v) => v && !Array.isArray(v) && typeof v === 'object'
  ? Object.fromEntries(Object.keys(v).sort().map(k => [k, v[k]])) : v);
const digest = x => 'sha256:' + createHash('sha256').update(x.schema).update('\0').update(canonical(x)).digest('hex');
const json = p => JSON.parse(readFileSync(p));
const NOW = 1790164800000;
const OUTPUTS = {
  'support.topic': { logits: { billing: 4, integration_defect: 0, account_access: 0, other: 0 } },
  'support.frustration': { logits: { calm: 3, frustrated: 0, very_angry: 0 } },
  'support.deadline': { logits: { false: 3, true: 0 } },
};
const answer = c => ({ output: OUTPUTS[JSON.parse(Buffer.from(c.projection)).task], charge: { observed: { units: 1 } } });
const until = async f => {
  const end = Date.now() + 30000;
  while (!f()) { assert.ok(Date.now() < end, 'condition timed out'); await new Promise(setImmediate); }
};
async function fixture(t, { model = 'bounded', infer, cap = 1000 } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'rustev-node-live-'));
  const path = n => join(dir, n);
  const write = (n, value) => { mkdirSync(dirname(path(n)), { recursive: true }); writeFileSync(path(n), Buffer.isBuffer(value) ? value : canonical(value)); };
  const descriptor = JSON.parse(fx('synthetic-linear-head.descriptor.json'));
  write('candidates/a/plan.json', fx('support.plan.json'));
  for (const n of ['synthetic-linear-head', 'synthetic-label-only', 'synthetic-linear-head-replica', 'synthetic-other-head']) {
    write(`candidates/a/descriptors/${n}.json`, fx(`${n}.descriptor.json`));
  }
  write('candidates/a/calibrations/topic.json', fx('topic.calibration.json'));
  const snapshot = JSON.parse(fx('support.snapshot.json'));
  const probe = structuredClone(snapshot);
  probe.entries[0].value = 'SYNTHETIC probe';
  write('snapshots/one.json', snapshot); write('probe.json', probe);
  const adapter = { schema: 'rustev.task-adapter/1', name: 'test', version: '1', labels: { values: ['yes'], prefixes: [] }, rules: [{ action: 'route_ticket', label: 'yes' }] };
  write('adapter.json', adapter);
  write('config.json', { schema: 'rustev.evaluator-config/2', adapter: { name: 'test', version: '1', rules: { bound: digest(adapter) } }, label_interpretation: 'SYNTHETIC', agreement: { params: [] }, probability_step: null, reliability_bins: ['0', '1'], subgroups: [], latency_quantiles: [], cost_units_comparable: false, metrics: [
                {"name":"coverage","formula":"comparable cases / all cases"},
                {"name":"acceptance_coverage","formula":"proposals / comparable cases"},
                {"name":"labeled_proposal_coverage","formula":"labeled proposals / proposals"},
                {"name":"error_among_accepted","formula":"wrong labeled proposals / labeled proposals"},
                {"name":"candidate_outcome_change","formula":"changed outcomes / comparable cases"},
                {"name":"mean_log_loss","formula":"mean negative log probability of label"},
                {"name":"brier","formula":"mean sum of squared probability errors"},
                {"name":"reliability","formula":"per bin mean confidence and accuracy"},
                {"name":"latency_ms","formula":"quantile of elapsed minus queued"},
                {"name":"queue_ms","formula":"quantile of queued"},
                {"name":"cost_observed_units","formula":"sum of observed units"},
                {"name":"cost_estimated_units","formula":"sum of estimated units"},
                {"name":"cost_unknown_attempts","formula":"attempts with unknown charge"},
                {"name":"cost_liability_units","formula":"units kept as liability"}
            ], gates: [], temperature_grid: [] });
  write('repeat.json', { schema: 'rustev.repeat-config/1', bootstrap: { method: 'percentile-cluster/1', prng: 'xoshiro256starstar-splitmix64/1', level: '0.95', resamples: 1000, seed: '1' }, noise: { rule: 'interval_width', floor: '0', overrides: {} }, slices: [], stability_metrics: ['coverage'], top_k: null });
  write('dataset.json', { schema: 'rustev.dataset/1', name: 'SYNTHETIC live', provenance: { synthetic: { generator: 'SYNTHETIC tests' } }, adapter: { name: 'test', version: '1' }, cases: [{ id: 'one', source: 'one', snapshot: digest(snapshot), split: 'training', label: 'missing', subgroups: [] }] });
  write('live.json', { schema: 'rustev.live-config/1', budget: 'testing', run_cap_units: cap, repeats: 1, evaluation_time: { fixed: { ms: NOW } }, bindings: [{ identity: { backend_id: descriptor.backend_id, artifact: descriptor.artifact }, cost_unit: 'nano_usd', max_in_flight: 1, min_interval_ms: 0, case_deadline_ms: 1000, rate_limit_pause_max_ms: 0 }], acquisitions: {}, probe: { calls: 1, interval_ms: 0, snapshot: path('probe.json'), digest: digest(probe), provenance: 'synthetic' }, authorize: [{ backend_id: descriptor.backend_id, artifact: descriptor.artifact, operations: ['classify', 'proposition', 'rubric'], max_exposure_units: 100 }] });
  const calls = [];
  const e = await rustev.Engine.create({ clock: 'manual', runtime: { maxInFlight: 2, maxQueued: 0, maxParallelRequests: 3, sinkTimeoutMs: 1000 }, sink: { queueCapacity: 8, deliver: async () => 'ack' }, backends: [{ descriptor, costModel: model, costBound: model === 'unknown' ? 'unknown' : { [model]: model === 'bounded' ? { max_units: 5 } : { units: 5 } }, maxConcurrency: 8, queueCapacity: 8, infer: async c => {
    const lines = readFileSync(path('journal'), 'utf8');
    assert.ok(lines.includes(c.attemptId), 'reservation must precede host dispatch');
    calls.push(c);
    return infer ? infer(c) : answer(c);
  } }] });
  t.after(async () => { await e.shutdown(); rmSync(dir, { recursive: true, force: true }); });
  const a = { live: true, liveConfig: path('live.json'), journal: path('journal'), runId: 'synthetic', dataset: path('dataset.json'), split: 'training', config: path('config.json'), adapter: path('adapter.json'), scope: { mode: 'tenant_only', tenant: 'synthetic', context_revision: '1' }, nowMs: NOW, repeatConfig: path('repeat.json'), candidates: path('candidates'), snapshots: path('snapshots'), out: path('out') };
  return { e, a, path, calls, write };
}
const expect = (r, status, code) => { assert.equal(r.status, status, Buffer.from(r.out).toString()); assert.equal(r.code, code); };
test('full runs reserve before dispatch and retain journal identity and spend', async t => {
  const f = await fixture(t);
  const r = await f.e.liveRun(f.a); expect(r, 'evaluated_live', 0);
  assert.equal(r.journal.created, 'this_run'); assert.equal(r.journal.spendBefore, 0);
  assert.match(Buffer.from(r.out).toString(), /new_journal\n$/);
  for (const name of ['live-run.json', 'checkpoint.json', 'comparison.md', 'a/adapter.json']) assert.ok(existsSync(f.path(`out/${name}`)), name);
  const second = await f.e.liveRun({ ...f.a, runId: 'second', out: f.path('out2'), requireExistingJournal: true });
  expect(second, 'evaluated_live', 0); assert.equal(second.journal.created, 'earlier');
  assert.equal(second.journal.id, r.journal.id); assert.ok(second.journal.spendBefore > 0);
  assert.doesNotMatch(Buffer.from(second.out).toString(), /new_journal/);
});
test('estimated probe-only and failed probe send no dataset case', async t => {
  const f = await fixture(t, { model: 'estimated' });
  expect(await f.e.liveRun({ ...f.a, probeOnly: true }), 'probed', 0);
  assert.ok(f.calls.every(c => c.attemptId.includes('.probe')));
  const g = await fixture(t, { infer: async () => { throw new Error('synthetic failure'); } });
  expect(await g.e.liveRun(g.a), 'probe_failed', 15);
  assert.ok(g.calls.every(c => c.attemptId.includes('.probe')));
  const failed = json(g.path('out/live-run.json'));
  assert.ok(failed.journal_after.liability > 0, 'plain thrown errors retain unknown charge as liability');
});
test('unknown cost, missing journal, locked journal and cap are outcomes', async t => {
  const f = await fixture(t, { model: 'unknown' });
  const r = await f.e.liveRun(f.a); expect(r, 'invalid_input', 4); assert.match(Buffer.from(r.out).toString(), /spend-unit/); assert.ok(!existsSync(f.a.journal));
  const g = await fixture(t);
  const missing = await g.e.liveRun({ ...g.a, requireExistingJournal: true }); expect(missing, 'invalid_input', 4); assert.match(Buffer.from(missing.out).toString(), /journal-missing/); assert.ok(!existsSync(g.a.journal)); assert.ok(!existsSync(g.a.out));
  // The journal uses an exclusive lock file, including when held by another process.
  writeFileSync(g.a.journal + '.lock', 'synthetic');
  const locked = await g.e.liveRun(g.a); expect(locked, 'io_error', 3); assert.match(Buffer.from(locked.out).toString(), /journal-locked/);
  const h = await fixture(t, { cap: 1 }); expect(await h.e.liveRun(h.a), 'spend_capped', 14); assert.equal(h.calls.length, 0);
});
test('abort and run deadline keep liability and discard late host settlements', async t => {
  for (const deadline of [false, true]) {
    let settle;
    const f = await fixture(t, { infer: c => new Promise(resolve => { settle = () => resolve(answer(c)); }) });
    const controller = new AbortController();
    const p = f.e.liveRun({ ...f.a, signal: controller.signal, ...(deadline ? { runDeadlineMs: 10 } : {}) });
    await until(() => !!settle);
    if (deadline) f.e.advanceClock(10); else controller.abort();
    await until(() => f.calls.some(c => c.signal.aborted));
    f.e.advanceClock(2000);
    const r = await p; expect(r, 'interrupted', 16);
    assert.ok(existsSync(f.path('out/checkpoint.json')));
    const report = json(f.path('out/live-run.json')); assert.ok(report.journal_after.liability + report.journal_after.outstanding > 0);
    const before = readFileSync(f.a.journal, 'utf8'); settle();
    await until(() => f.e.gauges().seams.some(g => g.lateResolutionsDiscarded > 0));
    assert.equal(readFileSync(f.a.journal, 'utf8'), before);
  }
});
test('already-aborted signal writes nothing; memory and usage rules precede reads', async t => {
  const f = await fixture(t);
  await assert.rejects(f.e.liveRun({ ...f.a, signal: AbortSignal.abort() }), { name: 'CancelledBeforeAdmission' });
  assert.ok(!existsSync(f.a.out)); assert.ok(!existsSync(f.a.journal));
  await assert.rejects(f.e.liveRun({ ...f.a, inputs: {} }), /durable-paths-required/);
  const bad = [
    [{ live: false }, 'live'], [{ liveConfig: undefined }, 'liveConfig'], [{ journal: undefined }, 'journal'], [{ out: undefined }, 'out'], [{ nowMs: undefined }, 'nowMs'],
    [{ snapshots: undefined }, 'exactly one'], [{ bundles: 'absent' }, 'exactly one'], [{ split: 'final_test' }, 'final_test'], [{ repeat: 1 }, 'repeat'],
    [{ rerunInterrupted: true }, 'resume'], [{ purpose: 'invalid' }, 'purpose'], [{ captures: 'absent' }, 'captures'],
    ...['lineage', 'ledger', 'ledgerOut'].map(k => [{ [k]: 'x' }, k]), [{ ledgerNew: true }, 'ledgerNew'],
    [{ runDeadlineMs: 0 }, 'runDeadlineMs'], [{ resume: 'yes' }, 'resume'],
  ];
  for (const [patch, rule] of bad) await assert.rejects(f.e.liveRun({ ...f.a, liveConfig: 'unread', ...patch }), { name: 'InvalidDocument', message: new RegExp(rule) });
  assert.ok(!existsSync(f.a.out)); assert.ok(!existsSync(f.a.journal));
});
test('configuration refusals precede journal creation and legacy headers stay unchanged', async t => {
  for (const patch of [c => { delete c.authorize; }, c => { c.bindings = []; }]) {
    const f = await fixture(t); const config = json(f.a.liveConfig); patch(config); f.write('live.json', config);
    const r = await f.e.liveRun(f.a); expect(r, 'invalid_input', 4);
    assert.match(Buffer.from(r.out).toString(), /live-config/);
    assert.ok(!existsSync(f.a.journal)); assert.equal(f.calls.length, 0);
  }
  const f = await fixture(t);
  const first = await f.e.liveRun(f.a); expect(first, 'evaluated_live', 0);
  const lines = readFileSync(f.a.journal, 'utf8').split('\n');
  const header = JSON.parse(lines[0]); delete header.journal_id; delete header.created_at_ms;
  lines[0] = JSON.stringify(header); writeFileSync(f.a.journal, lines.join('\n'));
  const legacy = await f.e.liveRun({ ...f.a, runId: 'legacy', out: f.path('legacy') });
  expect(legacy, 'evaluated_live', 0); assert.equal(legacy.journal.created, 'legacy');
  assert.equal(legacy.journal.id, null); assert.equal(legacy.journal.createdAtMs, null);
  assert.equal(readFileSync(f.a.journal, 'utf8').split('\n')[0], lines[0]);
  header.budget = 'production'; writeFileSync(f.a.journal, JSON.stringify(header) + '\n');
  const refused = await f.e.liveRun({ ...f.a, runId: 'refused', out: f.path('refused') });
  expect(refused, 'invalid_input', 4); assert.match(Buffer.from(refused.out).toString(), /journal-budget/);
});
