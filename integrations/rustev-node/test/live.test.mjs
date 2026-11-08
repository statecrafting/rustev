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
async function fixture(t, { model = 'bounded', infer, cap = 1000, journalPath, parallel = 3, queueCapacity = 8, costUnit = 'nano_usd' } = {}) {
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
  write('live.json', { schema: 'rustev.live-config/1', budget: 'testing', run_cap_units: cap, repeats: 1, evaluation_time: { fixed: { ms: NOW } }, bindings: [{ identity: { backend_id: descriptor.backend_id, artifact: descriptor.artifact }, cost_unit: costUnit, max_in_flight: 1, min_interval_ms: 0, case_deadline_ms: 1000, rate_limit_pause_max_ms: 0 }], acquisitions: {}, probe: { calls: 1, interval_ms: 0, snapshot: path('probe.json'), digest: digest(probe), provenance: 'synthetic' }, authorize: [{ backend_id: descriptor.backend_id, artifact: descriptor.artifact, operations: ['classify', 'proposition', 'rubric'], max_exposure_units: 100 }] });
  const calls = [];
  const journals = new Map();
  const e = await rustev.Engine.create({ clock: 'manual', runtime: { maxInFlight: 2, maxQueued: 0, maxParallelRequests: parallel, sinkTimeoutMs: 1000 }, sink: { queueCapacity: 8, deliver: async () => 'ack' }, backends: [{ descriptor, costModel: model, costBound: model === 'unknown' ? 'unknown' : { [model]: model === 'bounded' ? { max_units: 5 } : { units: 5 } }, maxConcurrency: 8, queueCapacity, infer: async c => {
    const decisionId = decodeURIComponent(c.attemptId.split('/')[0]);
    const lines = readFileSync(journals.get(decisionId.split('/')[1]) ?? journalPath ?? path('journal'), 'utf8');
    c = { ...c, decisionId };
    assert.ok(lines.includes(c.attemptId), 'reservation must precede host dispatch');
    calls.push(c);
    return infer ? infer(c) : answer(c);
  } }] });
  t.after(async () => { await e.shutdown(); rmSync(dir, { recursive: true, force: true }); });
  const a = { live: true, liveConfig: path('live.json'), journal: path('journal'), runId: 'synthetic', dataset: path('dataset.json'), split: 'training', config: path('config.json'), adapter: path('adapter.json'), scope: { mode: 'tenant_only', tenant: 'synthetic', context_revision: '1' }, nowMs: NOW, repeatConfig: path('repeat.json'), candidates: path('candidates'), snapshots: path('snapshots'), out: path('out') };
  return { e, a, path, calls, write, journals };
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

// Every dispatch observes both the durable reservation and, on rerun, its checkpoint.
const journalLines = f => readFileSync(f.a.journal, 'utf8').trim().split('\n').map(JSON.parse);
async function interrupted(t) {
  let settle;
  let f;
  f = await fixture(t, { parallel: 1, infer: c => {
    if (c.decisionId === 'live/synthetic/a/1/two') return new Promise(resolve => { settle = () => resolve(answer(c)); });
    if (c.decisionId.endsWith('/rerun-1')) {
      const checkpoint = json(f.path('out/checkpoint.json'));
      assert.ok(checkpoint.items.some(i => i.states.some(s => s.rerun?.k === 1)), 'rerun checkpoint precedes dispatch');
    }
    return answer(c);
  } });
  const dataset = json(f.a.dataset);
  dataset.cases.push({ ...dataset.cases[0], id: 'two', source: 'two' });
  f.write('dataset.json', dataset);
  f.write('snapshots/two.json', readFileSync(f.path('snapshots/one.json')));
  const controller = new AbortController();
  const pending = f.e.liveRun({ ...f.a, signal: controller.signal });
  await until(() => !!settle);
  controller.abort();
  expect(await pending, 'interrupted', 16);
  return { ...f, settle };
}
test('resume preserves completed items; interrupted items require explicit rerun', async t => {
  let f = await interrupted(t);
  const completed = f.calls.filter(c => c.decisionId.endsWith('/one')).length;
  assert.ok(completed > 0);
  const old = journalLines(f).filter(l => l.op === 'reserve' && decodeURIComponent(l.attempt).includes('/two/'));
  assert.equal(old.length, 1);
  expect(await f.e.liveRun({ ...f.a, resume: true }), 'evaluated_live', 0);
  assert.equal(f.calls.filter(c => c.decisionId.endsWith('/one')).length, completed);
  assert.equal(f.calls.filter(c => c.decisionId.endsWith('/two')).length, 1);
  assert.match(readFileSync(f.path('out/a/run-0.detail.json'), 'utf8'), /live-interrupted/);
  const liability = json(f.path('out/live-run.json')).journal_after.liability;
  assert.ok(liability > 0);
  f.settle();
  f = await interrupted(t);
  expect(await f.e.liveRun({ ...f.a, resume: true, rerunInterrupted: true }), 'evaluated_live', 0);
  assert.ok(f.calls.some(c => c.decisionId === 'live/synthetic/a/1/two/rerun-1'));
  assert.equal(f.calls.filter(c => c.decisionId.endsWith('/one')).length, completed);
  const lines = journalLines(f);
  assert.ok(lines.some(l => l.op === 'reserve' && decodeURIComponent(l.attempt).includes('/two/rerun-1/')));
  const report = json(f.path('out/live-run.json'));
  assert.equal(report.journal_after.liability, liability);
  const rerun = json(f.path('out/cases/a.1.two.rerun-1.case.json'));
  assert.equal(rerun.run.rerun, 1);
  assert.ok(rerun.spend.length > 0);
  assert.ok(report.journal_after.observed > report.journal_before.observed);
  const before = readFileSync(f.a.journal);
  f.settle();
  await until(() => f.e.gauges().seams.some(g => g.lateResolutionsDiscarded > 0));
  assert.deepEqual(readFileSync(f.a.journal), before);
});
test('resume rejects changed configuration before journal lock and preserves bytes', async t => {
  const f = await fixture(t);
  expect(await f.e.liveRun(f.a), 'evaluated_live', 0);
  expect(await f.e.liveRun(f.a), 'io_error', 3);
  const before = readFileSync(f.a.journal);
  const config = json(f.a.liveConfig); config.run_cap_units += 1;
  f.write('live.json', config);
  writeFileSync(f.a.journal + '.lock', 'held');
  const r = await f.e.liveRun({ ...f.a, resume: true });
  expect(r, 'invalid_input', 4); assert.match(Buffer.from(r.out).toString(), /checkpoint-mismatch/);
  assert.deepEqual(readFileSync(f.a.journal), before);
  assert.equal(readFileSync(f.a.journal + '.lock', 'utf8'), 'held');
  // A malformed header would fail if prepare reached its journal read.
  writeFileSync(f.a.journal, 'unreadable header');
  const unread = await f.e.liveRun({ ...f.a, resume: true });
  expect(unread, 'invalid_input', 4); assert.match(Buffer.from(unread.out).toString(), /checkpoint-mismatch/);
  assert.equal(readFileSync(f.a.journal, 'utf8'), 'unreadable header');
});
test('resume rejects a replacement journal and a reached cap remains reached', async t => {
  const f = await fixture(t);
  const first = await f.e.liveRun(f.a);
  rmSync(f.a.journal);
  const replacement = await f.e.liveRun({ ...f.a, runId: 'replacement', out: f.path('replacement') });
  expect(replacement, 'evaluated_live', 0);
  assert.equal(replacement.journal.created, 'this_run');
  assert.notEqual(first.journal.id, replacement.journal.id);
  assert.equal(replacement.journal.spendBefore, 0);
  const before = readFileSync(f.a.journal);
  const r = await f.e.liveRun({ ...f.a, resume: true });
  expect(r, 'invalid_input', 4); assert.match(Buffer.from(r.out).toString(), /checkpoint-mismatch/);
  assert.deepEqual(readFileSync(f.a.journal), before);
  const g = await fixture(t, { cap: 5, infer: c => ({ ...answer(c), charge: { observed: { units: 5 } } }) });
  expect(await g.e.liveRun(g.a), 'spend_capped', 14);
  assert.ok(g.calls.length > 0);
  const calls = g.calls.length;
  expect(await g.e.liveRun({ ...g.a, resume: true }), 'spend_capped', 14);
  assert.equal(g.calls.length, calls);
});
for (const drainMs of [0, 10000]) test(`shutdown (${drainMs}ms) cancels all live runs, writes checkpoints and frees locks`, async t => {
  const settles = [];
  const f = await fixture(t, { parallel: 1, infer: c => new Promise(resolve => { settles.push(() => resolve(answer(c))); }) });
  const second = { ...f.a, runId: 'second', out: f.path('second'), journal: f.path('second-journal') };
  f.journals.set('second', second.journal);
  const runs = [f.e.liveRun(f.a), f.e.liveRun(second)];
  await until(() => settles.length === 2);
  const shutdown = f.e.shutdown({ drainMs });
  // Both signals must be raised promptly, without spending the drain budget.
  const end = Date.now() + 2000;
  while (!f.calls.every(c => c.signal.aborted)) {
    assert.ok(Date.now() < end, 'live cancellation must precede draining');
    await new Promise(setImmediate);
  }
  const stopped = await shutdown;
  for (const a of [f.a, second]) assert.ok(existsSync(join(a.out, 'checkpoint.json')));
  for (const r of await Promise.all(runs)) expect(r, 'interrupted', 16);
  assert.equal(stopped.abandonedPending, f.e.gauges().seams.reduce((n, g) => n + (g.abandonedPending ?? 0), 0));
  assert.ok(stopped.abandonedPending >= 2);
  assert.ok(f.e.gauges().seams.some(g => g.abandonedPending >= 2));
  assert.equal(stopped.cancelled, 2);
  assert.equal(stopped.stillRunning, 0);
  await assert.rejects(f.e.liveRun({ ...f.a, resume: true }), { name: 'EngineShutDown' });
  for (const a of [f.a, second]) {
    assert.ok(existsSync(join(a.out, 'checkpoint.json')));
    assert.ok(!existsSync(a.journal + '.lock'));
    const report = json(join(a.out, 'live-run.json'));
    assert.ok(report.journal_after.liability + report.journal_after.outstanding > 0);
  }
  const before = readFileSync(f.a.journal);
  for (const settle of settles) settle();
  await until(() => f.e.gauges().seams.some(g => g.lateResolutionsDiscarded >= 2));
  assert.deepEqual(readFileSync(f.a.journal), before);
  assert.ok(f.e.gauges().seams.every(g => !g.abandonedPending));
  const next = await fixture(t, { journalPath: f.a.journal });
  expect(await next.e.liveRun({ ...f.a, resume: true }), 'evaluated_live', 0);
});

test('a concurrent live run refuses a locked journal without dispatching', async t => {
  let release;
  let gated = false;
  const f = await fixture(t, { parallel: 1, infer: c => {
    if (gated) return answer(c);
    gated = true;
    return new Promise(resolve => { release = () => resolve(answer(c)); });
  } });
  const first = f.e.liveRun(f.a);
  try {
    await until(() => !!release);
    const before = readFileSync(f.a.journal);
    const calls = f.calls.length;
    const second = await f.e.liveRun({ ...f.a, runId: 'contender', out: f.path('contender') });
    expect(second, 'io_error', 3);
    assert.equal(JSON.parse(Buffer.from(second.out).toString()).path, 'journal-locked');
    assert.equal(f.calls.length, calls);
    assert.deepEqual(readFileSync(f.a.journal), before);
  } finally {
    release?.();
  }
  expect(await first, 'evaluated_live', 0);
});

const caseDoc = f => json(f.path('out/cases/a.1.one.case.json'));
const reportDoc = f => json(f.path('out/live-run.json'));
const itemReservations = f => journalLines(f).filter(l => l.op === 'reserve' && l.attempt.startsWith('live/synthetic/a/1/one/'));
const descriptorNames = ['synthetic-linear-head', 'synthetic-label-only', 'synthetic-linear-head-replica', 'synthetic-other-head'];
const planInputs = () => ({ plan: fx('support.plan.json'), descriptors: descriptorNames.map(n => fx(`${n}.descriptor.json`)), calibrations: [fx('topic.calibration.json')] });

test('a thrown SeamFailure settles its observed charge without liability', async t => {
  const f = await fixture(t, { infer: c => {
    if (c.decisionId.endsWith('/one') && c.attemptId.includes('/topic/')) {
      throw new rustev.SeamFailure({ failureClass: 'permanent', detail: 'SYNTHETIC charged failure', charge: { observed: { units: 2 } }, remote: 'finished' });
    }
    return answer(c);
  } });
  expect(await f.e.liveRun(f.a), 'evaluated_live', 0);
  const ticket = itemReservations(f).find(l => l.attempt.includes('/topic/'));
  assert.ok(ticket);
  assert.ok(journalLines(f).some(l => l.op === 'settle' && l.seq === ticket.seq && l.charge?.observed?.units === 2));
  assert.deepEqual(caseDoc(f).spend.find(s => s.seq === ticket.seq).settlement, { observed: { units: 2 } });
  assert.equal(reportDoc(f).journal_after.observed, 7);
  assert.equal(reportDoc(f).journal_after.liability, 0);
});

test('a no-spend binding reserves zero and declares it in case and run documents', async t => {
  const f = await fixture(t, { costUnit: 'none', infer: c => ({ ...answer(c), charge: { observed: { units: 0 } } }) });
  expect(await f.e.liveRun(f.a), 'evaluated_live', 0);
  const identity = json(f.a.liveConfig).bindings[0].identity;
  assert.deepEqual(reportDoc(f).declared_no_spend, [identity]);
  assert.deepEqual(caseDoc(f).declared_no_spend, [identity]);
  const reservations = journalLines(f).filter(l => l.op === 'reserve');
  assert.equal(reservations.length, 6);
  assert.ok(reservations.every(l => l.units === 0));
  assert.ok(caseDoc(f).spend.every(s => s.reserved_units === 0 && s.settlement.observed.units === 0));
});

test('estimated exposure is reserved in full before the host observes each call', async t => {
  let f;
  f = await fixture(t, { model: 'estimated', infer: c => {
    const reservation = journalLines(f).find(l => l.op === 'reserve' && l.attempt.endsWith('/' + c.attemptId));
    assert.ok(reservation);
    assert.equal(reservation.units, 5);
    assert.ok(!journalLines(f).some(l => l.op === 'settle' && l.seq === reservation.seq));
    return answer(c);
  } });
  expect(await f.e.liveRun(f.a), 'evaluated_live', 0);
  assert.equal(itemReservations(f).length, 3);
  assert.ok(caseDoc(f).spend.every(s => s.reserved_units === 5));
  assert.equal(reportDoc(f).journal_after.observed, 6);
  assert.equal(reportDoc(f).journal_after.liability, 0);
});

test('a retryable failure reserves a new ticket for the successful retry', async t => {
  const f = await fixture(t, { parallel: 1, infer: c => {
    if (c.decisionId.endsWith('/one') && c.attemptId.endsWith('/topic//1')) {
      throw new rustev.SeamFailure({ failureClass: 'transient', detail: 'SYNTHETIC retry', charge: { observed: { units: 1 } }, remote: 'finished' });
    }
    return answer(c);
  } });
  const inputs = planInputs();
  const definition = JSON.parse(inputs.plan).definition;
  const execution = { schema: 'rustev.execution/1', max_attempts_per_decision: 10, cost: 'unlimited', steps: [{ step: 'topic', retry: { max_attempts: 2, on: ['transient'], delay: 'none' }, attempt_timeout: 'none', fallback: { backends: [], on: [] } }] };
  const p = await f.e.compile({ definition, execution, descriptors: inputs.descriptors, calibrations: inputs.calibrations });
  f.write('candidates/a/plan.json', Buffer.from(p.raw));
  expect(await f.e.liveRun(f.a), 'evaluated_live', 0);
  const tickets = itemReservations(f).filter(l => l.attempt.includes('/topic/'));
  assert.equal(tickets.length, 2);
  assert.notEqual(tickets[0].seq, tickets[1].seq);
  assert.ok(tickets[0].attempt.endsWith('//1')); assert.ok(tickets[1].attempt.endsWith('//2'));
  for (const ticket of tickets) assert.deepEqual(caseDoc(f).spend.find(s => s.seq === ticket.seq).settlement, { observed: { units: 1 } });
  assert.equal(reportDoc(f).journal_after.observed, 7);
  assert.equal(reportDoc(f).journal_after.liability, 0);
});

// 067 3.3.5 says a full seam queue releases its ticket. The seam reports the
// refused attempt as overloaded with an observed charge of zero (027 3.6.2),
// and 043 3.6.6 settles a ticket with the charge its attempt ends with, so
// the journal records a zero settlement: nothing sent, nothing spent, no
// liability, the same totals as a release (recorded in the 067 record).
test('a full backend seam queue sends nothing and spends nothing', async t => {
  let release;
  let gated = false;
  const f = await fixture(t, { queueCapacity: 1, parallel: 1, infer: c => {
    if (gated) return answer(c);
    gated = true;
    return new Promise(resolve => { release = () => resolve(answer(c)); });
  } });
  const first = f.e.liveRun(f.a);
  try {
    await until(() => !!release);
    const second = { ...f.a, runId: 'second', out: f.path('second'), journal: f.path('second-journal') };
    f.journals.set('second', second.journal);
    const calls = f.calls.length;
    expect(await f.e.liveRun(second), 'probe_failed', 15);
    assert.equal(f.calls.length, calls);
    const lines = readFileSync(second.journal, 'utf8').trim().split('\n').map(JSON.parse);
    const reservations = lines.filter(l => l.op === 'reserve');
    assert.equal(reservations.length, 3);
    const doc = json(join(second.out, 'probe/1.1.case.json'));
    const report = json(join(second.out, 'live-run.json'));
    assert.equal(report.journal_after.observed, 0); assert.equal(report.journal_after.liability, 0);
    const ends = lines.filter(l => l.op !== 'reserve' && l.seq !== undefined);
    assert.equal(ends.length, reservations.length);
    assert.deepEqual(ends.map(l => l.seq).sort(), reservations.map(l => l.seq).sort());
    for (const end of ends) {
      assert.equal(end.op, 'settle');
      assert.deepEqual(end.charge, { observed: { units: 0 } });
    }
    assert.deepEqual(doc.spend.map(s => s.seq).sort(), reservations.map(l => l.seq).sort());
    for (const spend of doc.spend) assert.deepEqual(spend.settlement, { observed: { units: 0 } });
  } finally {
    release?.();
    expect(await first, 'evaluated_live', 0);
  }
});

test('a live run accepts an embedded replay bundle and an admission record', async t => {
  const historical = await rustev.Engine.create({ clock: 'manual', runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1000 }, sink: { queueCapacity: 8, deliver: async () => 'ack' }, backends: [{ descriptor: fx('synthetic-linear-head.descriptor.json'), costModel: 'bounded', costBound: { bounded: { max_units: 5 } }, maxConcurrency: 1, queueCapacity: 8, infer: async c => answer(c) }] });
  t.after(() => historical.shutdown());
  const f = await fixture(t);
  const p = await historical.compile(planInputs());
  const r = await p.decide({ decisionId: 'historical', snapshot: fx('support.snapshot.json'), evaluationTimeMs: NOW }, { capture: { scope: f.a.scope, maxBytes: 16777216, createdAtMs: NOW, retain: 'embedded' } });
  assert.equal(r.capture.status, 'complete');
  f.write('bundles/one.json', Buffer.from(r.capture.bundle));
  f.write('admission.json', { synthetic: 'approved for this test' });
  expect(await f.e.liveRun({ ...f.a, snapshots: undefined, bundles: f.path('bundles'), admission: f.path('admission.json') }), 'evaluated_live', 0);
  assert.equal(reportDoc(f).counts.completed, 1);
  assert.equal(caseDoc(f).source.replay_bundle.decision_id, 'historical');
  assert.equal(caseDoc(f).source.replay_bundle.bundle_digest, digest(JSON.parse(Buffer.from(r.capture.bundle))));
});
