// Spec 063 increment 1: synthetic content, caller-only capture and replay.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const rustev = createRequire(import.meta.url)('../index.js');
const fx = n => readFileSync(new URL(`../../../tools/rustev-conformance/fixtures/runtime/${n}`, import.meta.url));
const scope = { mode: 'principal', tenant: 'tenant', context_revision: 'revision', principal_scope: 'scope' };
const now = 1790164800000;
const capture = { scope, maxBytes: 16777216, createdAtMs: now };
const request = () => ({ decisionId: 'capture', snapshot: fx('support.snapshot.json'), evaluationTimeMs: now });
const outputs = {
  'support.topic': { logits: { billing: 4, integration_defect: 0, account_access: 0, other: 0 } },
  'support.frustration': { logits: { calm: 3, frustrated: 0, very_angry: 0 } },
  'support.deadline': { logits: { false: 3, true: 0 } },
};
async function setup(t, extra = {}, compile = false) {
  const calls = { backend: 0, context: 0, sink: [] };
  const e = await rustev.Engine.create({
    clock: 'manual', runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 20000 },
    sink: { queueCapacity: 8, deliver: async r => { calls.sink.push(r); return 'ack'; } },
    backends: [{ descriptor: fx('synthetic-linear-head.descriptor.json'), costModel: 'bounded',
      costBound: { bounded: { max_units: 5 } }, maxConcurrency: 1, queueCapacity: 8,
      infer: async call => { calls.backend++; return { output: outputs[JSON.parse(Buffer.from(call.projection)).task], charge: { observed: { units: 1 } } }; } }],
    ...extra,
  });
  t.after(() => e.shutdown({ drainMs: 1000 }));
  const plan = JSON.parse(fx('support.plan.json'));
  const p = await e.compile({ ...(compile ? { definition: plan.definition } : { plan }),
    descriptors: ['synthetic-linear-head', 'synthetic-label-only', 'synthetic-linear-head-replica', 'synthetic-other-head'].map(n => fx(`${n}.descriptor.json`)),
    calibrations: [fx('topic.calibration.json')] });
  return { e, p, calls };
}
async function replay(e, c, omit) {
  const inputs = { 'b.json': c.bundle };
  for (const [k, v] of Object.entries(c.items ?? {})) if (k !== omit) inputs[`items/${k}`] = v;
  return JSON.parse(Buffer.from((await e.replay(inputs, { bundle: 'b.json', scope, nowMs: now, store: 'items' })).out));
}
test('capture absent and over limit preserve decision, evidence and delivery', async t => {
  const { p } = await setup(t);
  const bare = await p.decide(request());
  assert.ok(!Object.hasOwn(bare, 'capture'));
  const limited = await p.decide(request(), { capture: { ...capture, maxBytes: 1 } });
  assert.equal(limited.capture.status, 'limit_exceeded');
  assert.ok(!limited.capture.bundle && !limited.capture.items);
  delete limited.capture;
  assert.deepEqual(limited, bare);
});
test('invalid capture refuses before context fetch or admission without echoing content', async t => {
  let fetched = 0;
  const { p, calls } = await setup(t, { contextSource: { queueCapacity: 8, fetch: async () => { fetched++; return fx('support.snapshot.json'); } } });
  for (const [edit, code] of [
    [{ maxBytes: 0 }, 'zero_limit'], [{ maxBytes: 16777217 }, 'limit_too_large'],
    [{ lifetimeMs: 2592000001 }, 'out_of_range'], [{ hostCapMs: 1 }, 'out_of_range'],
    [{ scope: { ...scope, principal_scope: '' } }, 'invalid_scope'],
    [{ scope: { ...scope, credential: 'SECRET' } }, 'invalid_scope'],
    [{ createdAtMs: 253402300799999 }, 'out_of_range'], [{ maxBytes: 1.5 }, 'out_of_range'],
  ]) {
    await assert.rejects(p.decide({ ...request(), snapshot: undefined }, { capture: { ...capture, ...edit } }), e => {
      assert.ok(e instanceof rustev.CaptureConfigRefused); assert.equal(e.code, code);
      assert.ok(!JSON.stringify(e).includes('SECRET')); return true;
    });
  }
  assert.equal(fetched, 0); assert.equal(calls.backend, 0); assert.equal(calls.sink.length, 0);
});
for (const retain of ['digest_only', 'embedded', 'external']) test(`${retain}: replay and retention`, async t => {
  const { e, p, calls } = await setup(t, {}, retain === 'embedded');
  const r = await p.decide(request(), { capture: { ...capture, ...(retain === 'digest_only' ? {} : { retain }) } });
  assert.equal(r.capture.status, 'complete'); assert.ok(r.capture.bundle); assert.ok(!r.capture.error);
  const bundle = JSON.parse(Buffer.from(r.capture.bundle));
  assert.equal(bundle.expires_at_ms - bundle.created_at_ms, 604800000);
  const before = calls.backend;
  const result = await replay(e, r.capture);
  assert.equal(result.status, retain === 'digest_only' ? 'incomparable' : 'reproduced');
  // 004: a digest-only item is `missing`; 036's `content-not-retained` names
  // only an acquisition page, which joins with increment 3.
  if (retain === 'digest_only') assert.equal(result.reason, 'missing');
  if (retain === 'external') {
    const ref = bundle.snapshot.retention.external.reference;
    assert.equal(ref, bundle.snapshot.retention.external.digest);
    assert.ok(r.capture.items[ref]);
    const missing = await replay(e, r.capture, ref);
    assert.equal(missing.status, 'incomparable'); assert.match(JSON.stringify(missing), /snapshot/);
  }
  assert.equal(calls.backend, before, 'offline replay calls no backend');
});
test('snapshot canary survives capture and telemetry redaction; principal stays out', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'rustev-capture-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const path = join(dir, 'trace.jsonl');
  const snapshot = JSON.parse(fx('support.snapshot.json'));
  snapshot.entries.find(x => x.field === 'ticket.message').value = 'SNAPSHOT-CANARY-063';
  const { e, p, calls } = await setup(t, {
    contextSource: { queueCapacity: 8, fetch: async () => snapshot },
    telemetry: { exporter: { kind: 'jsonl', path }, key: { bytes: new Uint8Array(32).fill(7), keyId: 'KEY-CANARY-063' }, acknowledgement: 'exported',
      policy: { schema: 'rustev.redaction/1', classes: { 'user-supplied': 'drop' }, fields: [], content_events: [] } },
  });
  const r = await p.decide({ ...request(), snapshot: undefined }, { capture: { ...capture, retain: 'embedded' },
    principalHandle: Buffer.from('PRINCIPAL-CANARY-063'),
    traceparent: '00-0123456789abcdef0123456789abcdef-0123456789abcdef-01', tracestate: 'vendor=TRACE-CANARY-063' });
  const bundle = Buffer.from(r.capture.bundle).toString();
  assert.match(bundle, /SNAPSHOT-CANARY-063/);
  for (const canary of ['PRINCIPAL-CANARY-063', 'KEY-CANARY-063', 'TRACE-CANARY-063', '0123456789abcdef']) assert.ok(!bundle.includes(canary));
  const observations = JSON.stringify([calls.sink, e.gauges()]) + readFileSync(path, 'utf8');
  assert.ok(!observations.includes('SNAPSHOT-CANARY-063')); assert.ok(!observations.includes('PRINCIPAL-CANARY-063'));
});
test('SinkFailure carries capture beside record, never in detail or message', async t => {
  const { e, p } = await setup(t, { sink: { queueCapacity: 8, deliver: async () => { throw new Error('sink refused'); } } });
  let captured;
  await assert.rejects(p.decide(request(), { capture: { ...capture, retain: 'embedded' } }), error => {
    assert.ok(error instanceof rustev.SinkFailure); assert.ok(error.record); assert.ok(error.capture.bundle);
    assert.ok(!JSON.stringify(error.detail).includes('rustev.replay')); assert.ok(!error.message.includes('rustev.replay'));
    captured = error.capture; return true;
  });
  assert.equal((await replay(e, captured)).status, 'reproduced');
});
test('a rejected decision captures nothing', async t => {
  const { p, calls } = await setup(t);
  const controller = new AbortController(); controller.abort();
  await assert.rejects(p.decide(request(), { capture, signal: controller.signal }), e => {
    assert.ok(e instanceof rustev.CancelledBeforeAdmission); assert.ok(!e.capture); return true;
  });
  assert.equal(calls.backend, 0); assert.equal(calls.sink.length, 0);
});
