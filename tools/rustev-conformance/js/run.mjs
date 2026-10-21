// Spec 027, 3.12: the JavaScript conformance runner. One harness, one
// adapter per surface: `wasm` (the WebAssembly package) and `native` (the
// Node-API package). Every case of the shared manifest that names the
// surface runs from its pinned inputs and must produce each expected
// output's bytes under only the comparison the case declares.
//
//   node run.mjs wasm <wasm package dir>
//   node run.mjs chromium <wasm package dir>   (the same cases in headless Chromium)
//   node run.mjs native <native package dir>
//
// What a surface does not print is CLI presentation (3.1.3, 3.12.3): exit
// codes and the `run` envelope's paths and receipt. A native `run` case is
// therefore compared on the judgment and run-record bytes the envelope
// carries; a pure command's output is compared whole, exit code included.

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const [surface, pkg] = process.argv.slice(2);
if (!['wasm', 'chromium', 'native'].includes(surface) || !pkg) {
  console.error('usage: node run.mjs wasm|chromium|native <package dir>');
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
  const fromB64 = (s) => (s === undefined ? undefined : Buffer.from(s, 'base64'));
  comparePure(c, r.code, fromB64(r.out), (name) => fromB64(r.artifacts[name]));
}

async function wasmAdapter() {
  const glue = await import(pathToFileURL(resolve(pkg, 'web/rustev_wasm.js')).href);
  glue.initSync({ module: readFileSync(resolve(pkg, 'web/rustev_wasm_bg.wasm')) });
  return async (c) => compareWasm(c, executeWasm(glue, wasmCall(c)));
}

/** The same calls in headless Chromium (3.11.3, 3.13.3): one page holds the
 * `web` target, served from loopback, and runs each case's call there. */
async function chromiumAdapter() {
  const { serve, open } = await import('./chromium.mjs');
  const server = await serve(pkg);
  const { browser, page } = await open(server.origin);
  await page.evaluate(async () => {
    const glue = await import('/web/rustev_wasm.js');
    glue.initSync({ module: new Uint8Array(await (await fetch('/web/rustev_wasm_bg.wasm')).arrayBuffer()) });
    globalThis.rustevGlue = glue;
  });
  await page.addScriptTag({ content: `globalThis.executeWasm = ${executeWasm.toString()};` });
  const adapter = async (c) => compareWasm(c, await page.evaluate((call) => globalThis.executeWasm(globalThis.rustevGlue, call), wasmCall(c)));
  adapter.close = async () => {
    await browser.close();
    server.close();
  };
  return adapter;
}

// ---- native ---------------------------------------------------------------

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
    if (a.command === 'replay') {
      // An object: the native command takes its arguments as JSON and reads
      // `scope` from it (the WebAssembly binding takes a string instead).
      args.scope = scope(a);
      args.nowMs = Number(a.need('now-ms'));
      if (a.one('host-cap-ms') !== undefined) args.hostCapMs = Number(a.one('host-cap-ms'));
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
    const gates = rt.backends.map(() => new Map());
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
        infer: (call) => scripted(b, gates[bi], call, ignored),
      })),
    });
    const plan = await engine.compile({
      plan: files.get(rt.plan),
      descriptors: rt.descriptors.map((d) => files.get(d)),
      calibrations: rt.calibrations.map((x) => files.get(x)),
    });
    const controllers = rt.requests.map(() => new AbortController());
    const running = [];
    let next = 0;
    const until = async (what, cond) => {
      for (let i = 0; i < 100_000; i++) {
        if (cond()) return;
        await new Promise((r) => setImmediate(r));
      }
      throw new Error(`${c.id}: the driver waited forever for ${what}`);
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
                { signal: controllers[i].signal, deadlineMs: r.deadline_ms },
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
    done.forEach((d, i) => {
      const id = rt.requests[i].decision_id;
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
    });
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

  return async (c) => {
    const { files, derived } = inputsOf(c);
    if (c.runtime) return runtimeCase(c, files);
    const a = flags(c.argv);
    if (a.command === 'run') return runCase(c, a, files, derived);
    return pureCase(c, a, files);
  };
}

const manifest = JSON.parse(readFileSync(join(root, 'tools/rustev-conformance/cases/manifest.json'), 'utf8'));
if (manifest.schema !== 'rustev.conformance-manifest/1') throw new Error('manifest schema');
const run =
  surface === 'wasm' ? await wasmAdapter() : surface === 'chromium' ? await chromiumAdapter() : await nativeAdapter();
// The Chromium adapter runs the cases that name the WebAssembly surface.
const named = surface === 'chromium' ? 'wasm' : surface;
let ran = 0;
for (const c of manifest.cases.filter((c) => c.surfaces.includes(named))) {
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
