// Spec 063 increment 3a (3.5.2): `sources` on compile and load, with a
// SYNTHETIC graded-input definition (spec 032's evidence-quality shape).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const rustev = createRequire(import.meta.url)('../index.js');

const grade = (path, max_grade) => ({ path, max_grade, reliability: 'reliable' });
const sources = [
  { schema: 'rustev.source/1', name: 'catalogue', version: '1.0.0',
    facts: [{ name: 'hotels', provenance: 'system-of-record', grades: [grade('', 'cached')] }] },
  { schema: 'rustev.source/1', name: 'live-quote', version: '1.0.0',
    facts: [{ name: 'quotes', provenance: 'third-party', grades: [grade('', 'authoritative')] }] },
];
const text = n => ({ text: { max_bytes: n } });
const definition = {
  schema: 'rustev.definition/2', name: 'graded-lodging', version: '0.1.0', package: 'synthetic',
  inputs: [
    { name: 'hotels', type: { list: { max_items: 20, item: { record: { fields: [
      { name: 'id', type: text(16) }, { name: 'price', type: 'decimal' }] } } } },
      provenance: ['system-of-record'], freshness: 'not_required',
      evidence: { graded: { sources: ['catalogue'], caps: [] } } },
    { name: 'quotes', type: { map: { key_max_bytes: 16, max_entries: 20, value: { record: { fields: [
      { name: 'price', type: { nullable: { type: 'decimal' } } }],
      present_when: { any_non_null: ['price'] } } } } },
      provenance: ['third-party'], freshness: { max_age_ms: 600000 },
      evidence: { graded: { sources: ['live-quote'], caps: [] } } },
  ],
  steps: [
    { id: 'joined', body: { exact: { op: 'join_keyed', version: 1, args: {
      left: 'input:hotels', key: 'id', right: 'input:quotes' } } } },
  ],
  policy: { on_unresolved: [], rules: [
    { id: 'fallback', when: 'always', then: { escalate: { reason: 'review' } } }], adjustments: [] },
  limits: { max_semantic_requests: 0, on_excess: 'refuse', max_projection_bytes: 0,
    distribution_tolerance: '0.000001', deadline_ms: 1000 },
};

async function engine(t) {
  const e = await rustev.Engine.create({
    clock: 'manual', runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1000 },
    sink: { queueCapacity: 1, acceptsRun3: true, deliver: async () => 'ack' },
  });
  t.after(() => e.shutdown({ drainMs: 1000 }));
  return e;
}

test('a graded definition compiles with its sources, async and sync, to one PlanId', async t => {
  const e = await engine(t);
  const a = await e.compile({ definition, sources });
  const b = e.compileSync({ definition, sources: sources.map(s => Buffer.from(JSON.stringify(s))) });
  assert.match(a.planId, /\S/);
  assert.equal(a.planId, b.planId);
  // Order of the supplied sources is not identity.
  const c = await e.compile({ definition, sources: [...sources].reverse() });
  assert.equal(c.planId, a.planId);
});

test('a graded definition compiled without its sources is CompileRefusal source_binding', async t => {
  const e = await engine(t);
  for (const args of [{ definition }, { definition, sources: [] }, { definition, sources: [sources[0]] }]) {
    await assert.rejects(e.compile(args), err => {
      assert.ok(err instanceof rustev.CompileRefusal, String(err));
      assert.equal(err.code, 'source_binding');
      return true;
    });
    assert.throws(() => e.compileSync(args), err => err instanceof rustev.CompileRefusal && err.code === 'source_binding');
  }
});

test('a compiled graded plan loads against its sources and is refused without them', async t => {
  const e = await engine(t);
  const compiled = await e.compile({ definition, sources });
  const plan = compiled.raw;
  const loaded = await e.compile({ plan, sources });
  assert.equal(loaded.planId, compiled.planId);
  assert.equal(e.compileSync({ plan, sources }).planId, compiled.planId);
  await assert.rejects(e.compile({ plan }), err => err instanceof rustev.CompileRefusal && err.code === 'load');
  assert.throws(() => e.compileSync({ plan, sources: [sources[1]] }), err => err instanceof rustev.CompileRefusal);
});

test('an invalid source descriptor is InvalidDocument before compiling', async t => {
  const e = await engine(t);
  const bad = { ...sources[0], unknown: true };
  await assert.rejects(e.compile({ definition, sources: [bad, sources[1]] }), err => err instanceof rustev.InvalidDocument);
  assert.throws(() => e.compileSync({ definition, sources: [{ schema: 'rustev.source/1' }] }), err => err instanceof rustev.InvalidDocument);
});

// ---- the acquisition source seam (3.4, 3.5) and its host-seam cases (3.13.3)

const catalog = (name = 'catalog') => ({
  schema: 'rustev.source/1', name, version: '1.0.0',
  operations: [{ name: 'search', cancellation: 'acknowledges',
    request: [{ name: 'q', type: { text: { max_bytes: 512 } }, required: true }], max_request_bytes: 2048,
    item: [{ name: 'id', type: 'integer', provenance: 'system-of-record', max_grade: 'cached',
      reliability: 'reliable', freshness: 'not_required' }],
    max_page_items: 4, max_page_bytes: 1024, paging: 'offset',
    cost: { bounded: { max_units: 3 } }, idempotent: true, cache: 'not_cacheable' }],
});
const acquiring = (attempts = 1, retryOn = []) => ({
  schema: 'rustev.definition/2', name: 'acquiring', version: '0.1.0', package: 'synthetic', inputs: [],
  steps: [{ id: 'lookup', body: { acquire: {
    source: 'catalog', operation: 'search', request: { q: { lit: { text: 'tea' } } }, when: 'always',
    limits: { max_pages: 2, max_items: 5, max_attempts: attempts, retry_on: retryOn, delay: 'none',
      deadline_share: '0.2', as_of_skew_ms: 100 },
    stop: 'none', on_failure: 'unresolved' } } }],
  policy: { on_unresolved: [], rules: [
    { id: 'fallback', when: 'always', then: { escalate: { reason: 'review' } } }], adjustments: [] },
  limits: { max_semantic_requests: 0, on_excess: 'refuse', max_projection_bytes: 0,
    distribution_tolerance: '0.000001', deadline_ms: 100,
    acquisition: { max_calls: 8, max_items: 20, max_bytes: 16384 } },
});
const emptySnapshot = { schema: 'rustev.snapshot/1', entries: [] };
const charged = { observed: { units: 1 } };
const pageFor = (call, ids = [1], extra = {}) => ({
  schema: 'rustev.acquisition-page/1', request: call.requestId, as_of_ms: 900,
  items: ids.map(id => ({ id })), next: 'none', ...extra,
});
const tick = () => new Promise(r => setImmediate(r));
async function until(what, cond) {
  for (let i = 0; i < 2000; i++) { if (cond()) return; await tick(); }
  throw new Error(`waited forever for ${what}`);
}

async function sourced(t, acquire, { queueCapacity = 8, maxConcurrency = 2, attempts = 1, retryOn = [], maxInFlight = 1, sources: extra } = {}) {
  const calls = [];
  const e = await rustev.Engine.create({
    clock: 'manual', runtime: { maxInFlight, maxQueued: 0, maxParallelRequests: 2, sinkTimeoutMs: 1000 },
    sink: { queueCapacity: 8, acceptsRun3: true, deliver: async () => 'ack' },
    acquisitionSources: extra ?? [{ descriptor: catalog(), maxConcurrency, queueCapacity,
      acquire(call) { calls.push(call); return acquire(call, calls.length); } }],
  });
  t.after(() => e.shutdown({ drainMs: 1000 }));
  const p = await e.compile({ definition: acquiring(attempts, retryOn), sources: [catalog()] });
  return { e, p, calls };
}
const decideOne = (p, id = 'd1', opts = {}) =>
  p.decide({ decisionId: id, snapshot: emptySnapshot, evaluationTimeMs: 1000 }, { principalHandle: Buffer.from('tenant/principal'), ...opts });
const acq = r => r.evidence.acquisitions[0];
const ends = r => acq(r).attempts.map(a => a.end);
const gauge = (e, name = 'acquisition:catalog') => e.gauges().seams.find(s => s.name === name);

test('a page crosses unchanged and the call carries the request, its identity and the window', async t => {
  const { e, p, calls } = await sourced(t, call => ({ page: pageFor(call, [1, 2]), charge: charged }));
  const r = await decideOne(p, 'page/1');
  assert.equal(r.completion, 'judged');
  assert.deepEqual(ends(r), ['page']);
  const c = calls[0];
  assert.equal(c.decisionId, 'page/1');
  assert.equal(c.operation, 'search');
  const request = JSON.parse(Buffer.from(c.request));
  assert.equal(request.schema, 'rustev.acquisition-request/1');
  assert.equal(request.operation, 'search');
  assert.match(c.requestId, /\S/);
  assert.ok(c.attemptId.startsWith('page%2F1/lookup/'), c.attemptId);
  assert.equal(c.remainingMs, 20);
  assert.equal(Buffer.from(c.principalHandle).toString(), 'tenant/principal');
  assert.ok(c.signal instanceof AbortSignal && !c.signal.aborted);
  const g = gauge(e);
  assert.deepEqual([g.queueDepth, g.refusedFull, g.lateResolutionsDiscarded, g.abandonedPending], [0, 0, 0, 0]);
});

test('denied is never retried and is missing evidence, not an exception', async t => {
  const { p, calls } = await sourced(t, () => {
    throw new rustev.SeamFailure({ failureClass: 'denied', charge: { observed: { units: 0 } }, remote: 'finished', detail: 'no' });
  }, { attempts: 2, retryOn: ['transient'] });
  const r = await decideOne(p);
  assert.equal(calls.length, 1);
  assert.deepEqual(ends(r), ['denied']);
  assert.ok(JSON.stringify(acq(r).result).includes('missing_evidence'), JSON.stringify(acq(r).result));
});

test('a named class retries; a synchronous throw is permanent; a non-Promise page is a resolved one', async t => {
  {
    const { p, calls } = await sourced(t, (call, n) => {
      if (n === 1) return Promise.reject(new rustev.SeamFailure({ failureClass: 'transient', charge: charged, remote: 'finished' }));
      return { page: pageFor(call), charge: charged };
    }, { attempts: 2, retryOn: ['transient'] });
    assert.deepEqual(ends(await decideOne(p)), ['transient', 'page']);
    assert.equal(calls.length, 2);
  }
  {
    // A null charge on a page is a missing charge: permanent, never "null".
    const { p } = await sourced(t, call => ({ page: pageFor(call), charge: null }));
    const r = await decideOne(p);
    assert.deepEqual(ends(r), ['permanent']);
    assert.equal(acq(r).attempts[0].cost.charge, 'unknown');
  }
  {
    const { p } = await sourced(t, () => { throw new Error('boom'); }, { attempts: 2, retryOn: ['transient'] });
    const r = await decideOne(p);
    assert.deepEqual(ends(r), ['permanent']);
    assert.equal(JSON.stringify(acq(r).attempts[0].cost.charge), '"unknown"');
  }
});

test('an invalid page, or one over 4 MiB, is invalid_page', async t => {
  const big = 'x'.repeat(4 * 1024 * 1024);
  for (const make of [call => pageFor(call, [1], { unknown: true }), call => pageFor(call, [1], { pad: big })]) {
    const { p } = await sourced(t, call => ({ page: make(call), charge: charged }));
    const r = await decideOne(p);
    assert.deepEqual(ends(r), ['invalid_page']);
    assert.ok(JSON.stringify(acq(r).result).includes('invalid_backend_output'), JSON.stringify(acq(r).result));
  }
});

test('a full queue is overloaded with nothing charged, at once', async t => {
  const held = [];
  const { e, p } = await sourced(t, call => new Promise(res => held.push(() => res({ page: pageFor(call), charge: charged }))),
    { queueCapacity: 1, maxInFlight: 2 });
  const first = decideOne(p, 'first');
  await until('the first call', () => held.length === 1);
  const second = await decideOne(p, 'second');
  assert.deepEqual(ends(second), ['overloaded']);
  assert.deepEqual(acq(second).attempts[0].cost.charge, { observed: { units: 0 } });
  assert.equal(gauge(e).refusedFull, 1);
  held[0]();
  assert.deepEqual(ends(await first), ['page']);
});

test('a never-settling Promise ends at the window, is aborted and counted abandoned; a late settlement is discarded', async t => {
  let settle;
  const { e, p, calls } = await sourced(t, () => new Promise(res => { settle = res; }));
  const r = decideOne(p);
  await until('the call', () => calls.length === 1);
  await until('the window timer', () => e.clockDelays().includes(20));
  e.advanceClock(20);
  const done = await r;
  assert.deepEqual(ends(done), ['deadline']);
  assert.ok(JSON.stringify(acq(done).result).includes('deadline_exceeded'));
  await until('the abort', () => calls[0].signal.aborted);
  let g = gauge(e);
  assert.equal(g.abandonedPending, 1);
  assert.equal(g.queueDepth, 0, 'no slot held');
  settle({ page: pageFor(calls[0]), charge: charged });
  await until('the late discard', () => gauge(e).lateResolutionsDiscarded === 1);
  g = gauge(e);
  assert.equal(g.abandonedPending, 0);
});

test('shutdown with a pending callback reports it abandoned', async t => {
  const { e, p, calls } = await sourced(t, () => new Promise(() => {}));
  const r = decideOne(p).catch(err => err);
  await until('the call', () => calls.length === 1);
  const s = await e.shutdown({ drainMs: 0 });
  assert.equal(s.cancelled, 1);
  assert.equal(s.abandonedPending, 1);
  const out = await r;
  assert.ok(out.completion === 'cancelled' || out instanceof Error, String(out));
  assert.ok(calls[0].signal.aborted);
});

test('sources: repeated names and bad bounds refuse before any thread; an uninstalled source is source_binding', async t => {
  const base = { clock: 'manual', runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1000 },
    sink: { queueCapacity: 1, acceptsRun3: true, deliver: async () => 'ack' } };
  const src = (over = {}) => ({ descriptor: catalog(), maxConcurrency: 1, queueCapacity: 1, acquire: async () => ({}), ...over });
  await assert.rejects(rustev.Engine.create({ ...base, acquisitionSources: [src(), src()] }),
    err => err instanceof rustev.ConfigRefused && err.code === 'invalid_source');
  await assert.rejects(rustev.Engine.create({ ...base, acquisitionSources: [src({ descriptor: { schema: 'rustev.source/1' } })] }),
    err => err instanceof rustev.ConfigRefused && err.code === 'invalid_source');
  for (const over of [{ queueCapacity: 0 }, { maxConcurrency: 65537 }]) {
    await assert.rejects(rustev.Engine.create({ ...base, acquisitionSources: [src(over)] }),
      err => err instanceof rustev.ConfigRefused && err.code === 'out_of_range');
  }
  const bare = await rustev.Engine.create(base);
  t.after(() => bare.shutdown({ drainMs: 1000 }));
  await assert.rejects(bare.compile({ definition: acquiring(), sources: [catalog()] }),
    err => err instanceof rustev.PlanPrepareRefused && err.code === 'source_binding');
  const other = await rustev.Engine.create({ ...base, acquisitionSources: [src({ descriptor: { ...catalog(), version: '1.0.1' } })] });
  t.after(() => other.shutdown({ drainMs: 1000 }));
  await assert.rejects(other.compile({ definition: acquiring(), sources: [catalog()] }),
    err => err instanceof rustev.PlanPrepareRefused && err.code === 'source_binding');
  await assert.rejects(other.compile({ definition: acquiring() }),
    err => err instanceof rustev.CompileRefusal && err.code === 'source_binding');
});
