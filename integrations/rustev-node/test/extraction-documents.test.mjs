// Spec 073 increment 2: extraction programs and prompt templates on
// `compile`, `compileSync` and `command('check' | 'compile')`. The inputs
// are the core's shared rules-first golden (3.7), a SYNTHETIC fictional
// airline confirmation; every path must give the Rust path's PlanId and
// plan bytes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
const rustev = createRequire(import.meta.url)('../index.js');
const root = resolve(import.meta.dirname, '../../..');
const golden = file => readFileSync(join(root, 'crates/rustev-core/tests/golden/rules-first', file));
const json = bytes => JSON.parse(Buffer.from(bytes));
const definition = golden('definition.json');
const descriptor = golden('descriptor.json');
const program = golden('program.json');
const template = golden('template.json');
const planId = golden('plan.id').toString();

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
const args = extra => ({ definition, descriptors: [descriptor], extractRules: [program], templates: [template], ...extra });

test('a rules_first plan compiles to the Rust PlanId and plan bytes on every path', async t => {
  const e = await engine(t);
  const sync = e.compileSync(args());
  const asyncPlan = await e.compile(args());
  assert.equal(sync.planId, planId);
  assert.equal(asyncPlan.planId, planId);
  assert.deepEqual(Buffer.from(sync.raw), golden('plan.json'));
  assert.match(Buffer.from(sync.raw).toString(), /"rules_first"/);
  const inputs = { definition, descriptor, program, template };
  const named = { definition: 'definition', descriptors: ['descriptor'], extractRules: ['program'], templates: ['template'] };
  const r = e.commandSync('compile', inputs, { ...named, out: 'plan' });
  assert.equal(r.code, 0);
  assert.equal(json(r.out).plan_id, planId);
  assert.deepEqual(Buffer.from(r.artifacts.plan), golden('plan.json'));
  // The CLI's committed golden for the same inputs.
  const cli = json(readFileSync(join(root, 'crates/rustev-cli/tests/golden/extraction-documents.check.json')));
  const c = await e.command('check', inputs, named);
  assert.deepEqual(json(c.out), cli);
  // Section 5: document order and an unnamed program change nothing.
  const other = JSON.parse(program); other.name = 'unrelated';
  assert.equal(e.compileSync(args({ extractRules: [other, program] })).planId, planId);
});

test('without the new fields, a call is unchanged', async t => {
  // I-2: the definition still compiles to the core's refusal it gave before
  // (its template, checked first, and its program are not supplied).
  const e = await engine(t);
  assert.throws(() => e.compileSync({ definition, descriptors: [descriptor] }),
    err => err instanceof rustev.CompileRefusal && err.detail.subject === 'step:read_confirmation');
});

test('compile refusals carry the core category, as the CLI', async t => {
  const e = await engine(t);
  const cli = json(readFileSync(join(root, 'crates/rustev-cli/tests/golden/extraction-documents.no-program.json')));
  const refused = err => err instanceof rustev.CompileRefusal && err.code === cli.category_name;
  assert.throws(() => e.compileSync(args({ extractRules: undefined })), refused);
  await assert.rejects(e.compile(args({ extractRules: undefined })), refused);
  const r = e.commandSync('check', { definition, descriptor, template },
    { definition: 'definition', descriptors: ['descriptor'], templates: ['template'] });
  assert.equal(r.code, 5);
  assert.deepEqual(json(r.out), cli);
  const conflict = JSON.parse(program); conflict.fields[0].rules[0].name = 'other';
  assert.throws(() => e.compileSync(args({ extractRules: [program, conflict] })),
    err => err instanceof rustev.CompileRefusal && err.code === 'invalid_extraction');
});

test('a malformed document is InvalidDocument naming the field', async t => {
  const e = await engine(t);
  const v1 = JSON.parse(program); v1.schema = 'rustev.extract-rules/1';
  for (const [extra, field] of [
    [{ extractRules: [Buffer.from('not json')] }, 'extractRules[0]'],
    [{ templates: [Buffer.from('not json')] }, 'templates[0]'],
    [{ extractRules: [v1] }, 'extractRules[0]'],
  ]) {
    const refused = err => err instanceof rustev.InvalidDocument && String(err.detail).includes(field);
    assert.throws(() => e.compileSync(args(extra)), refused);
    await assert.rejects(e.compile(args(extra)), refused);
  }
});

test('an instantiation or a plan with the new fields is refused before any parse', async t => {
  // 3.2.2 and 3.3.4.
  const e = await engine(t);
  const bomb = { toJSON() { throw Error('document was read'); } };
  for (const [a, field] of [
    [{ plan: bomb, extractRules: [bomb] }, 'extractRules'],
    [{ plan: bomb, templates: [bomb] }, 'templates'],
    [{ definition: bomb, profile: bomb, extractRules: [bomb] }, 'profile'],
    [{ definition: bomb, variant: 'a', templates: [bomb] }, 'variant'],
  ]) {
    const refused = err => err instanceof rustev.InvalidDocument && String(err.detail).includes(field);
    assert.throws(() => e.compileSync(a), refused);
    await assert.rejects(e.compile(a), refused);
  }
  for (const a of [{ definition: 'd', profile: 'p', extractRules: ['x'] },
    { definition: 'd', variant: 'v', templates: ['x'] }, { plan: 'p', extractRules: ['x'] }]) {
    for (const name of ['check', 'compile']) {
      assert.throws(() => e.commandSync(name, {}, a), rustev.InvalidDocument);
    }
  }
  // A parameterized definition, found after it is read: `compile` throws;
  // `command` returns the CLI's bytes, `invalid_input` with exit 4 (3.3.4).
  const parameterized = JSON.parse(definition); parameterized.variants = [];
  assert.throws(() => e.compileSync(args({ definition: parameterized })),
    err => err instanceof rustev.InvalidDocument && String(err.detail).includes('extractRules'));
  const r = e.commandSync('check', { definition: Buffer.from(JSON.stringify(parameterized)), program },
    { definition: 'definition', extractRules: ['program'] });
  assert.equal(r.code, 4);
  assert.equal(json(r.out).status, 'invalid_input');
  assert.equal(json(r.out).input, 'program');
});
