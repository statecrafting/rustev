// Spec 027, 3.12: the JavaScript conformance runner. One harness, one
// adapter per surface: `wasm` (the WebAssembly package) and `native` (the
// Node-API package). Every case of the shared manifest that names the
// surface runs from its pinned inputs and must produce each expected
// output's bytes under only the comparison the case declares.
//
//   node run.mjs wasm <wasm package dir>
//   node run.mjs chromium <wasm package dir>   (the same cases in headless Chromium)
//   node run.mjs native <native package dir>
//   node run.mjs native <native package dir> --local-embed
//     (only the local-embed cases, on a build with the `local-embed`
//     feature: spec 011, 3.17.5; without the flag, a build without the
//     feature must refuse them `feature_disabled`, 3.17.1)
//
// What a surface does not print is CLI presentation (3.1.3, 3.12.3): exit
// codes and the `run` envelope's paths and receipt. A native `run` case is
// therefore compared on the judgment and run-record bytes the envelope
// carries; a pure command's output is compared whole, exit code included.

import { readFileSync, realpathSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const [surface, pkg, flag] = process.argv.slice(2);
const localEmbedOnly = flag === '--local-embed';
if (!['wasm', 'chromium', 'native'].includes(surface) || !pkg || (flag !== undefined && !(localEmbedOnly && surface === 'native'))) {
  console.error('usage: node run.mjs wasm|chromium|native <package dir> [--local-embed (native)]');
  process.exit(2);
}

function pinned(path, sha256) {
  const b = readFileSync(join(root, path));
  const got = createHash('sha256').update(b).digest('hex');
  if (got !== sha256) throw new Error(`${path}: sha256 ${got}, the manifest pins ${sha256}`);
  return b;
}

// The flags of a CLI argument vector: the command words, then each flag's
// values in order.
function flags(argv) {
  const words = [];
  const f = new Map();
  let i = 0;
  while (i < argv.length && !argv[i].startsWith('--')) words.push(argv[i++]);
  while (i < argv.length) {
    const k = argv[i].slice(2);
    const v = argv[i + 1];
    // Every flag of the harvested commands takes one value.
    if (v === undefined || v.startsWith('--')) throw new Error(`--${k} has no value in ${argv.join(' ')}`);
    i += 2;
    if (!f.has(k)) f.set(k, []);
    f.get(k).push(v);
  }
  // `need`: a flag the command cannot run without; absent, the case fails
  // naming it rather than passing an invalid value on.
  const need = (k) => {
    const v = f.get(k)?.[0];
    if (v === undefined) throw new Error(`--${k} is required in ${argv.join(' ')}`);
    return v;
  };
  return { command: words.join(' '), one: (k) => f.get(k)?.[0], need, many: (k) => f.get(k) ?? [] };
}

// The trusted scope the CLI's flags give (spec 006, 3.1.3).
function scope(a) {
  // Each mode's fields are required: a case without them fails by name.
  const mode = a.one('scope') ?? 'principal';
  return mode === 'tenant-only'
    ? { mode: 'tenant_only', tenant: a.need('tenant'), context_revision: a.need('context-revision') }
    : {
        mode: 'principal',
        tenant: a.need('tenant'),
        context_revision: a.need('context-revision'),
        principal_scope: a.need('principal-scope'),
      };
}

/** The raw bytes of a top-level member of a compact JSON object. */
function rawMember(json, key) {
  // The bytes of one top-level member of canonical JSON, found by walking
  // the document's structure: a key nested deeper, or text inside a string,
  // never matches.
  const text = json.toString('utf8');
  let depth = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === '"') {
      let k = i + 1;
      while (k < text.length && text[k] !== '"') k += text[k] === '\\' ? 2 : 1;
      let after = k + 1;
      while (/\s/.test(text[after] ?? '')) after++;
      const isKey = depth === 1 && text[after] === ':';
      if (isKey && JSON.parse(text.slice(i, k + 1)) === key) {
        let start = after + 1;
        while (/\s/.test(text[start] ?? '')) start++;
        return valueAt(text, start);
      }
      i = k;
    } else if (ch === '{' || ch === '[') depth++;
    else if (ch === '}' || ch === ']') depth--;
  }
  return null;
}

/** The bytes of the JSON value starting at `start`. */
function valueAt(text, start) {
  // A container ends at its matching bracket, included; a scalar ends
  // before the `,`, `}` or `]` that follows it.
  const open = text[start];
  if (open === '{' || open === '[') {
    let depth = 0;
    for (let i = start; i < text.length; i++) {
      const ch = text[i];
      if (ch === '"') {
        i++;
        while (i < text.length && text[i] !== '"') i += text[i] === '\\' ? 2 : 1;
      } else if (ch === '{' || ch === '[') depth++;
      else if ((ch === '}' || ch === ']') && --depth === 0) {
        return Buffer.from(text.slice(start, i + 1), 'utf8');
      }
    }
    return Buffer.from(text.slice(start), 'utf8');
  }
  let i = start;
  if (open === '"') {
    i++;
    while (i < text.length && text[i] !== '"') i += text[i] === '\\' ? 2 : 1;
    i++;
  }
  while (i < text.length && !/[,}\]\s]/.test(text[i])) i++;
  return Buffer.from(text.slice(start, i), 'utf8');
}

const failures = [];

/** A pure command's result against its case: the exit code, the printed
 * output, and any written output by the artifact the command published
 * under that name (`artifact(name)`), compared, never skipped. */
function comparePure(c, code, out, artifact) {
  if (c.exit !== undefined && c.exit !== null && code !== c.exit) {
    failures.push(`${c.id}: exit ${code}, expected ${c.exit}`);
  }
  for (const o of c.outputs) {
    const got = o.stream === 'stdout' ? out : artifact(o.file);
    if (got === undefined || got === null) {
      failures.push(`${c.id}: no output ${o.stream ?? o.file}`);
      continue;
    }
    if ((o.normalize ?? []).length) {
      failures.push(`${c.id}: no normalization applies to a pure command`);
      continue;
    }
    const golden = pinned(o.golden, o.sha256);
    if (!Buffer.from(got).equals(golden)) {
      failures.push(`${c.id}: ${o.golden} differs\n  got ${Buffer.from(got).toString().trim()}`);
    }
  }
}

function inputsOf(c) {
  const files = new Map();
  for (const i of c.inputs) files.set(i.name, pinned(i.path, i.sha256));
  const derived = new Map((c.derived ?? []).map((d) => [d.from, pinned(d.path, d.sha256)]));
  return { files, derived };
}

// ---- WebAssembly --------------------------------------------------------

const b64 = (bytes) => Buffer.from(bytes).toString('base64');

/** A case as one call of the WebAssembly package: plain data (bytes as
 * base64), so it crosses into a browser page unchanged. */
function wasmCall(c) {
  if (c.refusalCall) return c.refusalCall;
  const { files, derived } = inputsOf(c);
  const a = flags(c.argv);
  // A rules program's derived descriptor stands in for it (3.10.1).
  const inputs = [...files].map(([name, bytes]) => [name, b64(derived.get(name) ?? bytes)]);
  const descriptors = [...a.many('descriptor'), ...a.many('rules')];
  const calibrations = a.many('calibration');
  const call = { inputs, artifacts: c.outputs.filter((o) => o.stream !== 'stdout').map((o) => o.file) };
  // An absent optional argument is `null`, never `undefined`: a browser
  // page receives `undefined` array elements as `null`, so Node and
  // Chromium get the same value, which wasm-bindgen reads as `None`.
  const opt = (v) => v ?? null;
  switch (a.command) {
    case 'plan check':
      return { ...call, fn: 'check', args: [a.need('definition'), descriptors, calibrations, opt(a.one('execution'))] };
    case 'plan compile':
      return {
        ...call,
        fn: 'compile',
        args: [a.need('definition'), descriptors, calibrations, opt(a.one('execution')), a.need('out')],
      };
    case 'plan show':
      return { ...call, fn: 'show', args: [a.need('plan'), descriptors, calibrations] };
    case 'replay': {
      const cap = a.one('host-cap-ms');
      // The store's items by reference: the synchronous resolver answers
      // from them. An item that exists but is empty is still found: only
      // an absent one is null.
      const store = a.one('store');
      const items = {};
      if (store !== undefined) {
        for (const [name, bytes] of files) {
          if (name.startsWith(`${store}/`)) items[name.slice(store.length + 1)] = b64(bytes);
        }
      }
      return {
        ...call,
        fn: 'replay',
        args: [a.need('bundle'), JSON.stringify(scope(a)), Number(a.need('now-ms')), cap === undefined ? null : Number(cap)],
        items,
      };
    }
    default:
      // `run` is no WebAssembly command (3.1.1); the per-case catch below
      // reports a case that names one as a failure.
      throw new Error(`${c.id}: the WebAssembly surface has no command ${a.command}`);
  }
}

/** Runs a `wasmCall` against the package's glue and returns the exit code,
 * the printed output and the named artifacts, base64. Self-contained (no
 * outer name, only `atob` and `btoa`), so the same source runs in Node and,
 * by its text, in a browser page. */
function executeWasm(glue, call) {
  const bytes = (s) => Uint8Array.from(atob(s), (ch) => ch.charCodeAt(0));
  const text = (u8) => {
    let s = '';
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return btoa(s);
  };
  const inputs = new glue.Inputs();
  for (const [name, b] of call.inputs) inputs.add(name, bytes(b));
  const args = [inputs, ...call.args];
  if (call.fn === 'replay') {
    args.push((reference) => (Object.hasOwn(call.items, reference) ? bytes(call.items[reference]) : null));
  }
  const r = glue[call.fn](...args);
  const artifacts = {};
  for (const name of call.artifacts) {
    const got = r.artifact(name);
    if (got !== undefined && got !== null) artifacts[name] = text(got);
  }
  return { code: r.code, out: text(r.out), artifacts };
}

/** A `wasmCall`'s result against its case. */
function compareWasm(c, r) {
  if (c.refusalCall) {
    const out = JSON.parse(Buffer.from(r.out, 'base64').toString());
    if (r.code !== 5 || out.category !== 1 || out.category_name !== 'parse'
        || !out.detail?.includes(c.member) || !out.detail.includes(c.schema)
        || !out.detail.includes('no `parameterized` feature')
        || Object.keys(r.artifacts).length !== 0) {
      throw new Error(`${c.id}: expected parameterized parse refusal, got ${JSON.stringify(r)} ${JSON.stringify(out)}`);
    }
    return;
  }
  const fromB64 = (s) => (s === undefined ? undefined : Buffer.from(s, 'base64'));
  comparePure(c, r.code, fromB64(r.out), (name) => fromB64(r.artifacts[name]));
}

// Self-contained so Chromium runs the identical engine API and error checks.
async function executeExplain(engine, call, errors) {
  const bytes = (s) => Uint8Array.from(atob(s), (ch) => ch.charCodeAt(0));
  const base64 = (u8) => {
    let s = '';
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return btoa(s);
  };
  const files = Object.fromEntries(call.inputs.map(([n, b]) => [n, bytes(b)]));
  const pack = (e) => Object.fromEntries(Object.entries(e).map(([k, n]) => [k, files[n]]));
  const a = call.explain.args;
  const q = a.query?.diff ? { diff: { other: pack(a.query.diff.other) } } : a.query;
  const options = { ...a.options };
  if (options.redaction) options.redaction = files[options.redaction];
  let calls = 0;
  if (call.explain.resolve) options.resolve = () => { calls++; throw new Error('unexpected resolver call'); };
  try {
    const r = await engine.explain(pack(a.evidence), q, options);
    if (call.explain.refusal) throw new Error('expected InvalidInput');
    if (call.explain.probeResolve) {
      const bundle = JSON.parse(new TextDecoder().decode(files[a.evidence.bundle]));
      const retained = bundle.run?.retention?.retained;
      if (!retained) throw new Error(`probeResolve needs a bundle with a retained run record`);
      bundle.run.retention = { external: { reference: 'host-record', digest: retained.digest, expires_at_ms: retained.expires_at_ms } };
      // Retention::Retained embeds JSON UTF-8, unlike call.inputs transport base64.
      const source = new TextEncoder().encode(retained.bytes);
      let resolved = 0;
      const external = await engine.explain({ bundle: new TextEncoder().encode(JSON.stringify(bundle)) }, q, {
        ...options,
        resolve: (reference, maxBytes) => {
          if (reference !== 'host-record' || maxBytes < source.length) throw new Error('resolver arguments');
          resolved++;
          return source;
        },
      });
      const doc = JSON.parse(new TextDecoder().decode(external.explanation));
      const original = JSON.parse(new TextDecoder().decode(r.explanation));
      if (resolved !== 1 || JSON.stringify(doc.facts) !== JSON.stringify(original.facts)) throw new Error('resolver lost evidence');
    }
    return { code: 0, out: '', artifacts: { explanation: base64(r.explanation), markdown: base64(new TextEncoder().encode(r.markdown)) } };
  } catch (e) {
    if (!call.explain.refusal || e.name !== 'InvalidInput' || e.code !== call.explain.refusal || calls !== 0) throw e;
    if (!(e instanceof errors.InvalidInput) || !(e instanceof errors.RustevError) || !Object.hasOwn(e, 'detail')) {
      throw new Error('explain refusal must be an exported InvalidInput with detail');
    }
    const error = JSON.stringify({ class: e.name, code: e.code }, null, 2) + '\n';
    return { code: 0, out: '', artifacts: { error: base64(new TextEncoder().encode(error)) } };
  }
}
function explainCall(c) {
  if (c.explain.comparison !== 'bytes') throw new Error('explain must compare bytes');
  return { explain: c.explain, inputs: [...inputsOf(c).files].map(([n, b]) => [n, b.toString('base64')]) };
}

async function wasmAdapter() {
  const glue = await import(pathToFileURL(resolve(pkg, 'web/index.js')).href);
  glue.initSync({ module: readFileSync(resolve(pkg, 'web/rustev_wasm_bg.wasm')) });
  const engine = new glue.Engine();
  return async (c) => compareWasm(c, c.explain ? await executeExplain(engine, explainCall(c), glue) : executeWasm(glue, wasmCall(c)));
}

/** The same calls in headless Chromium (3.11.3, 3.13.3): one page holds the
 * `web` target, served from loopback, and runs each case's call there. */
async function chromiumAdapter() {
  const { serve, open } = await import('./chromium.mjs');
  const server = await serve(pkg);
  const { browser, page } = await open(server.origin);
  await page.evaluate(async () => {
    const glue = await import('/web/index.js');
    glue.initSync({ module: new Uint8Array(await (await fetch('/web/rustev_wasm_bg.wasm')).arrayBuffer()) });
    globalThis.rustevGlue = glue;
  });
  await page.addScriptTag({ content: `globalThis.executeWasm = ${executeWasm.toString()};` });
  await page.addScriptTag({ content: `globalThis.executeExplain = ${executeExplain.toString()}; globalThis.explainEngine = new globalThis.rustevGlue.Engine();` });
  const adapter = async (c) => compareWasm(c, c.explain
    ? await page.evaluate((call) => globalThis.executeExplain(globalThis.explainEngine, call, globalThis.rustevGlue), explainCall(c))
    : await page.evaluate((call) => globalThis.executeWasm(globalThis.rustevGlue, call), wasmCall(c)));
  adapter.close = async () => {
    await browser.close();
    server.close();
  };
  return adapter;
}

// ---- native ---------------------------------------------------------------

// Spec 063, 3.9: the CLI's repeatability commands and their native names;
// `eval` only with `--repeat-config`.
const REPEAT_COMMANDS = {
  eval: 'eval',
  'eval candidates': 'evalCandidates',
  'baseline record': 'baselineRecord',
  'eval reproducibility': 'evalReproducibility',
  'eval diff': 'evalDiff',
  'gate select': 'gateSelect',
  'eval repeat import': 'evalRepeatImport',
};

async function nativeAdapter() {
  const rustev = createRequire(import.meta.url)(resolve(pkg));
  // The engine for pure commands decides nothing, so its sink is never
  // delivered to; if it ever were, the case fails rather than passing.
  const quiet = {
    queueCapacity: 1,
    deliver: async () => {
      throw new Error('the pure-command engine delivered a record');
    },
  };
  const runtime = { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 4, sinkTimeoutMs: 1000 };
  const pure = await rustev.Engine.create({ runtime, sink: quiet, clock: 'manual' });

  // The native package links the rules backend (3.3.2) and reads a rules
  // program itself, exactly as the CLI does, so it gets the program's own
  // bytes; only the WebAssembly adapter, which cannot read one (3.10.1),
  // takes the derived descriptor in its place.
  async function pureCase(c, a, files) {
    const names = {
      'profile check': 'profileCheck',
      'plan diff': 'planDiff',
      'plan check': 'check',
      'plan compile': 'compile',
      'plan show': 'show',
      replay: 'replay',
    };
    const args = {
      definition: a.one('definition'),
      plan: a.one('plan'),
      descriptors: a.many('descriptor'),
      rules: a.many('rules'),
      calibrations: a.many('calibration'),
      execution: a.one('execution'),
      out: a.one('out'),
      bundle: a.one('bundle'),
      store: a.one('store'),
    };
    if (c.id.startsWith('parameterized.')) {
      delete args.definition;
      if (a.many('definition').length) args.definitions = a.many('definition');
      if (a.many('profile-base').length) args.profileBases = a.many('profile-base');
      for (const [flag, key] of [['profile', 'profile'], ['variant', 'variant'],
        ['definition-version', 'definitionVersion'], ['left-plan', 'leftPlan'],
        ['right-plan', 'rightPlan'], ['left-profile', 'leftProfile'],
        ['right-profile', 'rightProfile'], ['left-variant', 'leftVariant'], ['right-variant', 'rightVariant']]) {
        if (a.one(flag) !== undefined) args[key] = a.one(flag);
      }
      for (const key of ['descriptors', 'rules', 'calibrations']) if (!args[key].length) delete args[key];
    }
    if (a.command === 'replay') {
      // An object: the native command takes its arguments as JSON and reads
      // `scope` from it (the WebAssembly binding takes a string instead).
      args.scope = scope(a);
      args.nowMs = Number(a.need('now-ms'));
      if (a.one('host-cap-ms') !== undefined) args.hostCapMs = Number(a.one('host-cap-ms'));
    }
    if (Object.hasOwn(REPEAT_COMMANDS, a.command) && (a.command !== 'eval' || a.one('repeat-config') !== undefined)) {
      const r = await pure.command(REPEAT_COMMANDS[a.command], Object.fromEntries(files), repeatArgs(c.argv));
      return comparePure(c, r.code, r.out, (name) => r.artifacts[name]);
    }
    const r = await pure.command(names[a.command], Object.fromEntries(files), args);
    comparePure(c, r.code, r.out, (name) => r.artifacts[name]);
  }

  async function runCase(c, a, files, derived) {
    const records = [];
    const engine = await rustev.Engine.create({
      runtime: { ...runtime, maxParallelRequests: Number(a.one('max-parallel-requests') ?? 4) },
      clock: 'manual',
      sink: { queueCapacity: 4, acceptsRun3: true, deliver: async (rec) => (records.push(rec), 'ack') },
      rulesBackends: a.many('rules').map((r) => ({ program: files.get(r), maxConcurrency: 4 })),
    });
    const plan = await engine.compile({
      plan: files.get(a.need('plan')),
      descriptors: [...a.many('descriptor').map((d) => files.get(d)), ...a.many('rules').map((r) => derived.get(r))],
      calibrations: a.many('calibration').map((x) => files.get(x)),
    });
    const res = await plan.decide(
      {
        decisionId: a.need('decision-id'),
        snapshot: files.get(a.need('snapshot')),
        evaluationTimeMs: Number(a.need('evaluation-time')),
      },
      { deadlineMs: a.one('deadline-ms') === undefined ? undefined : Number(a.one('deadline-ms')) },
    );
    for (const o of c.outputs) {
      const golden = pinned(o.golden, o.sha256);
      if (o.stream === 'stdout') {
        // The envelope's judgment, byte for byte; the rest is CLI
        // presentation (paths, receipt, exit code).
        const want = rawMember(golden, 'judgment');
        if (!want || !res.raw.judgment || !Buffer.from(res.raw.judgment).equals(want)) {
          failures.push(`${c.id}: the judgment differs from ${o.golden}`);
        }
      } else {
        // The record is compared byte for byte with its golden, which holds
        // the normalized form (timings zero), and every normalization the
        // case declares must already hold: under the manual clock those
        // timings are zero, so normalizing would change nothing. An unknown
        // normalization fails the case.
        const got = Buffer.concat([Buffer.from(res.raw.evidence), Buffer.from('\n')]);
        if (!got.equals(golden)) failures.push(`${c.id}: the record differs from ${o.golden}`);
        const record = JSON.parse(Buffer.from(res.raw.evidence).toString('utf8'));
        for (const n of o.normalize ?? []) {
          if (n !== 'timing') {
            failures.push(`${c.id}: normalization ${n} does not apply to a record`);
            continue;
          }
          const timings = (r) => {
            const t = [r.timing.elapsed_ms, r.timing.queued_ms];
            for (const q of r.requests) for (const at of q.attempts) t.push(at.dispatched_ms, at.ended_ms);
            return t;
          };
          // Both sides already normalized: the golden as the CLI tests write
          // it, the record by the manual clock.
          if (timings(JSON.parse(golden.toString('utf8'))).some((x) => x !== 0)) {
            failures.push(`${c.id}: ${o.golden} is not in its normalized form (timings zero)`);
          }
          if (timings(record).some((x) => x !== 0)) failures.push(`${c.id}: the record's timings are not the normalized zeros`);
        }
      }
    }
    // The sink was delivered exactly the bytes the result carries, whatever
    // outputs the case pins.
    if (records.length !== 1 || !Buffer.from(records[0]).equals(Buffer.from(res.raw.evidence))) {
      failures.push(`${c.id}: the sink received ${records.length} record(s), not the result's`);
    }
  }

  async function runtimeCase(c, files) {
    // Each case settles its own ignored attempts when it ends, however it
    // ends, so none reaches a later case.
    const ignored = [];
    try {
      return await runtimeCaseWith(c, files, ignored);
    } finally {
      for (const settle of ignored.splice(0)) settle();
    }
  }

  async function runtimeCaseWith(c, files, ignored) {
    const rt = c.runtime;
    // Spec 063 3.13.2: a store case runs its requests in order.
    if (rt.store) return storeCaseWith(c, files, ignored);
    const gates = rt.backends.map(() => new Map());
    let backendCalls = 0;
    const engine = await rustev.Engine.create({
      runtime: {
        maxInFlight: rt.config.max_in_flight,
        maxQueued: rt.config.max_queued,
        maxParallelRequests: rt.config.max_parallel_requests,
        sinkTimeoutMs: rt.config.sink_timeout_ms,
      },
      clock: 'manual',
      sink: { queueCapacity: 64, acceptsRun3: true, deliver: async (_r, { decisionId }) => `ack:${decisionId}` },
      backends: rt.backends.map((b, bi) => ({
        descriptor: files.get(b.descriptor),
        costModel: b.cost_model,
        costBound: b.cost_bound,
        maxConcurrency: b.max_concurrency,
        queueCapacity: 65_536,
        infer: (call) => { backendCalls++; return scripted(b, gates[bi], call, ignored); },
      })),
      // Spec 063 3.13.2: scripted acquisition sources, answering per
      // decision in call order as the Rust runner's do.
      acquisitionSources: (rt.sources ?? []).map((s) => scriptedSource(s, files.get(s.descriptor), ignored)),
    });
    const plan = await engine.compile({
      plan: files.get(rt.plan),
      descriptors: rt.descriptors.map((d) => files.get(d)),
      calibrations: rt.calibrations.map((x) => files.get(x)),
      sources: (rt.sources ?? []).map((s) => files.get(s.descriptor)),
    });
    const controllers = rt.requests.map(() => new AbortController());
    const running = [];
    let next = 0;
    // Bounded by wall time, not by a count of event loop turns, which a
    // loaded runner can exhaust while the engine's threads are still
    // answering; every turn yields with setImmediate, never a timed sleep.
    const until = async (what, cond) => {
      const start = Date.now();
      while (Date.now() - start < 120_000) {
        if (cond()) return;
        await new Promise((r) => setImmediate(r));
      }
      if (cond()) return;
      throw new Error(`${c.id}: the driver waited 120000 ms for ${what}`);
    };
    for (const step of rt.drive) {
      const [kind, arg] = typeof step === 'string' ? [step, {}] : Object.entries(step)[0];
      switch (kind) {
        case 'start': {
          const i = next++;
          const r = rt.requests[i];
          running.push(
            plan
              .decide(
                { decisionId: r.decision_id, snapshot: files.get(r.snapshot), evaluationTimeMs: r.evaluation_time_ms },
                { signal: controllers[i].signal, deadlineMs: r.deadline_ms,
                  ...(rt.capture ? { capture: { scope: rt.capture.scope, maxBytes: rt.capture.max_bytes,
                    createdAtMs: rt.capture.created_at_ms, retain: rt.capture.retain[r.decision_id] } } : {}) },
              )
              .then(
                (v) => ({ ok: v }),
                (e) => ({ err: e }),
              ),
          );
          await new Promise((r) => setImmediate(r));
          break;
        }
        case 'wait_gated':
          await until('gated attempts', () => gates[arg.backend].size >= arg.count);
          break;
        case 'wait_queued':
          await until('queued decisions', () => engine.gauges().queued >= arg.count);
          break;
        case 'advance_timer':
          await until(
            'the timer, with no attempt in flight',
            () =>
              engine.clockDelays().includes(arg.delay_ms) &&
              Object.values(engine.gauges().backendInFlight).every((n) => n === 0),
          );
          engine.advanceClock(arg.delay_ms);
          break;
        case 'wait_sleeper':
          await until('a sleeper', () => engine.clockSleepers() > 0);
          break;
        case 'advance_ms':
          engine.advanceClock(arg.ms);
          break;
        case 'release_all':
          // A release that frees nothing is a script error, as in the Rust
          // runner.
          if (gates[arg.backend].size === 0) throw new Error(`release_all on backend ${arg.backend} released nothing`);
          for (const [, g] of gates[arg.backend]) g.release();
          gates[arg.backend].clear();
          break;
        case 'cancel':
          controllers[arg.request].abort();
          break;
        default:
          throw new Error(`${c.id}: no driver step ${kind}`);
      }
    }
    const out = new Map();
    const done = await Promise.all(running);
    for (const [i, d] of done.entries()) {
      const id = rt.requests[i].decision_id;
      settled(out, id, d);
      if (rt.capture) {
        const captured = d.ok?.capture;
        if (!captured?.bundle || captured.error) throw new Error(`${id}: capture failed`);
        out.set(`${id}.bundle.json`, Buffer.from(captured.bundle));
        const inputs = { 'b.json': captured.bundle };
        for (const [ref, bytes] of Object.entries(captured.items ?? {})) inputs[`items/${ref}`] = bytes;
        const calls = backendCalls;
        const replay = await engine.replay(inputs, { bundle: 'b.json', scope: rt.capture.scope,
          nowMs: rt.capture.created_at_ms, store: 'items' });
        if (replay.code !== 0 || JSON.parse(Buffer.from(replay.out)).status !== 'reproduced' || backendCalls !== calls) {
          throw new Error(`${id}: offline replay failed`);
        }
        out.set(`${id}.replay.json`, Buffer.from(replay.out));
      }
    }
    await compareOutputs(c, out);
  }

  /** A store case (spec 063, 3.13.2): one scripted store shared by every
   * engine of the case; each request runs to its end before the next. A
   * restart is a new engine over the same store; an invalidation before a
   * request is `plan.invalidate()`, its result written with sorted keys. */
  async function storeCaseWith(c, files, ignored) {
    const rt = c.runtime;
    const store = scriptedStore(rt.store);
    const engines = [];
    const open = async () => {
      const e = await rustev.Engine.create({
        runtime: {
          maxInFlight: rt.config.max_in_flight,
          maxQueued: rt.config.max_queued,
          maxParallelRequests: rt.config.max_parallel_requests,
          sinkTimeoutMs: rt.config.sink_timeout_ms,
        },
        clock: 'manual',
        sink: { queueCapacity: 64, acceptsRun3: true, deliver: async (_r, { decisionId }) => `ack:${decisionId}` },
        backends: rt.backends.map((b) => ({
          descriptor: files.get(b.descriptor),
          costModel: b.cost_model,
          costBound: b.cost_bound,
          maxConcurrency: b.max_concurrency,
          queueCapacity: 65_536,
          infer: (call) => scripted(b, new Map(), call, ignored),
        })),
        store: store.seam,
      });
      engines.push(e);
      return e;
    };
    const compile = (e, name) =>
      e.compile({
        plan: files.get(name),
        descriptors: rt.descriptors.map((d) => files.get(d)),
        calibrations: rt.calibrations.map((x) => files.get(x)),
      });
    const out = new Map();
    try {
      let engine = await open();
      let current = rt.plan;
      let plan = await compile(engine, current);
      for (const r of rt.requests) {
        const id = r.decision_id;
        store.begin(id);
        const name = rt.store.plan_for?.[id] ?? rt.plan;
        if ((rt.store.restart_before ?? []).includes(id)) {
          engine = await open();
          plan = await compile(engine, name);
        } else if (name !== current) {
          plan = await compile(engine, name);
        }
        current = name;
        if ((rt.store.invalidate_before ?? []).includes(id)) {
          const v = await plan.invalidate();
          const json =
            v === null
              ? 'null'
              : JSON.stringify({
                  complete: v.complete,
                  generation: v.generation,
                  persistent:
                    v.persistent === null || typeof v.persistent === 'string'
                      ? v.persistent
                      : { generation: v.persistent.generation, removed: v.persistent.removed },
                  removedBytes: v.removedBytes,
                  removedEntries: v.removedEntries,
                });
          out.set(`${id}.invalidation.json`, Buffer.from(json + '\n'));
        }
        const d = await plan
          .decide({ decisionId: id, snapshot: files.get(r.snapshot), evaluationTimeMs: r.evaluation_time_ms },
            { deadlineMs: r.deadline_ms })
          .then((v) => ({ ok: v }), (e) => ({ err: e }));
        settled(out, id, d);
      }
    } finally {
      for (const e of engines) await e.shutdown({ drainMs: 0 });
    }
    await compareOutputs(c, out);
  }

  /** The scripted store of a store case, as the Rust runner's: entries by
   * namespace and key with their scope, namespace and scope generations,
   * conditional put-if-absent writes, and failures from a request's start. */
  function scriptedStore(script) {
    const entries = new Map();
    const nsGen = new Map();
    const scopeGen = new Map();
    // Typed as a JSON pair, so no namespace or key id can alias another.
    const key = (a, b) => JSON.stringify([a, b]);
    let f = {};
    const failed = (what) => new Error(`SYNTHETIC ${what} failure`);
    const generations = (ns, scope) => ({ namespace: nsGen.get(ns) ?? 0, scope: scopeGen.get(key(ns, scope)) ?? 0 });
    const remove = script.remove_on_invalidate ?? true;
    return {
      begin(id) { f = script.failures?.[id] ?? {}; },
      seam: {
        contract: script.contract,
        queueCapacity: 65_536,
        async get({ namespace, scope, keyId }) {
          if (f.fail_get) throw failed('get');
          const e = entries.get(key(namespace, keyId));
          return { bytes: e ? Buffer.from(e.bytes) : null, generations: generations(namespace, scope) };
        },
        async put({ namespace, keyId, bytes, terms }) {
          if (f.fail_put) throw failed('put');
          if (f.put_answer) {
            // As the Rust scripted store: an unknown answer is a failure.
            if (!['stored', 'generation_advanced', 'over_budget', 'conflict'].includes(f.put_answer)) throw failed(`unknown put answer ${f.put_answer}`);
            return f.put_answer;
          }
          const now = generations(namespace, terms.scope);
          if (now.namespace !== terms.generations.namespace || now.scope !== terms.generations.scope) return 'generation_advanced';
          const k = key(namespace, keyId);
          if (entries.has(k)) return 'conflict';
          entries.set(k, { namespace, scope: terms.scope, bytes: Buffer.from(bytes) });
          return 'stored';
        },
        async delete({ namespace, keyId }) {
          entries.delete(key(namespace, keyId));
        },
        async invalidate({ namespace, selector }) {
          if (f.fail_invalidate) throw failed('invalidate');
          const before = entries.size;
          let generation;
          if (selector === 'namespace') {
            if (remove) for (const [k, e] of [...entries]) if (e.namespace === namespace) entries.delete(k);
            generation = (nsGen.get(namespace) ?? 0) + 1;
            nsGen.set(namespace, generation);
          } else {
            if (remove) {
              for (const [k, e] of [...entries]) if (e.namespace === namespace && e.scope === selector.scope) entries.delete(k);
            }
            const sk = key(namespace, selector.scope);
            generation = (scopeGen.get(sk) ?? 0) + 1;
            scopeGen.set(sk, generation);
          }
          return { generation, removed: before - entries.size };
        },
      },
    };
  }

  /** A decision's outputs, as the Rust runner writes them: its result and,
   * when one was produced, its run record. */
  function settled(out, id, d) {
    let status;
    let record = null;
    if (d.ok) {
      status = { decision_id: id, status: d.ok.completion };
      record = d.ok.raw.evidence;
    } else if (d.err.name === 'SinkFailure') {
      status = { decision_id: id, status: 'evidence_not_delivered' };
      record = d.err.record;
    } else {
      const kinds = {
        AdmissionRejected: d.err.code,
        DeadlineExceeded: 'queue_deadline',
        CancelledBeforeAdmission: 'cancelled',
      };
      status = { decision_id: id, rejection: kinds[d.err.name] ?? d.err.name, status: 'rejected' };
    }
    out.set(`${id}.result.json`, Buffer.from(JSON.stringify(status) + '\n'));
    if (record) out.set(`${id}.record.json`, Buffer.concat([Buffer.from(record), Buffer.from('\n')]));
  }

  /** Every output against its golden, and no output the case does not pin. */
  async function compareOutputs(c, out) {
    if (process.env.RUSTEV_CONFORMANCE_DUMP) {
      const { mkdirSync, writeFileSync } = await import('node:fs');
      // Plain names only: a case id or output name cannot leave the dump
      // directory.
      const plain = (n) => /^[A-Za-z0-9._-]+$/.test(n) && n !== '.' && n !== '..';
      if (!plain(c.id) || [...out.keys()].some((f) => !plain(f))) {
        throw new Error(`${c.id}: a case id or output name is not a plain file name`);
      }
      const d = join(process.env.RUSTEV_CONFORMANCE_DUMP, c.id);
      mkdirSync(d, { recursive: true });
      for (const [f, b] of out) writeFileSync(join(d, f), b);
    }
    for (const o of c.outputs) {
      const golden = pinned(o.golden, o.sha256);
      const got = out.get(o.file);
      if (!got || !got.equals(golden)) failures.push(`${c.id}: ${o.file} differs from ${o.golden}`);
    }
    if (out.size !== c.outputs.length) failures.push(`${c.id}: ${out.size} outputs, expected ${c.outputs.length}`);
  }

  /** A local-embed case (spec 011, 3.17.5): the local semantic backend
   * opened from the case's SYNTHETIC bundle, registered like a rules
   * backend, deciding the requests one after another on a manual clock. */
  async function localEmbedCase(c, files) {
    const le = c.local_embed;
    const config = {
      bundleDir: realpathSync(join(root, le.bundle)),
      expectedArtifact: le.expected_artifact,
      workers: le.workers,
      maxQueued: le.max_queued,
    };
    if (!rustev.features().localEmbed) {
      // 3.17.1: a build without the feature refuses, typed, and opens nothing.
      let refusal;
      try {
        await rustev.LocalEmbedBackend.open(config);
      } catch (e) {
        refusal = e;
      }
      if (!(refusal instanceof rustev.ConfigRefused) || refusal.code !== 'feature_disabled') {
        failures.push(`${c.id}: a build without local-embed answered ${refusal ?? 'with a backend'}, not feature_disabled`);
      }
      return;
    }
    const backend = await rustev.LocalEmbedBackend.open(config);
    // The descriptor the backend derives is the one the case pins.
    if (!le.descriptors.some((d) => Buffer.from(files.get(d)).equals(Buffer.concat([Buffer.from(backend.descriptor), Buffer.from('\n')])))) {
      failures.push(`${c.id}: the opened backend's descriptor is not among the case's`);
    }
    const engine = await rustev.Engine.create({
      runtime: {
        maxInFlight: le.config.max_in_flight,
        maxQueued: le.config.max_queued,
        maxParallelRequests: le.config.max_parallel_requests,
        sinkTimeoutMs: le.config.sink_timeout_ms,
      },
      clock: 'manual',
      sink: { queueCapacity: 64, acceptsRun3: true, deliver: async (_r, { decisionId }) => `ack:${decisionId}` },
      localEmbedBackends: [{ backend, maxConcurrency: le.max_concurrency }],
    });
    try {
      const plan = await engine.compile({
        plan: files.get(le.plan),
        descriptors: le.descriptors.map((d) => files.get(d)),
        calibrations: (le.calibrations ?? []).map((x) => files.get(x)),
      });
      const out = new Map();
      for (const r of le.requests) {
        const d = await plan
          .decide(
            { decisionId: r.decision_id, snapshot: files.get(r.snapshot), evaluationTimeMs: r.evaluation_time_ms },
            { principalHandle: Buffer.from(r.principal_handle, 'utf8') },
          )
          .then(
            (v) => ({ ok: v }),
            (e) => ({ err: e }),
          );
        if (d.ok?.raw.judgment) {
          out.set(`${r.decision_id}.judgment.json`, Buffer.concat([Buffer.from(d.ok.raw.judgment), Buffer.from('\n')]));
        }
        settled(out, r.decision_id, d);
      }
      await compareOutputs(c, out);
    } finally {
      await engine.shutdown();
    }
  }

  /** A scripted host backend from a runtime case's data (3.12.4).
   * `ignored`: this case's settlements of attempts whose backend ignored
   * cancellation, run when the case ends. */
  function scripted(b, gates, call, ignored) {
    const n = Number(call.attemptId.split('/').pop()) || 1;
    const task = JSON.parse(Buffer.from(call.projection).toString('utf8')).task;
    const list = b.answers[task] ?? b.answers['*'] ?? [];
    const answer = list.length ? list[Math.min(Math.max(n, 1), list.length) - 1] : null;
    const remote = b.remote;
    const settle = (a) => {
      if (a === null) {
        throw new rustev.SeamFailure({ failureClass: 'permanent', charge: { observed: { units: 0 } }, remote, detail: `no scripted answer for ${task}` });
      }
      if (a.output) return { output: a.output.output, charge: a.output.charge, remote };
      if (a.fail) {
        throw new rustev.SeamFailure({ failureClass: a.fail.class, charge: a.fail.charge, remote, detail: a.fail.detail });
      }
      throw new Error(`unknown answer ${JSON.stringify(a)}`);
    };
    if (answer !== 'gate') return Promise.resolve().then(() => settle(answer));
    return new Promise((resolveP, rejectP) => {
      gates.set(call.attemptId, {
        release: () => {
          try {
            resolveP(settle(b.on_release?.[task] ?? null));
          } catch (e) {
            rejectP(e);
          }
        },
      });
      call.signal.addEventListener('abort', () => {
        gates.delete(call.attemptId);
        const on = b.on_cancel;
        if (on === 'ignore') {
          // A backend that ignores cancellation answers only after the
          // case ends, as a late resolution the engine discards.
          // Resolved, not rejected: a resolution can never surface as an
          // unhandled rejection. Its value is never read: the native seam
          // takes a settlement as plain JSON and, the attempt already
          // finished, discards and counts it before anything parses it.
          ignored.push(() => resolveP({ output: {}, charge: { observed: { units: 0 } }, remote }));
          return;
        }
        if (on === 'unconfirmed') {
          rejectP(new rustev.SeamFailure({ failureClass: 'cancelled', charge: 'unknown', remote, cancel: 'unconfirmed', detail: 'stopped waiting' }));
        } else if (on?.stop) {
          rejectP(new rustev.SeamFailure({ failureClass: 'cancelled', charge: on.stop.charge, remote, cancel: 'stopped', detail: 'stopped' }));
        } else {
          // A script error, failed at once rather than left waiting.
          rejectP(new Error(`unknown on_cancel ${JSON.stringify(on)}`));
        }
      });
    });
  }

  /** A scripted acquisition source from a runtime case's data (spec 063,
   * 3.13.2): per decision, the answer of each call in order (the last
   * repeats). A page is written with its keys sorted at every level, the
   * bytes the Rust runner writes. `ignore` never answers while the case
   * runs: it resolves when the case ends, a late settlement the engine
   * discards. */
  function scriptedSource(s, descriptor, ignored) {
    const calls = new Map();
    const sorted = (v) =>
      Array.isArray(v)
        ? `[${v.map(sorted).join(',')}]`
        : v !== null && typeof v === 'object'
          ? `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${sorted(v[k])}`).join(',')}}`
          : JSON.stringify(v);
    return {
      descriptor,
      maxConcurrency: s.max_concurrency,
      queueCapacity: 65_536,
      acquire: (call) => {
        const n = (calls.get(call.decisionId) ?? 0) + 1;
        calls.set(call.decisionId, n);
        const list = s.answers[call.decisionId] ?? [];
        const a = list.length ? list[Math.min(n, list.length) - 1] : null;
        if (a === 'ignore') {
          return new Promise((resolveP) => {
            ignored.push(() => resolveP({ page: 'late', charge: { observed: { units: 0 } } }));
          });
        }
        return Promise.resolve().then(() => {
          if (a === null) {
            throw new rustev.SeamFailure({ failureClass: 'permanent', charge: { observed: { units: 0 } }, detail: `no scripted answer for ${call.decisionId}` });
          }
          if (a.page) {
            const { items, next, as_of_ms: asOf, charge } = a.page;
            const page = { as_of_ms: asOf, items, next, request: call.requestId, schema: 'rustev.acquisition-page/1' };
            return { page: sorted(page), charge };
          }
          if (a.invalid) return { page: 'invalid page', charge: a.invalid.charge };
          if (a.fail) throw new rustev.SeamFailure({ failureClass: a.fail.class, charge: a.fail.charge, detail: a.fail.detail });
          throw new Error(`unknown source answer ${JSON.stringify(a)}`);
        });
      },
    };
  }

  return async (c) => {
    if (c.explain) return compareWasm(c, await executeExplain(pure, explainCall(c), rustev));
    const { files, derived } = inputsOf(c);
    if (c.local_embed) return localEmbedCase(c, files);
    if (c.runtime) return runtimeCase(c, files);
    const a = flags(c.argv);
    if (a.command === 'run') return runCase(c, a, files, derived);
    return pureCase(c, a, files);
  };
}

// Spec 050: select spec 034 cases by their inputs, without changing the
// shared manifest or the golden bytes used by native and CLI runners.
function parameterizedDocument(v) {
  if (!v || typeof v !== 'object' || typeof v.schema !== 'string') return false;
  if (v.schema?.startsWith('rustev.plan/')) return Object.hasOwn(v, 'instantiation');
  if (!v.schema?.startsWith('rustev.definition/')) return false;
  if (Object.hasOwn(v, 'parameters') || Object.hasOwn(v, 'variants')) return true;
  const array = (a) => Array.isArray(a) ? a : [];
  const has = (i, key) => i !== null && typeof i === 'object' && Object.hasOwn(i, key);
  const items = [...array(v.steps), ...array(v.policy?.rules),
    ...array(v.policy?.adjustments), ...array(v.policy?.on_unresolved)];
  if (array(v.inputs).some((i) => has(i, 'only_in'))
      || items.some((i) => has(i, 'only_in') || has(i, 'enabled'))) return true;
  const reference = (x) => x && typeof x === 'object'
    && ((Object.keys(x).length === 1 && typeof x.param === 'string') || Object.values(x).some(reference));
  return reference(v);
}

function requiresParameterized(c) {
  if (c.explain) return false;
  if ((c.argv ?? []).some((a) => ['--profile', '--variant', '--base-profile'].includes(a))) return true;
  return [...inputsOf(c).files.values()].some((b) => {
    let v;
    try { v = JSON.parse(b); } catch { return false; }
    return parameterizedDocument(v);
  });
}

function featureOffCases(cases) {
  const documents = cases.flatMap((c) => [...inputsOf(c).files.values()]).flatMap((b) => {
    try { return [JSON.parse(b)]; } catch { return []; }
  });
  const definition = documents.find((v) => v.schema?.startsWith('rustev.definition/') && v.inputs?.length && v.steps?.length && !parameterizedDocument(v));
  const plan = documents.find((v) => v.schema === 'rustev.plan/3' && !parameterizedDocument(v));
  if (!definition || !plan) throw new Error('spec 050 needs existing 032/033 fixtures');
  const result = [];
  for (const member of ['parameters', 'variants', 'only_in', 'enabled', 'param', 'instantiation']) {
    const document = structuredClone(member === 'instantiation' ? plan : definition);
    if (member !== 'instantiation') document.schema = 'rustev.definition/2';
    if (member === 'instantiation') document.instantiation = {};
    else if (member === 'parameters' || member === 'variants') document[member] = [];
    else if (member === 'only_in') document.inputs[0].only_in = ['test'];
    else if (member === 'enabled') document.steps[0].enabled = true;
    else document.limits.deadline_ms = { param: 'deadline' };
    for (const fn of member === 'instantiation' ? ['show'] : ['check', 'compile']) {
      result.push({
        id: `050.${fn}.${member}`, member, schema: document.schema,
        refusalCall: {
          fn, inputs: [['input.json', b64(JSON.stringify(document))]],
          args: fn === 'show' ? ['input.json', [], []]
            : ['input.json', [], [], null, ...(fn === 'compile' ? ['out.plan.json'] : [])],
          artifacts: ['out.plan.json'],
        },
      });
    }
  }
  // Profile and variant arguments are not exposed by the wasm binding yet.
  return result;
}

const manifest = JSON.parse(readFileSync(join(root, 'tools/rustev-conformance/cases/manifest.json'), 'utf8'));
if (manifest.schema !== 'rustev.conformance-manifest/1') throw new Error('manifest schema');
const run =
  surface === 'wasm' ? await wasmAdapter() : surface === 'chromium' ? await chromiumAdapter() : await nativeAdapter();
// The Chromium adapter runs the cases that name the WebAssembly surface.
const named = surface === 'chromium' ? 'wasm' : surface;
let ran = 0;
let selected = manifest.cases.filter((c) => c.surfaces.includes(named) && (!localEmbedOnly || c.local_embed));
if (named === 'wasm') {
  const supported = selected.filter((c) => !requiresParameterized(c));
  console.log(`conformance ${surface}: skipped ${selected.length - supported.length} parameterized case(s)`);
  selected = [...supported, ...featureOffCases(supported)];
}
for (const c of selected) {
  if (process.env.RUSTEV_CONFORMANCE_TRACE) console.error(`case ${c.id}`);
  try {
    await run(c);
  } catch (e) {
    failures.push(`${c.id}: ${e.stack ?? e}`);
  }
  ran += 1;
}
await run.close?.();
console.log(`conformance ${surface}: ${ran} case(s), ${failures.length} failure(s)`);
for (const f of failures) console.error(f);
process.exit(failures.length || ran === 0 ? 1 : 0);

// Spec 063, 3.9: the repeatability commands on the native binding, their
// CLI flags in camel case: repeated flags as lists (the import's `--run`
// as `runs`), a single `--bundles` as one prefix, milliseconds and
// `--repeat` as numbers, and the scope flags as the scope object.
function repeatArgs(argv) {
  const a = flags(argv);
  const lists = { bundles: 'bundles', descriptor: 'descriptors', rules: 'rules', calibration: 'calibrations' };
  // `--run` repeats only on the import; elsewhere it names one prefix.
  if (a.command === 'eval repeat import') lists.run = 'runs';
  const numbers = new Set(['now-ms', 'host-cap-ms', 'repeat']);
  const scoped = new Set(['scope', 'tenant', 'context-revision', 'principal-scope']);
  const camel = (k) => k.replace(/-([a-z])/g, (_, ch) => ch.toUpperCase());
  const args = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--')) continue;
    const k = argv[i].slice(2);
    if (scoped.has(k)) args.scope = scope(a);
    else if (k === 'bundles') args.bundles = a.many(k).length === 1 ? a.one(k) : a.many(k);
    else if (Object.hasOwn(lists, k)) args[lists[k]] = a.many(k);
    else if (numbers.has(k)) args[camel(k)] = Number(a.one(k));
    else args[camel(k)] = a.one(k);
  }
  return args;
}
