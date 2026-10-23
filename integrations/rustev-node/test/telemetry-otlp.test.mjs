// 049 3.2, 3.6 and 028 3.9.2: synthetic loopback collectors only.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';

const pkg = join(dirname(fileURLToPath(import.meta.url)), '..');
const root = join(pkg, '../..');
const rustev = createRequire(import.meta.url)(pkg);
const read = (p) => readFileSync(join(root, p));
const cli = (task, name) => read(`tools/rustev-conformance/fixtures/cli/${task}.show/${name}`);
const otel = (name) => read(`integrations/rustev-otel/tests/fixtures/${name}`);
const parent = '00-0123456789abcdef0123456789abcdef-0123456789abcdef-01';
const key = { bytes: new Uint8Array(32).fill(7), keyId: 'test' };
const runtime = { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 4, sinkTimeoutMs: 20_000 };
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

async function collector(t, status = 200, location) {
  const seen = { connections: 0, requests: [] };
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    seen.requests.push({ path: req.url, body: Buffer.concat(chunks), headers: req.headers });
    res.writeHead(status, { 'content-type': 'application/x-protobuf', ...(location ? { location } : {}) });
    res.end();
  });
  server.on('connection', () => { seen.connections++; });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  t.after(() => new Promise((resolve) => { server.close(resolve); server.closeAllConnections(); }));
  return { ...seen, get connections() { return seen.connections; }, origin: `http://127.0.0.1:${server.address().port}` };
}
function settings(c, extra = {}) {
  return { exporter: { kind: 'otlp-http', endpoint: c.origin, allowedEndpoints: [c.origin] },
    key, acknowledgement: 'exported', ...extra };
}
for (const task of ['support', 'lodging']) {
  test(`${task}: OTLP traces, metrics and redaction (049 3.3 to 3.5)`, async (t) => {
    const c = await collector(t);
    const e = await engine(settings(c, { logs: true, exporter: { ...settings(c).exporter, headers: { 'x-test-token': 'PRIVATE-header' } } }), task);
    const bare = await engine(undefined, task);
    t.after(() => e.shutdown()); t.after(() => bare.shutdown());
    const req = request(task);
    req.snapshot = JSON.parse(req.snapshot);
    const canary = 'PRIVATE-CANARY-049-OTLP';
    req.snapshot.entries.find((x) => x.field === (task === 'support' ? 'ticket.message' : 'trip.text')).value += canary;
    const p = await plan(e, task);
    const result = await p.decide(req);
    const base = await (await plan(bare, task)).decide(req);
    assert.deepEqual(result.raw, base.raw);
    assert.deepEqual(result.delivery, base.delivery);
    assert.deepEqual(await e.flushTelemetryMetrics(), { outcome: 'exported' });
    assert.ok(c.requests.some((r) => r.path === '/v1/traces' && r.body.includes('https://opentelemetry.io/schemas/1.41.1')));
    assert.ok(c.requests.some((r) => r.path === '/v1/metrics'));
    // 028 3.6.3: missing evidence produces an unresolved decision log.
    const missing = request(task, 'missing');
    missing.snapshot = JSON.parse(missing.snapshot);
    missing.snapshot.entries = [];
    await p.decide(missing);
    await until(() => e.gauges().telemetry.logs.inFlight === 0);
    assert.ok(c.requests.some((r) => r.path === '/v1/logs'));
    assert.ok(e.gauges().telemetry.logs.exported > 0);
    for (const r of c.requests) {
      assert.ok(!r.body.includes(canary));
      assert.ok(!r.body.includes('PRIVATE-header'));
      assert.equal(r.headers['x-test-token'], 'PRIVATE-header');
    }
  });
}

test('049 3.6: refused endpoints never connect; errors contain only origins', async (t) => {
  const allowed = await collector(t);
  const outside = await collector(t);
  for (const exporter of [
    { endpoint: `${outside.origin}/secret?token=PRIVATE`, allowedEndpoints: [allowed.origin] },
    { endpoint: outside.origin.replace('127.0.0.1', 'localhost'), allowedEndpoints: [outside.origin.replace('127.0.0.1', 'localhost')] },
    { endpoint: outside.origin.replace('://', '://user:PRIVATE@'), allowedEndpoints: [outside.origin] },
    { endpoint: outside.origin, allowedEndpoints: [] },
  ]) {
    await assert.rejects(engine(settings(allowed, { exporter: { kind: 'otlp-http', ...exporter } })), (error) => {
      rejects('ConfigRefused', 'endpoint_not_allowed')(error);
      assert.ok(!JSON.stringify(error).includes('PRIVATE'));
      assert.ok(!error.message.includes('PRIVATE'));
      if (exporter.endpoint.includes('/secret')) assert.equal(error.detail.origin, outside.origin);
      return true;
    });
  }
  assert.equal(outside.connections, 0);
  assert.equal(allowed.connections, 0);
  const script = `const {Engine} = require(${JSON.stringify(pkg)});
    Engine.create({
      runtime: ${JSON.stringify(runtime)}, clock: 'manual', sink: {queueCapacity: 8, deliver: async () => 'ack'},
      telemetry: {exporter: ${JSON.stringify(settings(allowed).exporter)}, key: {bytes: new Uint8Array(32).fill(7), keyId: 'test'}}
    }).then(() => {process.exitCode = 1}, e => console.log(JSON.stringify({code:e.code, detail:e.detail})));`;
  const child = spawn(process.execPath, ['-e', script], { env: { ...process.env, OTEL_EXPORTER_OTLP_ENDPOINT: `${outside.origin}/private` } });
  let stdout = '', stderr = '';
  child.stdout.on('data', (b) => { stdout += b; }); child.stderr.on('data', (b) => { stderr += b; });
  const code = await new Promise((resolve, reject) => { child.on('error', reject); child.on('close', resolve); });
  assert.equal(code, 0, stderr);
  assert.deepEqual(JSON.parse(stdout), { code: 'endpoint_not_allowed', detail: { field: 'telemetry.exporter.endpoint', origin: outside.origin } });
  assert.equal(outside.connections, 0);
});

test('049 3.6: header and option errors disclose no secrets', async (t) => {
  const c = await collector(t);
  const base = settings(c).exporter;
  for (const headers of [{ Authorization: 'PRIVATE-one', authorization: 'PRIVATE-two' }, { 'bad name': 'PRIVATE' }, { authorization: 'PRIVATE\nvalue' }]) {
    await assert.rejects(engine(settings(c, { exporter: { ...base, headers } })), (error) => {
      rejects('ConfigRefused', 'invalid_header', 'telemetry.exporter.headers')(error);
      assert.equal(typeof error.detail.name, 'string');
      assert.ok(!error.message.includes('PRIVATE')); assert.ok(!JSON.stringify(error.detail).includes('PRIVATE'));
      return true;
    });
  }
  for (const headers of [null, [], 'PRIVATE', { authorization: 1 }, new Date()]) {
    await assert.rejects(engine(settings(c, { exporter: { ...base, headers } })), rejects('ConfigRefused', 'out_of_range', 'telemetry.exporter.headers'));
  }
  for (const timeoutMs of [0, 600001, 1.5, '1']) {
    await assert.rejects(engine(settings(c, { exporter: { ...base, timeoutMs } })), rejects('ConfigRefused', 'out_of_range', 'telemetry.exporter.timeoutMs'));
  }
  await assert.rejects(engine(settings(c, { exporter: { ...base, caPem: 'PRIVATE' } })), rejects('ConfigRefused', 'out_of_range', 'telemetry.exporter.caPem'));
  await assert.rejects(engine(settings(c, { exporter: { ...base, caPem: Buffer.from('PRIVATE-invalid-CA') } })), (error) => {
    rejects('ConfigRefused', 'exporter_unavailable', 'telemetry.exporter')(error);
    assert.ok(!error.message.includes('PRIVATE')); assert.ok(!JSON.stringify(error.detail).includes('PRIVATE'));
    return true;
  });
  assert.equal(c.connections, 0);
});

for (const status of [500, 307]) {
  test(`049 3.3.2: HTTP ${status} loss, required delivery and metrics failure`, async (t) => {
    const target = await collector(t);
    const c = await collector(t, status, `${target.origin}/v1/traces`);
    const bare = await engine(); t.after(() => bare.shutdown());
    const base = await (await plan(bare)).decide(request());
    for (const requireTelemetry of [false, true]) {
      const e = await engine(settings(c, { requireTelemetry })); t.after(() => e.shutdown());
      const p = await plan(e);
      if (requireTelemetry) {
        await assert.rejects(p.decide(request()), (error) => {
          assert.ok(error instanceof rustev.SinkFailure);
          assert.deepEqual(error.record, base.raw.evidence); return true;
        });
      } else {
        const result = await p.decide(request());
        assert.deepEqual(result.raw, base.raw); assert.deepEqual(result.delivery, base.delivery);
      }
      const stats = e.gauges().telemetry;
      assert.equal(stats.decisionsLost.export_failed, 1);
      assert.ok(stats.spans.lost.export_failed > 0);
      assert.deepEqual(await e.flushTelemetryMetrics(), { outcome: 'lost', reason: 'export_failed' });
      assert.equal(e.gauges().telemetry.metricExports.lost.export_failed, 1);
    }
    assert.equal(target.connections, 0);
  });
}
