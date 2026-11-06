// Spec 063 increment 4 (3.4, 3.6, 3.7): the store seam, plan.invalidate,
// the host-seam cases of 3.13.3 for the store, and the reference
// Valkey-shaped double with its non-atomic variants. SYNTHETIC content, a
// manual clock, no network access and no wall-clock sleep.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { ValkeyCluster, ValkeyStore } from './valkey-double.mjs';
const rustev = createRequire(import.meta.url)('../index.js');
const fx = (dir, n) => readFileSync(new URL(`../../../tools/rustev-conformance/fixtures/${dir}/${n}`, import.meta.url));
const NOW = 1790164800000;

const term = (t) => `SYNTHETIC ${t} term`;
const contract = {
  schema: term('schema'), namespace: term('namespace'), durability: term('durability'),
  encryption: term('encryption'), atomicity: term('atomicity'), conflict: 'first writer wins',
  expiry: term('expiry'), invalidation: term('invalidation'), erasure: term('erasure'),
};
const outputs = {
  'support.topic': { logits: { billing: 4, integration_defect: 0, account_access: 0, other: 0 } },
  'support.frustration': { logits: { calm: 3, frustrated: 0, very_angry: 0 } },
  'support.deadline': { logits: { false: 3, true: 0 } },
};
const descriptors = ['synthetic-linear-head', 'synthetic-label-only', 'synthetic-linear-head-replica', 'synthetic-other-head']
  .map((n) => fx('runtime', `${n}.descriptor.json`));
const tick = () => new Promise((r) => setImmediate(r));
async function until(what, cond) {
  for (let i = 0; i < 2000; i++) { if (cond()) return; await tick(); }
  throw new Error(`waited forever for ${what}`);
}

/** An engine over `store` with a backend answering every task; `calls`
 * counts the backend's dispatches across engines sharing it. */
async function engine(t, store, { plan = 'store.plan.json', calls = { backend: 0 }, maxInFlight = 1 } = {}) {
  const e = await rustev.Engine.create({
    clock: 'manual', runtime: { maxInFlight, maxQueued: 0, maxParallelRequests: 3, sinkTimeoutMs: 20000 },
    sink: { queueCapacity: 8, deliver: async () => 'ack' },
    backends: [{ descriptor: fx('runtime', 'synthetic-linear-head.descriptor.json'), costModel: 'bounded',
      costBound: { bounded: { max_units: 5 } }, maxConcurrency: 3, queueCapacity: 8,
      infer: async (call) => { calls.backend++; return { output: outputs[JSON.parse(Buffer.from(call.projection)).task], charge: { observed: { units: 1 } } }; } }],
    ...(store ? { store } : {}),
  });
  t.after(() => e.shutdown({ drainMs: 1000 }));
  const dir = plan.startsWith('support') ? 'runtime' : 'store';
  const p = await e.compile({ plan: fx(dir, plan), descriptors, calibrations: [fx('runtime', 'topic.calibration.json')] });
  return { e, p, calls };
}
const decide = (p, id, at = NOW, opts = {}) =>
  p.decide({ decisionId: id, snapshot: fx('runtime', 'support.snapshot.json'), evaluationTimeMs: at }, opts);
const outcomes = (r) => r.evidence.requests.map((q) => q.optimization.outcome);
const every = (r, o) => outcomes(r).length === 3 && outcomes(r).every((x) => x === o);
const diagnosed = (r, needle) => r.evidence.requests.every((q) => q.optimization.diagnostics.some((d) => d.includes(needle)));
const gauge = (e) => e.gauges().seams.find((s) => s.name === 'store');

/** A store whose methods are given, the rest of an atomic double behind. */
function scripted(over = {}) {
  const inner = new ValkeyStore(new ValkeyCluster(), contract);
  const st = { contract, queueCapacity: 8, calls: inner.calls };
  for (const m of ['get', 'put', 'delete', 'invalidate']) st[m] = over[m] ?? ((c) => inner[m](c));
  if (over.queueCapacity) st.queueCapacity = over.queueCapacity;
  return st;
}

test('the store contract is checked before any thread; prepare keeps store_missing and store_contract_mismatch', async (t) => {
  const base = { clock: 'manual', runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1000 },
    sink: { queueCapacity: 1, deliver: async () => 'ack' } };
  const { erasure, ...noErasure } = contract;
  for (const c of [noErasure, { ...contract, erasure: '' }, { ...contract, extra: 'x' }, null]) {
    await assert.rejects(rustev.Engine.create({ ...base, store: { ...scripted(), contract: c } }),
      (err) => err instanceof rustev.ConfigRefused && err.code === 'invalid_store_contract', JSON.stringify(c));
  }
  await assert.rejects(rustev.Engine.create({ ...base, store: { ...scripted(), queueCapacity: 0 } }),
    (err) => err instanceof rustev.ConfigRefused && err.code === 'out_of_range');
  await assert.rejects(engine(t, undefined), (err) => err instanceof rustev.PlanPrepareRefused && err.code === 'store_missing');
  await assert.rejects(engine(t, { ...scripted(), contract: { ...contract, conflict: 'last writer wins' } }),
    (err) => err instanceof rustev.PlanPrepareRefused && err.code === 'store_contract_mismatch');
});

test('a restarted engine hits the same Valkey store; an advanced generation misses', async (t) => {
  const cluster = new ValkeyCluster();
  // Entries stay on invalidation (G2 makes removal optional), so the
  // advanced generation is what turns them into misses.
  const store = new ValkeyStore(cluster, contract, { removeOnInvalidate: false });
  const calls = { backend: 0 };
  const cold = await decide((await engine(t, store, { calls })).p, 'cold');
  assert.ok(every(cold, 'miss'), outcomes(cold));
  assert.equal(calls.backend, 3);
  assert.equal(store.calls.put, 3);
  const warm = await decide((await engine(t, store, { calls })).p, 'warm', NOW + 1000);
  assert.ok(every(warm, 'hit'), outcomes(warm));
  assert.equal(calls.backend, 3);
  assert.equal(store.calls.put, 3, 'hits write nothing');
  assert.deepEqual(warm.judgment.outcome, cold.judgment.outcome);
  for (const q of warm.evidence.requests) assert.equal(q.optimization.age_ms, 1000);
  // Another process's invalidation, through a third engine.
  const third = await engine(t, store, { calls });
  const inv = await third.p.invalidate();
  assert.deepEqual(inv.persistent, { generation: 1, removed: 0 });
  assert.equal(inv.complete, true);
  const after = await decide((await engine(t, store, { calls })).p, 'after', NOW + 2000);
  assert.ok(every(after, 'miss') && diagnosed(after, 'generation_advanced'), outcomes(after));
  assert.equal(calls.backend, 6);
});

test('store failures follow the plan policy and are never hits', async (t) => {
  const failing = () => scripted({ get: async () => { throw new Error('SYNTHETIC get failure'); } });
  {
    const { p, calls } = await engine(t, failing(), { plan: 'store-closed.plan.json' });
    const r = await decide(p, 'closed');
    assert.ok(every(r, 'refused') && diagnosed(r, 'store_failed'), outcomes(r));
    assert.equal(calls.backend, 0);
  }
  {
    const st = failing();
    const r = await decide((await engine(t, st)).p, 'open');
    assert.ok(every(r, 'miss') && diagnosed(r, 'store_failed'), outcomes(r));
    assert.equal(st.calls.put, 0, 'a failed read captured no generations');
  }
  // Out of bounds: generation 2^53, a fractional generation, an unknown
  // put answer, bytes that are not a Uint8Array.
  for (const [over, needle] of [
    [{ get: async () => ({ bytes: null, generations: { namespace: 2 ** 53, scope: 0 } }) }, 'not a safe integer'],
    [{ get: async () => ({ bytes: null, generations: { namespace: 0.5, scope: 0 } }) }, 'not a safe integer'],
    [{ put: async () => 'maybe' }, 'store_failed'],
    [{ get: async () => ({ bytes: 'not bytes', generations: { namespace: 0, scope: 0 } }) }, 'store_failed'],
  ]) {
    const r = await decide((await engine(t, scripted(over))).p, 'bounds');
    assert.ok(every(r, 'miss') && diagnosed(r, needle), JSON.stringify(r.evidence.requests.map((q) => q.optimization.diagnostics)));
  }
});

test('a full store queue is a store failure at once; a late get is discarded and counted', async (t) => {
  let release;
  const held = new Promise((r) => { release = r; });
  const st = scripted({ queueCapacity: 1, get: async (c) => { await held; return { bytes: null, generations: { namespace: 0, scope: 0 } }; } });
  const { e, p } = await engine(t, st, { maxInFlight: 2 });
  const first = decide(p, 'first');
  await until('the held get', () => gauge(e).queueDepth === 1);
  const second = await decide(p, 'second');
  assert.ok(diagnosed(second, 'host_seam_queue_full'), JSON.stringify(second.evidence.requests.map((q) => q.optimization.diagnostics)));
  assert.ok(gauge(e).refusedFull >= 1);
  // The first decision's deadline passes on the manual clock with its get
  // unsettled: a store failure, then a late settlement.
  await until('the deadline timer', () => e.clockDelays().length > 0);
  e.advanceClock(Math.max(...e.clockDelays()));
  const r = await first;
  assert.ok(diagnosed(r, 'store_failed'), JSON.stringify(r.evidence.requests.map((q) => q.optimization.diagnostics)));
  assert.ok(gauge(e).abandonedPending >= 1);
  release();
  await until('the late discard', () => gauge(e).lateResolutionsDiscarded >= 1 && gauge(e).abandonedPending === 0);
});

test('plan.invalidate reaches the store; a never-settling store call ends at timeoutMs as pending', async (t) => {
  let signal;
  const st = scripted({ invalidate: (c) => { signal = c.signal; return new Promise(() => {}); } });
  const { e, p } = await engine(t, st);
  await decide(p, 'cold');
  const gets = st.calls.get;
  const pending = p.invalidate({ timeoutMs: 50 });
  await until('the timeout timer', () => e.clockDelays().includes(50));
  e.advanceClock(50);
  const r = await pending;
  assert.equal(r.persistent, 'pending');
  assert.equal(r.complete, false);
  assert.ok(signal.aborted, 'the store call is aborted at the timeout');
  assert.equal(gauge(e).abandonedPending, 1);
  // Unavailable: no persistent read or write for the selection.
  const d = await decide(p, 'unavailable');
  assert.ok(every(d, 'miss') && diagnosed(d, 'unavailable until invalidation completes'), outcomes(d));
  assert.equal(st.calls.get, gets);
  // Bounds, and a plan without an optimization namespace.
  await assert.rejects(p.invalidate({ timeoutMs: 0 }), (err) => err instanceof rustev.ConfigRefused && err.code === 'out_of_range');
  await assert.rejects(p.invalidate({ timeoutMs: 600_001 }), (err) => err instanceof rustev.ConfigRefused);
  const plain = await engine(t, scripted(), { plan: 'support.plan.json' });
  assert.equal(await plain.p.invalidate(), null);
});

test('a store invalidation that throws leaves the selection pending; a later call completes it', async (t) => {
  let fail = true;
  const inner = new ValkeyStore(new ValkeyCluster(), contract);
  const st = scripted({ invalidate: async (c) => { if (fail) throw new Error('SYNTHETIC invalidate failure'); return inner.invalidate(c); },
    get: (c) => inner.get(c), put: (c) => inner.put(c), delete: (c) => inner.delete(c) });
  const { p } = await engine(t, st);
  await decide(p, 'cold');
  const failed = await p.invalidate();
  assert.deepEqual([failed.persistent, failed.complete], ['pending', false]);
  fail = false;
  const done = await p.invalidate();
  assert.deepEqual([done.persistent, done.complete], [{ generation: 1, removed: 3 }, true]);
  const after = await decide(p, 'after');
  assert.ok(every(after, 'miss'), outcomes(after));
  const warm = await decide(p, 'warm');
  assert.ok(every(warm, 'hit'), outcomes(warm));
});

test('shutdown reports a pending store callback abandoned', async (t) => {
  const st = scripted({ get: () => new Promise(() => {}) });
  const { e, p } = await engine(t, st);
  const r = decide(p, 'stuck').catch((err) => err);
  // One get per request, in parallel.
  await until('the gets', () => gauge(e).queueDepth === 3);
  const s = await e.shutdown({ drainMs: 0 });
  assert.equal(s.cancelled, 1);
  assert.equal(s.abandonedPending, 3);
  assert.equal(gauge(e).queueDepth, 0, 'no slot held');
  await r;
});

// ---- the Valkey 8.1 double: volatile-lru, primary-only reads (3.7) --------

test('volatile-lru evicts only entries; generation keys survive, so an invalidated entry is never served', async (t) => {
  // Room for the generation and counter keys and a few entries.
  const cluster = new ValkeyCluster({ maxmemory: 2_600 });
  const store = new ValkeyStore(cluster, contract);
  const calls = { backend: 0 };
  const { p } = await engine(t, store, { calls });
  await decide(p, 'cold');
  assert.ok(await p.invalidate());
  // Memory pressure: entries carry a TTL and go first; keys without one
  // (generations, byte counters) are never evicted.
  for (let i = 0; i < 20; i++) {
    cluster.primary.hset(`junk:${i}`, { v: 'x'.repeat(200) });
    cluster.primary.pexpire(`junk:${i}`, 1_000_000);
  }
  assert.ok(cluster.primary.evicted.length > 0);
  assert.ok(cluster.primary.evicted.every((k) => !/:g(:|$)/.test(k) && !/:b(:|$)/.test(k)), cluster.primary.evicted.join());
  const r = await decide(p, 'after');
  assert.ok(every(r, 'miss'), outcomes(r));
  assert.equal(calls.backend, 6);
  // With nothing evictable a write is refused with OOM and changes nothing.
  const full = new ValkeyCluster({ maxmemory: 40 });
  full.primary.set('{rx:n}:g', '1');
  assert.throws(() => full.primary.set('{rx:n}:b', '1'.repeat(64)), /OOM/);
  assert.deepEqual([...full.primary.keys.keys()], ['{rx:n}:g']);
});

test('non-conforming variants: allkeys-lru and a reader endpoint serve stale hits Rustev cannot detect; split reads only miss', async (t) => {
  // allkeys-lru may evict a generation key: G1 broken, the invalidated
  // entry passes every check again.
  {
    // Entries remain after the advance (removal is optional).
    const cluster = new ValkeyCluster({ policy: 'allkeys-lru' });
    const store = new ValkeyStore(cluster, contract, { removeOnInvalidate: false });
    const { p } = await engine(t, store);
    await decide(p, 'cold');
    await p.invalidate();
    const gkeys = [...cluster.primary.keys.keys()].filter((k) => /:g$/.test(k));
    assert.equal(gkeys.length, 1);
    cluster.primary.del(gkeys[0]); // the eviction allkeys-lru may make
    const r = await decide((await engine(t, store)).p, 'stale');
    assert.ok(every(r, 'hit'), 'not detectable by Rustev; G1 forbids it');
  }
  // A lagging reader endpoint: G2 broken.
  {
    const cluster = new ValkeyCluster();
    const store = new ValkeyStore(cluster, contract, { readFrom: 'replica' });
    const writer = await engine(t, new ValkeyStore(cluster, contract, { removeOnInvalidate: false }));
    await decide(writer.p, 'cold');
    cluster.replicate();
    await writer.p.invalidate();
    const r = await decide((await engine(t, store)).p, 'stale');
    assert.ok(every(r, 'hit'), 'not detectable by Rustev; G2 forbids it');
    // The same read on the primary misses.
    const primary = await decide((await engine(t, new ValkeyStore(cluster, contract))).p, 'primary');
    assert.ok(every(primary, 'miss') && diagnosed(primary, 'generation_advanced'), outcomes(primary));
  }
  // Split reads (G3 broken) with an invalidation in the gap: spurious
  // misses with generation_advanced, never a stale hit.
  {
    const cluster = new ValkeyCluster();
    const writer = await engine(t, new ValkeyStore(cluster, contract, { removeOnInvalidate: false }));
    await decide(writer.p, 'cold');
    let once = true;
    const split = new ValkeyStore(cluster, contract, { atomicGet: false,
      between: async () => { if (once) { once = false; cluster.primary.incrby('{rx:x}:unused', 1); } } });
    const r = await decide((await engine(t, split)).p, 'split');
    assert.ok(every(r, 'hit'), 'with nothing in the gap a split read still hits');
    await writer.p.invalidate();
    const after = await decide((await engine(t, split)).p, 'after');
    assert.ok(!outcomes(after).includes('hit'), outcomes(after));
  }
  // Check-then-write (G4 broken) with an invalidation in the gap: the
  // entry keeps its old generations and is never served.
  {
    const cluster = new ValkeyCluster();
    let invalidator;
    const racy = new ValkeyStore(cluster, contract, { atomicPut: false,
      between: async () => { if (invalidator) { const i = invalidator; invalidator = null; await i(); } } });
    const { p } = await engine(t, racy);
    invalidator = () => p.invalidate();
    const cold = await decide(p, 'cold');
    assert.ok(every(cold, 'miss'), outcomes(cold));
    const next = await decide((await engine(t, new ValkeyStore(cluster, contract))).p, 'next');
    assert.ok(!outcomes(next).includes('hit'), outcomes(next));
    // The entry written in the gap carries the old generations: read, and
    // refused by the generation check.
    assert.ok(next.evidence.requests.some((q) => q.optimization.diagnostics.includes('generation_advanced')),
      JSON.stringify(next.evidence.requests.map((q) => q.optimization.diagnostics)));
  }
});
