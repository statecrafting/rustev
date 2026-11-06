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
