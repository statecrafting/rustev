// Spec 027, 3.12: the JavaScript conformance runner. One harness, one
// adapter per surface; this file holds the WebAssembly adapter (the native
// adapter joins with `rustev-node`). Every case of the shared manifest that
// names the surface runs from its pinned inputs and must produce each
// expected output's bytes under only the comparison the case declares.
//
//   node run.mjs wasm <package dir>

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const [surface, pkg] = process.argv.slice(2);
if (surface !== 'wasm' || !pkg) {
  console.error('usage: node run.mjs wasm <package dir>');
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

// The trusted scope the CLI's flags give (spec 006, 3.1.3), as its JSON.
function scope(a) {
  // Each mode's fields are required: a case without them fails by name.
  const mode = a.one('scope') ?? 'principal';
  return JSON.stringify(
    mode === 'tenant-only'
      ? { mode: 'tenant_only', tenant: a.need('tenant'), context_revision: a.need('context-revision') }
      : {
          mode: 'principal',
          tenant: a.need('tenant'),
          context_revision: a.need('context-revision'),
          principal_scope: a.need('principal-scope'),
        },
  );
}

const glue = await import(pathToFileURL(resolve(pkg, 'web/rustev_wasm.js')).href);
glue.initSync({ module: readFileSync(resolve(pkg, 'web/rustev_wasm_bg.wasm')) });

/** Run one case on the WebAssembly surface: its exit code and printed bytes. */
function runWasm(c) {
  const inputs = new glue.Inputs();
  const files = new Map();
  for (const i of c.inputs) files.set(i.name, pinned(i.path, i.sha256));
  // A rules program's derived descriptor stands in for it (3.10.1).
  const derived = new Map((c.derived ?? []).map((d) => [d.from, pinned(d.path, d.sha256)]));
  for (const [name, bytes] of files) inputs.add(name, derived.get(name) ?? bytes);
  const a = flags(c.argv);
  const descriptors = [...a.many('descriptor'), ...a.many('rules')];
  const calibrations = a.many('calibration');
  const resolveItem = (reference) => {
    const store = a.one('store');
    const b = store !== undefined ? files.get(`${store}/${reference}`) : undefined;
    return b ?? null;
  };
  switch (a.command) {
    case 'plan check':
      return glue.check(inputs, a.need('definition'), descriptors, calibrations, a.one('execution'));
    case 'plan compile':
      return glue.compile(
        inputs,
        a.need('definition'),
        descriptors,
        calibrations,
        a.one('execution'),
        a.need('out'),
      );
    case 'plan show':
      return glue.show(inputs, a.need('plan'), descriptors, calibrations);
    case 'replay': {
      const cap = a.one('host-cap-ms');
      return glue.replay(
        inputs,
        a.need('bundle'),
        scope(a),
        Number(a.need('now-ms')),
        cap === undefined ? undefined : Number(cap),
        resolveItem,
      );
    }
    default:
      // `run` is no WebAssembly command (3.1.1), and the harvest never
      // names `wasm` on a run case; a case that did is a failure, reported
      // with the others rather than ending the runner.
      return null;
  }
}

const manifest = JSON.parse(
  readFileSync(join(root, 'tools/rustev-conformance/cases/manifest.json'), 'utf8'),
);
if (manifest.schema !== 'rustev.conformance-manifest/1') throw new Error('manifest schema');
const failures = [];
let ran = 0;
/** One case, run and compared; anything it throws fails that case only. */
function runCase(c) {
  const r = runWasm(c);
  if (r === null) {
    failures.push(`${c.id}: the WebAssembly surface has no command ${flags(c.argv).command}`);
    return;
  }
  if (c.exit !== undefined && c.exit !== null && r.code !== c.exit) {
    failures.push(`${c.id}: exit ${r.code}, expected ${c.exit}`);
  }
  for (const o of c.outputs) {
    // A printed output is the command's output object; a written one is the
    // artifact it published under that name (none of today's cases pins
    // one, but a case that did is compared, never skipped).
    const got = o.stream === 'stdout' ? r.out : r.artifact(o.file);
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

for (const c of manifest.cases.filter((c) => c.surfaces.includes(surface))) {
  ran += 1;
  try {
    runCase(c);
  } catch (e) {
    failures.push(`${c.id}: ${e.stack ?? e}`);
  }
}
console.log(`conformance ${surface}: ${ran} case(s), ${failures.length} failure(s)`);
for (const f of failures) console.error(f);
if (failures.length || ran === 0) process.exit(1);
