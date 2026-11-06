// Spec 063 increment 2: explicit profiles, shared command bytes and identities.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
const rustev = createRequire(import.meta.url)('../index.js');
const root = resolve(import.meta.dirname, '../../..');
const fixture = (name, file) => readFileSync(join(root, 'tools/rustev-conformance/fixtures/cli', `parameterized.${name}`, file));
const document = (name, file) => JSON.parse(fixture(name, file));
const json = bytes => JSON.parse(Buffer.from(bytes));
const descriptor = fixture('compile', 'descriptor');
async function engine(t) {
  const e = await rustev.Engine.create({
    clock: 'manual', runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 4, sinkTimeoutMs: 1000 },
    sink: { queueCapacity: 1, acceptsRun3: true, deliver: async () => { throw Error('unexpected decision'); } },
    backends: [{ descriptor, costModel: 'bounded', costBound: { bounded: { max_units: 1 } },
      maxConcurrency: 1, queueCapacity: 1, infer: async () => { throw Error('unexpected inference'); } }],
  });
  t.after(() => e.shutdown());
  return e;
}
const compileArgs = () => ({ definition: fixture('compile', 'template'), profile: fixture('compile', 'on'), variant: 'interactive', descriptors: [descriptor] });

test('usage rules precede document serialization and parsing on both compile paths', async t => {
  const e = await engine(t);
  const bomb = { toJSON() { throw Error('document was read'); } };
  for (const [args, field] of [
    [{ definition: bomb, definitions: [bomb] }, 'definitions'],
    [{ definition: bomb, profileBases: [bomb] }, 'profileBases'],
    [{ definition: bomb, definitionVersion: '2.1.0' }, 'definitionVersion'],
    [{ definitions: [bomb, bomb] }, 'definitions'],
    [{ definition: bomb, profile: bomb, profileBases: [] }, 'profileBases'],
    [{ definitions: [] }, 'definitions'],
    [{ definitions: Array(65).fill(bomb), profile: bomb }, 'definitions'],
    [{ definition: bomb, profile: bomb, profileBases: Array(65).fill(bomb) }, 'profileBases'],
    ...['definitions', 'profile', 'profileBases', 'variant', 'definitionVersion'].map(k => [{ plan: bomb, [k]: k === 'definitions' || k === 'profileBases' ? [bomb] : bomb }, 'plan']),
  ]) {
    const refused = err => err instanceof rustev.InvalidDocument && String(err.detail).includes(field);
    assert.throws(() => e.compileSync(args), refused);
    await assert.rejects(e.compile(args), refused);
  }
  for (const args of [{ definition: 'missing', profileBases: ['missing'] },
    { definition: 'missing', definitionVersion: '2.1.0' }, { definitions: ['a', 'b'] },
    { definition: 'missing', definitions: ['missing'] }, { definitions: ['a'], profile: 'p', profileBases: [] },
    { plan: 'p', definition: 'd' }]) {
    for (const name of ['check', 'compile', 'profileCheck']) {
      assert.throws(() => e.commandSync(name, {}, args), rustev.InvalidDocument);
    }
  }
});

// The CLI's committed golden for the same inputs, and the command path
// (`plan_command`, which `rustev plan compile` calls), so no CLI build is
// needed beside the addon.
test('compile and compileSync equal the CLI PlanId and canonical plan bytes', async t => {
  const e = await engine(t);
  const a = compileArgs();
  const sync = e.compileSync(a), asyncPlan = await e.compile(a);
  const cli = json(readFileSync(join(root, 'crates/rustev-cli/tests/golden/parameterized.compile.json')));
  assert.equal(sync.planId, cli.plan_id);
  assert.equal(asyncPlan.planId, cli.plan_id);
  const r = e.commandSync('compile', { template: a.definition, on: a.profile, descriptor },
    { definition: 'template', profile: 'on', variant: 'interactive', descriptors: ['descriptor'], out: 'plan' });
  assert.equal(r.code, 0);
  assert.deepEqual(json(r.out), cli);
  assert.deepEqual(Buffer.from(sync.raw), Buffer.from(r.artifacts.plan));
  const multi = await e.compile({ ...a, definition: undefined, definitions: [a.definition], definitionVersion: '2.1.0' });
  assert.equal(multi.planId, sync.planId);
});

test('instantiation and later compile refusals preserve the shared detail object', async t => {
  const e = await engine(t);
  const expected = json(readFileSync(join(root, 'crates/rustev-cli/tests/golden/parameterized.variant-required.json')));
  delete expected.command; delete expected.status;
  const a = { definition: fixture('compile', 'template') };
  const refused = err => {
    assert.ok(err instanceof rustev.CompileRefusal);
    assert.equal(err.code, 'variant_required'); assert.deepEqual(err.detail, expected); return true;
  };
  assert.throws(() => e.compileSync(a), refused);
  await assert.rejects(e.compile(a), refused);
  const later = json(readFileSync(join(root, 'crates/rustev-cli/tests/golden/parameterized.compile-refused.json')));
  delete later.command; delete later.status;
  await assert.rejects(e.compile({ definition: fixture('compile-refused', 'template'), profile: fixture('compile-refused', 'bad'), variant: 'batch', descriptors: [descriptor] }), err => {
    assert.ok(err instanceof rustev.CompileRefusal);
    assert.equal(err.code, later.category_name); assert.deepEqual(err.detail, later); return true;
  });
});

test('profileCheck and planDiff return happy and refusal results', async t => {
  const e = await engine(t);
  const inputs = { template: fixture('compile', 'template'), on: fixture('compile', 'on'), descriptor };
  const p = e.profileCheck(inputs, { definitions: ['template'], profile: 'on', variant: 'interactive' });
  assert.equal(p.code, 0);
  assert.equal(json(p.out).command, 'profile check');
  assert.equal(e.profileCheck(inputs, { definitions: ['template'], variant: 'absent' }).code, 5);
  const diff = await e.planDiff(inputs, { definitions: ['template'], leftProfile: 'on', rightProfile: 'on', leftVariant: 'interactive', rightVariant: 'interactive', descriptors: ['descriptor'], out: 'diff' });
  assert.equal(diff.code, 0); assert.equal(json(diff.out).identical, true); assert.ok(diff.artifacts.diff);
  const refused = await e.planDiff(inputs, { definitions: ['template'], leftVariant: 'absent', out: 'diff' });
  assert.equal(refused.code, 5); assert.equal(json(refused.out).side, 'left'); assert.deepEqual(refused.artifacts, {});
  const p1 = e.compileSync(compileArgs());
  const plans = await e.planDiff({ l: p1.raw, r: p1.raw }, { leftPlan: 'l', rightPlan: 'r', out: 'diff' });
  assert.equal(plans.code, 0); assert.equal(json(plans.out).identical, true);
  for (const [args, rule] of [
    [{ leftPlan: 'missing', definitions: ['missing'] }, '(a)'], [{}, '(a)'],
    [{ leftPlan: 'missing', profileBases: [] }, '(a)'],
    [{ leftPlan: 'missing' }, '(b)'], [{ definitions: ['missing'] }, '(c)'],
    [{ definitions: ['missing'], leftVariant: 'x' }, '(d)'],
  ]) await assert.rejects(e.planDiff({}, args), err => err instanceof rustev.InvalidDocument && err.detail.includes(rule));
});

test('execution enables persistent-tier compilation before the absent store refuses preparation', async t => {
  const e = await engine(t);
  const a = compileArgs();
  const execution = { schema: 'rustev.execution/1', cost: 'unlimited', max_attempts_per_decision: 1000, steps: [],
    optimization: { key_schema_version: 1, cache_namespace_version: 1, admission: 'refuse', expiry: 'injected_runtime_time',
      persistent_cache: { store_contract: Object.fromEntries(['schema', 'namespace', 'durability', 'encryption', 'atomicity', 'conflict', 'expiry', 'invalidation', 'erasure'].map(k => [k, `synthetic ${k}`])),
        max_entry_bytes: 16384, max_scope_bytes: 1000000, max_namespace_bytes: 10000000, ttl_ms: 60000, on_store_failure: 'continue_without_cache' } } };
  const r = await e.command('compile', { template: a.definition, on: a.profile, descriptor, execution },
    { definition: 'template', profile: 'on', variant: 'interactive', descriptors: ['descriptor'], execution: 'execution', out: 'plan' });
  assert.equal(r.code, 0, Buffer.from(r.out).toString());
  assert.deepEqual(json(r.artifacts.plan).execution.declared.policy, execution);
  for (const method of ['compile', 'compileSync']) {
    const refusal = err => err instanceof rustev.PlanPrepareRefused && err.code === 'store_missing';
    if (method === 'compile') await assert.rejects(e.compile({ ...a, execution }), refusal);
    else assert.throws(() => e.compileSync({ ...a, execution }), refusal);
  }
});

test('base chains and version pins are explicit and preserved in plan identity', async t => {
  const e = await engine(t);
  const old = document('compile', 'template');
  const newer = { ...old, version: '2.2.0' };
  const leaf = { ...document('compile', 'on'), profile: 'leaf', extends: 'off@1.0.0' };
  const base = { ...document('compile', 'on'), profile: 'off', values: { cutoff: 2, enabled: false } };
  const a = { definitions: [old, newer], profile: leaf, profileBases: [base], variant: 'batch', definitionVersion: '2.1.0', descriptors: [descriptor] };
  const p = await e.compile(a);
  const result = await e.command('compile', { old, newer, leaf, base, descriptor },
    { definitions: ['old', 'newer'], profile: 'leaf', profileBases: ['base'], variant: 'batch', definitionVersion: '2.1.0', descriptors: ['descriptor'], out: 'plan' });
  assert.equal(result.code, 0);
  assert.equal(p.planId, json(result.out).plan_id);
  assert.equal(json(result.out).definition_version, '2.1.0');
  const missing = { ...a, profileBases: undefined };
  await assert.rejects(e.compile(missing), err => err instanceof rustev.CompileRefusal && err.code === 'profile_missing');
});

test('a definition over the definition byte bound is InvalidDocument before template detection', async t => {
  const e = await engine(t);
  const big = Buffer.concat([fixture('compile', 'template'), Buffer.alloc(1024 * 1024, 0x20)]);
  assert.throws(() => e.compileSync({ definition: big }), err => err instanceof rustev.InvalidDocument && String(err.detail).includes('max_bytes'));
});
