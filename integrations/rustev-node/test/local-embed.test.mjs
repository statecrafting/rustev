// Spec 011, 3.17.1: the optional `local-embed` feature of the native
// package. Without the feature `LocalEmbedBackend.open` rejects
// `feature_disabled` and `features().localEmbed` is false; with it, the
// committed SYNTHETIC support-routing bundle opens, setup refusals are
// typed, and the backend registers on an engine like a rules backend. The
// byte parity of its decisions is the conformance suite's
// (`run.mjs native --local-embed`). Mechanics only, no network.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { cpSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkg = join(here, '..');
const root = realpathSync(join(pkg, '../..'));
const rustev = createRequire(import.meta.url)(pkg);
const built = rustev.features().localEmbed;

const BUNDLE = join(root, 'backends/rustev-backend-embed-linear/tests/fixtures/support-routing');
const FIXTURES = join(root, 'tools/rustev-conformance/fixtures/local-embed');
const fx = (n) => readFileSync(join(FIXTURES, n));
const DESCRIPTOR = fx('support-routing.descriptor.json');
const ARTIFACT = JSON.parse(DESCRIPTOR.toString('utf8')).artifact;
const config = (over = {}) => ({ bundleDir: BUNDLE, expectedArtifact: ARTIFACT, workers: 1, maxQueued: 2, ...over });

/** The typed refusal `open` rejects with. */
async function refusal(cfg) {
  try {
    await rustev.LocalEmbedBackend.open(cfg);
  } catch (e) {
    return e;
  }
  assert.fail('open succeeded');
}

test('features() reports the build', () => {
  assert.equal(typeof built, 'boolean');
  assert.deepEqual(rustev.features(), { localEmbed: built });
});

test('a build without local-embed rejects open with feature_disabled', { skip: built }, async () => {
  const e = await refusal(config());
  assert.ok(e instanceof rustev.ConfigRefused && e instanceof rustev.RustevError);
  assert.equal(e.code, 'feature_disabled');
  assert.equal(e.detail.item, 'local-embed');
});

test('LocalEmbedBackend is made by open only', () => {
  assert.throws(() => new rustev.LocalEmbedBackend(), TypeError);
});

test('the synthetic bundle opens with its derived descriptor', { skip: !built }, async () => {
  const b = await rustev.LocalEmbedBackend.open(config());
  assert.equal(b.artifact, ARTIFACT);
  assert.deepEqual(Buffer.concat([Buffer.from(b.descriptor), Buffer.from('\n')]), DESCRIPTOR);
});

test('setup refusals are ConfigRefused with the 3.7.2 code and the item', { skip: !built }, async () => {
  const other = `sha256:${'0'.repeat(64)}`;
  let e = await refusal(config({ expectedArtifact: other }));
  assert.ok(e instanceof rustev.ConfigRefused);
  assert.equal(e.code, 'artifact_mismatch');
  assert.equal(typeof e.detail.item, 'string');
  e = await refusal(config({ workers: 0 }));
  assert.equal(e.code, 'manifest_invalid');
  // A relative bundle directory is never resolved against the process.
  e = await refusal(config({ bundleDir: 'backends/rustev-backend-embed-linear/tests/fixtures/support-routing' }));
  assert.equal(e.code, 'bundle_missing');
  assert.equal(e.detail.item, 'bundle_dir');
  // One byte of a head changed: refused before any use.
  const copy = realpathSync(mkdtempSync(join(tmpdir(), 'rustev-local-embed-')));
  try {
    cpSync(BUNDLE, copy, { recursive: true });
    const head = join(copy, 'heads/topic.bin');
    const bytes = readFileSync(head);
    bytes[0] ^= 1;
    writeFileSync(head, bytes);
    e = await refusal(config({ bundleDir: copy }));
    assert.equal(e.code, 'digest_mismatch');
  } finally {
    rmSync(copy, { recursive: true, force: true });
  }
});

test('an unknown configuration member is refused, never ignored', { skip: !built }, async () => {
  const e = await refusal(config({ embeddingCache: { maxEntries: 8, maxBytes: 4096 } }));
  assert.ok(e instanceof rustev.ConfigRefused);
  assert.equal(e.code, 'out_of_range');
  assert.equal(e.detail.field, 'localEmbed');
  assert.equal((await refusal(null)).code, 'out_of_range');
});

test('a registration that is not an opened backend is refused before any thread', async () => {
  await assert.rejects(
    rustev.Engine.create({
      runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 3, sinkTimeoutMs: 1000 },
      sink: { queueCapacity: 1, deliver: async () => 'ack' },
      localEmbedBackends: [{ backend: {}, maxConcurrency: 1 }],
    }),
    (e) => e instanceof rustev.ConfigRefused && e.code === 'out_of_range' && e.detail.field === 'localEmbedBackends[0].backend',
  );
});

test('the backend registers like a rules backend and answers model-derived', { skip: !built }, async () => {
  const backend = await rustev.LocalEmbedBackend.open(config());
  const engine = await rustev.Engine.create({
    runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 3, sinkTimeoutMs: 1000 },
    clock: 'manual',
    sink: { queueCapacity: 4, acceptsRun3: true, deliver: async () => 'ack' },
    localEmbedBackends: [{ backend, maxConcurrency: 3 }],
  });
  try {
    const plan = await engine.compile({
      plan: fx('support.plan.json'),
      descriptors: [backend.descriptor],
      calibrations: [fx('topic.calibration.json')],
    });
    const r = await plan.decide({
      decisionId: 'SYNTHETIC.node',
      snapshot: fx('sr-c01.snapshot.json'),
      evaluationTimeMs: 1_790_164_800_000,
    });
    assert.equal(r.completion, 'judged');
    const steps = r.evidence.core.recorded.steps;
    for (const id of ['topic', 'frustration', 'explicit_deadline']) {
      const s = steps.find((x) => x.step === id);
      assert.equal(s.status.resolved.derivation, 'model-derived', id);
    }
    // Registered twice on one engine: the runtime refuses the duplicate id.
    await assert.rejects(
      rustev.Engine.create({
        runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 3, sinkTimeoutMs: 1000 },
        sink: { queueCapacity: 1, deliver: async () => 'ack' },
        localEmbedBackends: [
          { backend, maxConcurrency: 1 },
          { backend, maxConcurrency: 1 },
        ],
      }),
      (e) => e instanceof rustev.ConfigRefused && e.code === 'duplicate_backend',
    );
  } finally {
    await engine.shutdown();
  }
});
