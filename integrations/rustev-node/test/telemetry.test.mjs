// 049 increment 1: synthetic reference decisions, manual time, no network.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkg = join(dirname(fileURLToPath(import.meta.url)), '..');
const root = join(pkg, '../..');
const rustev = createRequire(import.meta.url)(pkg);
const read = (p) => readFileSync(join(root, p));
const cli = (task, name) => read(`tools/rustev-conformance/fixtures/cli/${task}.show/${name}`);
const otel = (name) => read(`integrations/rustev-otel/tests/fixtures/${name}`);
const parent = '00-0123456789abcdef0123456789abcdef-0123456789abcdef-01';
const key = { bytes: new Uint8Array(32).fill(7), keyId: 'test' };
const runtime = { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 4, sinkTimeoutMs: 20_000 };
function settings(path, extra = {}) {
  return { exporter: { kind: 'jsonl', path }, key, acknowledgement: 'exported', ...extra };
}
function temp(t) {
  const dir = mkdtempSync(join(tmpdir(), 'rustev-node-telemetry-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return join(dir, 'trace.jsonl');
}
async function engine(telemetry, task = 'support', sink) {
  return rustev.Engine.create({ runtime, clock: 'manual', telemetry,
    sink: sink ?? { queueCapacity: 8, deliver: async () => 'ack' },
    rulesBackends: [{ program: cli(task, `${task}.rules.json`), maxConcurrency: 4 }],
  });
}
async function plan(e, task = 'support', compile = false) {
  const document = JSON.parse(otel(`${task}.plan.json`));
  return e.compile({ ...(compile ? { definition: document.definition } : { plan: document }),
    descriptors: [cli(task, `${task}.rules.json.descriptor.json`)],
    calibrations: task === 'support' ? [otel('topic.calibration.json')] : [],
  });
}
function request(task = 'support', id = 'decision') {
  return { decisionId: id, snapshot: otel(`${task}.snapshot.json`), evaluationTimeMs: 1_790_164_800_000 };
}
const until = async (condition) => {
  const end = Date.now() + 120_000;
  while (!condition()) {
    assert.ok(Date.now() < end, 'condition did not settle');
    await new Promise((r) => setImmediate(r));
  }
};
const rejects = (Class, code, field) => (e) => {
  assert.ok(e instanceof rustev[Class], String(e));
  assert.equal(e.code, code);
  if (field) assert.equal(e.detail.field, field);
  return true;
};
function sorted(v) {
  if (Array.isArray(v)) return v.map(sorted);
  if (v && typeof v === 'object') return Object.fromEntries(Object.keys(v).sort().map((k) => [k, sorted(v[k])]));
  return v;
}
function shape(lines) {
  const byId = new Map(lines.map((l) => [l.span_id, l]));
  function branch(line) {
    const p = byId.get(line.parent_span_id);
    return `${p ? branch(p) : ''}/${line.name}`;
  }
  return lines.map((l) => ({ branch: branch(l), kind: l.kind, attributes: Object.keys(l.attributes).sort() }))
    .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

for (const task of ['support', 'lodging']) {
  test(`${task}: reference tree, redaction, trace IDs and byte equivalence (049 3.3 to 3.5)`, async (t) => {
    const path = temp(t);
    const e = await engine(settings(path), task);
    const bare = await engine(undefined, task);
    t.after(() => e.shutdown({ drainMs: 1000 }));
    t.after(() => bare.shutdown({ drainMs: 1000 }));
    // Exercise both load and compile registration, against the same plans.
    const p = await plan(e, task, task === 'lodging');
    const q = await plan(bare, task);
    const req = request(task);
    const snapshot = JSON.parse(req.snapshot);
    const text = snapshot.entries.find((x) => x.field === (task === 'support' ? 'ticket.message' : 'trip.text'));
    const canary = 'PRIVATE-CANARY-049';
    text.value += ` ${canary}`;
    req.snapshot = snapshot;
    const a = await p.decide(req, { traceparent: parent, tracestate: 'vendor=value' });
    const b = await q.decide(req, { traceparent: parent, tracestate: 'vendor=value' });
    assert.deepEqual(a.raw, b.raw);
    assert.deepEqual(a.delivery, b.delivery);
    assert.equal(Object.hasOwn(b, 'trace'), false);
    assert.equal(Object.hasOwn(bare.gauges(), 'telemetry'), false);
    const content = readFileSync(path, 'utf8');
    assert.ok(!content.includes(canary));
    const lines = content.trim().split('\n').map((line) => {
      assert.ok(Buffer.byteLength(line) < 65536);
      const value = JSON.parse(line);
      assert.equal(line, JSON.stringify(sorted(value)));
      assert.equal(value.schema, 'rustev.trace/1');
      assert.equal(value.schema_version, 1);
      return value;
    });
    const top = lines.find((l) => l.name === 'rustev.decision');
    assert.ok(top);
    assert.equal(top.trace_id, parent.split('-')[1]);
    assert.equal(top.parent_span_id, parent.split('-')[2]);
    assert.equal(top.trace_id, a.trace.traceId);
    assert.equal(top.span_id, a.trace.rootSpanId);
    const golden = read(`integrations/rustev-otel/tests/golden/${task}.trace.jsonl`).toString().trim().split('\n').map(JSON.parse);
    assert.deepEqual(shape(lines), shape(golden));
    const stats = e.gauges().telemetry;
    assert.equal(stats.planUnregistered, 0);
    assert.equal(stats.invalidTraceparent, 0);
    assert.equal(stats.spans.accepted, lines.length);
    assert.equal(stats.spans.exported, lines.length);
    assert.equal(stats.spans.inFlight, 0);
    assert.deepEqual(stats.spans.lost, {});
    assert.deepEqual(stats.metricExports, { accepted: 0, exported: 0, inFlight: 0, lost: {} });
    assert.deepEqual(await e.flushTelemetryMetrics(), { outcome: 'not_taken' });
    assert.equal((await e.shutdown({ drainMs: 1000 })).telemetryUnflushed, 0);
    assert.equal((await bare.shutdown()).telemetryUnflushed, 0);
  });
}

test('049 3.6: create refusals precede opening the file and disclose no secret', async (t) => {
  const path = temp(t);
  const cases = [
    ...['otlp-grpc', 'unknown'].map((kind) => [{ exporter: { kind, path } }, 'out_of_range', 'telemetry.exporter.kind']),
    [{ key: undefined }, 'digest_key_required', 'telemetry.key'],
    [{ logs: true }, 'logs_unsupported', 'telemetry.logs'],
    [{ telemetryTimeoutMs: 20_000 }, 'out_of_range', 'telemetry.telemetryTimeoutMs'],
    [{ telemetryTimeoutMs: 0 }, 'out_of_range', 'telemetry.telemetryTimeoutMs'],
    [{ acknowledgement: 'wrong' }, 'out_of_range', 'telemetry.acknowledgement'],
    [{ sampling: { parent: 'wrong' } }, 'out_of_range', 'telemetry.sampling.parent'],
    [{ sampling: { root: 'wrong' } }, 'out_of_range', 'telemetry.sampling.root'],
    [{ sampling: { root: { ratioPerMillion: 1_000_001 } } }, 'out_of_range', 'telemetry.sampling.root.ratioPerMillion'],
    [{ sampling: { alwaysSampleOn: ['wrong'] } }, 'out_of_range', 'telemetry.sampling.alwaysSampleOn'],
    [{ key: { bytes: new Uint8Array(31), keyId: 'test' } }, 'out_of_range', 'telemetry.key.bytes'],
    [{ key: { bytes: new Uint8Array(65), keyId: 'test' } }, 'out_of_range', 'telemetry.key.bytes'],
    [{ key: { ...key, keyId: '' } }, 'out_of_range', 'telemetry.key.keyId'],
    [{ key: { ...key, keyId: 'x'.repeat(65) } }, 'out_of_range', 'telemetry.key.keyId'],
  ];
  const ranges = { maxQueuedSpans: [1, 1_048_576], batchSize: [1, 8192], exportTimeoutMs: [1, 600_000],
    valueBound: [16, 4096], maxPendingContexts: [1, 65_536], contextTtlMs: [1, 86_400_000], joinBuffer: [1, 65_536] };
  for (const [name, [min, max]] of Object.entries(ranges)) {
    for (const value of [min - 1, max + 1, 1.5, '1']) cases.push([{ limits: { [name]: value } }, 'out_of_range', `telemetry.limits.${name}`]);
  }
  for (const [change, code, field] of cases) {
    await assert.rejects(engine(settings(path, change)), rejects('ConfigRefused', code, field));
    assert.equal(existsSync(path), false);
  }
  for (const policy of ['{bad json', { schema: 'rustev.redaction/1', classes: { 'user-supplied': 'allow' }, fields: [], content_events: [] }]) {
    await assert.rejects(engine(settings(path, { policy })), rejects('InvalidDocument', 'invalid_document', 'telemetry.policy'));
    assert.equal(existsSync(path), false);
  }
  await assert.rejects(engine(settings(join(path, 'missing', 'file'))), rejects('ConfigRefused', 'telemetry_file', 'telemetry.exporter.path'));
});

test('049 3.3.2: file full is counted; required telemetry fails delivery', async (t) => {
  const bare = await engine();
  const base = await (await plan(bare)).decide(request());
  t.after(() => bare.shutdown());
  for (const requireTelemetry of [false, true]) {
    const path = temp(t);
    const e = await engine(settings(path, { exporter: { kind: 'jsonl', path, maxBytes: 1 }, requireTelemetry }));
    t.after(() => e.shutdown());
    const p = await plan(e);
    if (requireTelemetry) {
      await assert.rejects(p.decide(request()), (error) => {
        assert.ok(error instanceof rustev.SinkFailure);
        assert.deepEqual(error.record, base.raw.evidence);
        return true;
      });
    } else {
      const result = await p.decide(request());
      assert.deepEqual(result.raw, base.raw);
      assert.deepEqual(result.delivery, base.delivery);
    }
    assert.equal(e.gauges().telemetry.decisionsLost.file_full, 1);
    assert.ok(e.gauges().telemetry.spans.lost.file_full > 0);
  }
});

test('049 3.4.2: tracestate is validated with and without telemetry', async (t) => {
  for (const telemetry of [undefined, settings(temp(t))]) {
    const e = await engine(telemetry);
    t.after(() => e.shutdown());
    const p = await plan(e);
    for (const options of [{ tracestate: 'a=b' }, { traceparent: parent, tracestate: 'x'.repeat(513) },
      { traceparent: parent, tracestate: 'a=\n' }, { traceparent: parent, tracestate: 'a=é' }, { traceparent: parent, tracestate: 'a=\x7f' }]) {
      await assert.rejects(p.decide(request(), options), rejects('InvalidTraceContext', 'invalid_tracestate', 'tracestate'));
    }
    assert.equal(e.gauges().sink.acknowledged, 0);
    await p.decide(request(), { traceparent: parent, tracestate: 'x'.repeat(512) });
  }
});

test('049 3.4.4: rejected admission abandons its context', async (t) => {
  let release;
  let hold = true;
  const e = await engine(settings(temp(t), { limits: { maxPendingContexts: 1, contextTtlMs: 10 } }), 'support', {
    queueCapacity: 8, deliver: () => hold ? new Promise((r) => { release = r; }) : Promise.resolve('ack'),
  });
  t.after(() => e.shutdown());
  const p = await plan(e);
  const first = p.decide(request('support', 'first'));
  await until(() => release !== undefined);
  // First has a context: TableFull is harmless to the rejected second call.
  await assert.rejects(p.decide(request('support', 'second')), rejects('AdmissionRejected', 'overloaded'));
  hold = false;
  release('ack');
  await first;
  // Keep a decision active after its telemetry context has expired, leaving
  // room for a new context that is rejected and must then be abandoned.
  hold = true; release = undefined;
  const held = p.decide(request('support', 'held'));
  await until(() => release !== undefined);
  e.advanceClock(11);
  await assert.rejects(p.decide(request('support', 'rejected')), rejects('AdmissionRejected', 'overloaded'));
  const expired = e.gauges().telemetry.contextExpired;
  hold = false; release('ack'); await held;
  const next = await p.decide(request('support', 'next'));
  assert.ok(next.trace, 'rejection released the only context slot');
  assert.equal(e.gauges().telemetry.contextExpired, expired);
});

test('049 3.5: exchange validation, metrics without telemetry, and shutdown', async (t) => {
  const bare = await engine();
  assert.throws(() => bare.recordExchange({}), rejects('ConfigRefused', 'telemetry_not_configured'));
  await assert.rejects(bare.flushTelemetryMetrics(), rejects('ConfigRefused', 'telemetry_not_configured'));
  await bare.shutdown();
  const e = await engine(settings(temp(t)));
  assert.throws(() => e.recordExchange({}), rejects('InvalidDocument', 'invalid_document'));
  const exchange = otel('support-remote.exchanges.jsonl').toString().trim().split('\n')[0];
  e.recordExchange(exchange);
  assert.deepEqual(await e.flushTelemetryMetrics(), { outcome: 'not_taken' });
  assert.equal((await e.shutdown()).telemetryUnflushed, 0);
  assert.throws(() => e.recordExchange(exchange), rejects('EngineShutDown', 'engine_shut_down'));
});

test('049 3.2: bounded options and no-digest policy map without requiring a key', async (t) => {
  const path = temp(t);
  const e = await engine(settings(path, {
    key: undefined,
    policy: { schema: 'rustev.redaction/1', classes: { id: 'drop', 'content-digest': 'drop' }, fields: [], content_events: [] },
    acknowledgement: 'accepted', telemetryTimeoutMs: 1,
    sampling: { parent: 'ignore', root: { ratioPerMillion: 0 }, alwaysSampleOn: ['unresolved', 'refusal', 'fallback', 'cancelled'] },
    bindings: { providers: { rules: 'synthetic' }, usageKeys: { rules: { input: 'in', output: 'out' } } },
    limits: { maxQueuedSpans: 1, batchSize: 1, exportTimeoutMs: 1, valueBound: 16, maxPendingContexts: 1, contextTtlMs: 1, joinBuffer: 1 },
  }));
  t.after(() => e.shutdown());
  const result = await (await plan(e)).decide(request());
  assert.match(result.trace.traceId, /^[0-9a-f]{32}$/);
  assert.match(result.trace.rootSpanId, /^[0-9a-f]{16}$/);
  assert.equal(e.gauges().telemetry.sampledOut, 1);
  assert.equal(readFileSync(path, 'utf8'), '');
});

test('049 3.4.2: the native entry point also refuses invalid tracestate', async () => {
  const native = createRequire(import.meta.url)(join(pkg, readdirSync(pkg).find((name) => name.startsWith('rustev.') && name.endsWith('.node'))));
  const e = native.Engine.create({ ...runtime, clock: 'manual', workerThreads: 1, blockingThreads: 1 },
    { queueCapacity: 8, acceptsRun3: false, deliver: async () => 'ack' },
    [{ program: cli('support', 'support.rules.json'), maxConcurrency: 4 }], [], undefined, undefined);
  const p = e.load(otel('support.plan.json'), [cli('support', 'support.rules.json.descriptor.json')], [otel('topic.calibration.json')]);
  for (const options of [{ tracestate: 'a=b' }, { traceparent: parent, tracestate: 'x'.repeat(513) }, { traceparent: parent, tracestate: '\n' }]) {
    await assert.rejects(p.decide({ ...request(), ...options }, e.cancelHandle()), (error) => {
      const value = JSON.parse(error.message);
      assert.equal(value.class, 'InvalidTraceContext');
      assert.equal(value.code, 'invalid_tracestate');
      return true;
    });
  }
  assert.equal(e.gauges().sink.acknowledged, 0);
  e.shutdown();
});
