// Spec 027, 3.10.3 to 3.10.6: the WebAssembly module's import allowlist,
// its size budget and its instantiation budget, measured and written to a
// measurement document; a gated value over its budget fails the run. A
// budget changes only by amending spec 027, never here.
//
//   node wasm-check.mjs <package dir> <measurement.json> [--chromium-ms <median>]
//       [--chromium-record <chromium.mjs time output>]
//
// <package dir> holds the `web` target (`web/rustev_wasm.js` and its
// optimized `web/rustev_wasm_bg.wasm`).

import { execFileSync, spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

const here = dirname(fileURLToPath(import.meta.url));
const [pkg, outPath, ...rest] = process.argv.slice(2);
// `--chromium-ms <median>`: the headless Chromium median the CI job
// measured (3.10.6), checked against its budget; absent locally.
const chromiumAt = rest.indexOf('--chromium-ms');
const chromiumMs = chromiumAt >= 0 ? Number(rest[chromiumAt + 1]) : null;
if (chromiumAt >= 0 && !(chromiumMs >= 0)) {
  console.error('usage: --chromium-ms takes a non-negative median in milliseconds');
  process.exit(2);
}
// `--chromium-record <file>`: what chromium.mjs measured (browser version,
// samples, median, p95), recorded in the measurement document (3.10.5,
// 3.10.6); its median must be the one gated.
const recordAt = rest.indexOf('--chromium-record');
let chromiumRecord = null;
if (recordAt >= 0) {
  try {
    chromiumRecord = JSON.parse(readFileSync(rest[recordAt + 1], 'utf8'));
  } catch (e) {
    console.error(`--chromium-record: ${e.message}`);
    process.exit(2);
  }
  if (chromiumMs === null || chromiumRecord.median !== chromiumMs) {
    console.error('--chromium-record: its median must equal --chromium-ms');
    process.exit(2);
  }
}
if (!pkg || !outPath) {
  console.error('usage: node wasm-check.mjs <package dir> <measurement.json>');
  process.exit(2);
}
const wasmPath = resolve(pkg, 'web/rustev_wasm_bg.wasm');
const gluePath = resolve(pkg, 'web/rustev_wasm.js');
const bytes = readFileSync(wasmPath);

const BUDGET = {
  gzip_bytes: 786_432,
  // Spec 046 (A-43) raised it from 2,097,152 bytes, amending 027 3.10.4.
  raw_bytes: 2_621_440,
  node_median_ms: 50,
  chromium_median_ms: 100,
};
const failures = [];

// 3.10.3: only wasm-bindgen's glue, each import an exact, reviewed
// `module:name` pair (the name without wasm-bindgen's prefix and hash).
const allowed = JSON.parse(readFileSync(join(here, 'wasm-imports.json'), 'utf8'));
const imports = WebAssembly.Module.imports(new WebAssembly.Module(bytes));
const seen = [];
for (const i of imports) {
  // wasm-bindgen 0.2.129, pinned exactly (3.10.2), suffixes each import
  // with a 16-hex-digit hash; a new pin that changes it fails here loudly.
  const base = i.name.replace(/^__wbg_/, '').replace(/_[0-9a-f]{16}$/, '');
  seen.push(`${i.module}:${base}`);
  if (!allowed.imports.includes(`${i.module}:${base}`)) {
    failures.push(`027 3.10.3: import ${i.module}.${i.name} is not on the allowlist`);
  }
}

// 3.10.4: sizes with the compressors this Node version pins.
const sizes = {
  raw_bytes: bytes.length,
  gzip_bytes: gzipSync(bytes, { level: 9 }).length,
  brotli_bytes: brotliCompressSync(bytes, {
    params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
  }).length,
};
for (const k of ['raw_bytes', 'gzip_bytes']) {
  if (sizes[k] > BUDGET[k]) {
    failures.push(`027 3.10.4: ${k} ${sizes[k]} exceeds its budget of ${BUDGET[k]}`);
  }
}

// 3.10.5: cold instantiation in a fresh process each time, from module
// bytes in memory to the first successful `version()`; 3 discarded warm-up
// processes, then 21 measured.
const child = `
  import { readFileSync } from 'node:fs';
  import { pathToFileURL } from 'node:url';
  const glue = await import(pathToFileURL(${JSON.stringify(gluePath)}).href);
  const bytes = readFileSync(${JSON.stringify(wasmPath)});
  const t0 = process.hrtime.bigint();
  glue.initSync({ module: bytes });
  const v = glue.version();
  const t1 = process.hrtime.bigint();
  if (!v) process.exit(1);
  process.stdout.write(String(Number(t1 - t0) / 1e6));
`;
function sample() {
  const r = spawnSync(process.execPath, ['--input-type=module', '-e', child], {
    encoding: 'utf8',
  });
  if (r.status !== 0) {
    throw new Error(`instantiation failed: ${r.stderr}`);
  }
  return Number(r.stdout);
}
// A failed instantiation is recorded as a failure; the measurement document
// is still written with everything else measured.
let samples = [];
let median = null;
let p95 = null;
try {
  for (let i = 0; i < 3; i++) sample();
  samples = Array.from({ length: 21 }, sample);
  const sorted = [...samples].sort((a, b) => a - b);
  median = sorted[10];
  p95 = sorted[Math.ceil(0.95 * sorted.length) - 1];
  if (median > BUDGET.node_median_ms) {
    failures.push(`027 3.10.5: Node median ${median} ms exceeds ${BUDGET.node_median_ms} ms`);
  }
} catch (e) {
  failures.push(`027 3.10.5: ${e.message}`);
}
if (chromiumMs !== null && chromiumMs > BUDGET.chromium_median_ms) {
  failures.push(`027 3.10.6: Chromium median ${chromiumMs} ms exceeds ${BUDGET.chromium_median_ms} ms`);
}

let commit = null;
try {
  commit = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
} catch {
  // Outside a repository the commit is unknown, and recorded so.
}
const measurement = {
  schema: 'rustev.wasm-measurement/1',
  commit,
  node: process.version,
  platform: `${process.platform}-${process.arch}`,
  runner: process.env.ImageOS ?? null,
  budgets: BUDGET,
  sizes,
  imports: seen.sort(),
  node_instantiation_ms: { samples, median, p95 },
  // Headless Chromium is measured by the CI job (3.10.6), null when not given.
  chromium_instantiation_ms:
    chromiumMs === null
      ? null
      : chromiumRecord
        ? { samples: chromiumRecord.samples, median: chromiumMs, p95: chromiumRecord.p95 }
        : { median: chromiumMs },
  chromium: chromiumRecord?.chromium ?? null,
  failures,
};
writeFileSync(outPath, JSON.stringify(measurement, null, 2) + '\n');
console.log(JSON.stringify({ sizes, node_median_ms: median, failures }, null, 2));
if (failures.length) process.exit(1);
