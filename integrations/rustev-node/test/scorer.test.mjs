// Spec 041, 3.9: host scorers through the Node binding. A TypeScript
// scorer reproduces the committed fixture of `the_scorer_fixture_reproduces`
// (crates/rustev-cli/tests/eval.rs) byte for byte, and throwing, late,
// cancelled and queue-full scorers map as 3.9.1 states. Deterministic: a
// manual clock drives every deadline, and no test sleeps on wall time.
// SYNTHETIC fixtures from the CLI tests.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkg = join(here, '..');
const root = join(pkg, '../..');
const require = createRequire(import.meta.url);
const rustev = require(pkg);
const fixture = (n) => readFileSync(join(root, 'crates/rustev-cli/tests/golden/scorer', n));

const INPUTS = [
  'lodging.dataset.json',
  'lodging.config.json',
  'lodging.adapter.json',
  'bundles/l-1.json',
  'bundles/l-2.json',
];
const inputs = () => Object.fromEntries(INPUTS.map((n) => [n, fixture(n)]));
// The CLI test's arguments (`eval_args`).
const args = (out) => ({
  dataset: 'lodging.dataset.json',
  split: 'final_test',
  config: 'lodging.config.json',
  adapter: 'lodging.adapter.json',
  bundles: 'bundles',
  nowMs: 1_800_003_600_000,
  scope: { mode: 'principal', tenant: 'tenant-a', context_revision: 'rev-1', principal_scope: 'scope-p' },
  out,
});
const PRESENTED = JSON.parse(fixture('lodging.config.json')).scorers[0];
const views = fixture('views.json');

const UNTIL_MS = 120_000;
const until = async (cond) => {
  const start = Date.now();
  while (Date.now() - start < UNTIL_MS) {
    if (cond()) return;
    await new Promise((r) => setImmediate(r));
  }
  if (cond()) return;
  throw new Error(`waited ${UNTIL_MS} ms for a condition`);
};

const engine = () =>
  rustev.Engine.create({
    runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1000 },
    clock: 'manual',
    sink: { queueCapacity: 8, deliver: async () => 'ack' },
  });

/** The TypeScript twin of the fixture's Rust scorer (`Shown`). */
const shown = (extra = {}) => ({
  descriptor: PRESENTED,
  maxConcurrency: 2,
  queueCapacity: 8,
  score: async (_caseView, proposal) =>
    proposal.action === 'present_ranked_lodging' ? { score: 1 } : { unscored: 'not_presented' },
  ...extra,
});

/** A scorer whose calls wait until the test settles them. */
function held(extra = {}) {
  const calls = [];
  return {
    calls,
    scorer: {
      descriptor: PRESENTED,
      maxConcurrency: 1,
      queueCapacity: 1,
      score: (caseView, proposal, call) =>
        new Promise((resolve) => calls.push({ caseView, proposal, call, resolve })),
      ...extra,
    },
  };
}

const outcomes = (r) => r.scores.entries.map((e) => e.outcome);
const gauge = (e) => e.gauges().seams.find((g) => g.name === 'scorer:presented');

test('a TypeScript scorer reproduces the Rust scorer byte for byte', async () => {
  const e = await engine();
  const v = await e.evalViews(inputs(), args('views.json'));
  assert.equal(v.code, 0, Buffer.from(v.out).toString());
  assert.deepEqual(Buffer.from(v.artifacts['views.json']), fixture('views.json'));

  const r = await e.scoreViews(v.artifacts['views.json'], shown());
  assert.deepEqual(Buffer.from(r.raw), fixture('shown.scores.json'));
  assert.deepEqual(outcomes(r), [{ score: 1 }, { unscored: 'not_presented' }]);

  const report = await e.evaluate(
    { ...inputs(), 'shown.scores.json': r.raw },
    { ...args('scored'), scores: ['shown.scores.json'] },
  );
  assert.equal(report.code, 0, Buffer.from(report.out).toString());
  assert.deepEqual(Buffer.from(report.artifacts['scored/report.json']), fixture('report.json'));
  assert.deepEqual(Buffer.from(report.artifacts['scored/views.json']), fixture('views.json'));
});

test('a throwing or rejecting scorer is scorer_failed, never a score', async () => {
  const e = await engine();
  const thrown = await e.scoreViews(views, shown({ score: () => { throw new Error('boom'); } }));
  assert.deepEqual(outcomes(thrown), [{ unscored: 'scorer_failed' }, { unscored: 'scorer_failed' }]);
  const rejected = await e.scoreViews(views, shown({ score: async () => Promise.reject(new Error('no')) }));
  assert.deepEqual(outcomes(rejected), [{ unscored: 'scorer_failed' }, { unscored: 'scorer_failed' }]);
});

test('an answer outside the output, or a reserved reason, is invalid_score', async () => {
  const e = await engine();
  for (const answer of [{ score: 2 }, { score: NaN }, { unscored: 'scorer_failed' }, { unscored: 'Bad' }, 'x', null, { score: 1, unscored: 'both' }]) {
    const r = await e.scoreViews(views, shown({ score: async () => answer }));
    assert.deepEqual(outcomes(r)[0], { unscored: 'invalid_score' }, JSON.stringify(answer));
  }
});

test('gold is withheld unless the scorer declares reads_gold', async () => {
  const e = await engine();
  const seen = [];
  const record = async (caseView, _p, call) => {
    seen.push([caseView.gold, JSON.parse(Buffer.from(call.raw.caseView).toString()).gold]);
    return { score: 1 };
  };
  await e.scoreViews(views, shown({ score: record }));
  assert.deepEqual(seen, [[null, null], [null, null]]);
  seen.length = 0;
  await e.scoreViews(views, shown({ descriptor: { ...PRESENTED, reads_gold: true }, score: record }));
  assert.equal(seen.length, 2);
  for (const [gold, raw] of seen) {
    assert.deepEqual(gold.relevance, { 'c-1': 2, 'c-2': 3, 'c-3': 0 });
    assert.deepEqual(raw, gold);
  }
});

test('a full queue is scorer_overloaded at once', async () => {
  const e = await engine();
  const h = held();
  // The first call holds the scorer's one queue slot.
  const first = e.scoreViews(views, h.scorer);
  await until(() => h.calls.length === 1);
  const second = await e.scoreViews(views, h.scorer);
  assert.deepEqual(outcomes(second), [{ unscored: 'scorer_overloaded' }, { unscored: 'scorer_overloaded' }]);
  assert.equal(gauge(e).refusedFull, 2);
  h.calls[0].resolve({ score: 1 });
  await until(() => h.calls.length === 2);
  h.calls[1].resolve({ unscored: 'not_presented' });
  assert.deepEqual(outcomes(await first), [{ score: 1 }, { unscored: 'not_presented' }]);
});

test('a late scorer is scorer_deadline, aborted, and its settlement discarded and counted', async () => {
  const e = await engine();
  const h = held();
  const pending = e.scoreViews(views, h.scorer, { deadlineMs: 50 });
  await until(() => h.calls.length === 1 && e.clockSleepers() > 0);
  assert.equal(h.calls[0].call.remainingMs, 50);
  e.advanceClock(50);
  // The second entry starts past the deadline: the host is not called.
  assert.deepEqual(outcomes(await pending), [{ unscored: 'scorer_deadline' }, { unscored: 'scorer_deadline' }]);
  assert.equal(h.calls.length, 1);
  await until(() => h.calls[0].call.signal.aborted);
  h.calls[0].resolve({ score: 1 });
  await until(() => gauge(e).lateResolutionsDiscarded === 1);
});

test('a cancelled call is scorer_deadline, and its signal is aborted', async () => {
  const e = await engine();
  const h = held();
  const controller = new AbortController();
  const pending = e.scoreViews(views, h.scorer, { signal: controller.signal });
  await until(() => h.calls.length === 1);
  assert.equal(h.calls[0].call.remainingMs, undefined);
  controller.abort();
  assert.deepEqual(outcomes(await pending), [{ unscored: 'scorer_deadline' }, { unscored: 'scorer_deadline' }]);
  await until(() => h.calls[0].call.signal.aborted);
  // An already-aborted signal calls no host at all.
  const before = h.calls.length;
  const r = await e.scoreViews(views, h.scorer, { signal: controller.signal });
  assert.deepEqual(outcomes(r), [{ unscored: 'scorer_deadline' }, { unscored: 'scorer_deadline' }]);
  assert.equal(h.calls.length, before);
  h.calls[0].resolve({ score: 1 });
  await until(() => gauge(e).lateResolutionsDiscarded === 1);
});

test('scoreViews refuses what a seam declaration may not hold', async () => {
  const e = await engine();
  const refused = async (scorer, options, C, code) =>
    assert.rejects(e.scoreViews(views, scorer, options), (err) => err instanceof C && err.code === code);
  await refused({ ...shown(), score: undefined }, {}, rustev.ConfigRefused, 'missing_scorer');
  await refused(shown({ maxConcurrency: 0 }), {}, rustev.ConfigRefused, 'out_of_range');
  await refused(shown({ queueCapacity: 65_537 }), {}, rustev.ConfigRefused, 'out_of_range');
  await refused(shown({ queueCapacity: undefined }), {}, rustev.ConfigRefused, 'out_of_range');
  await refused(shown({ descriptor: { ...PRESENTED, extra: 1 } }), {}, rustev.InvalidDocument, 'invalid_document');
  await refused(shown(), { deadlineMs: -1 }, rustev.ConfigRefused, 'out_of_range');
  await refused(shown(), { deadlineMs: 1.5 }, rustev.ConfigRefused, 'out_of_range');
  await assert.rejects(e.scoreViews('{}', shown()), (err) => err instanceof rustev.InvalidDocument);
  await assert.rejects(
    e.evalViews(inputs(), { ...args('views.json'), scores: ['x'] }),
    (err) => err instanceof rustev.InvalidDocument,
  );
});
