// Spec 027, 3.10.5, 3.11.3 and 3.13.3: the WebAssembly package in headless
// Chromium, exact-pinned through the test harness (playwright-core, whose
// version fixes the Chromium build; installed by `npm ci` and
// `playwright-core install chromium-headless-shell`).
//
//   node chromium.mjs time <package dir> <out.json>
//       cold instantiation, from module bytes in memory to the first
//       successful `version()`: 3 discarded warm-up runs, then 21 measured,
//       each in a fresh browser process; writes the samples, median, p95
//       and the browser version, and prints the median
//
// `serve` and `open` are shared with run.mjs's Chromium adapter, which runs
// the WebAssembly conformance cases in a page.
//
// The page is served from a loopback HTTP server holding only the package
// directory; nothing is fetched from the network.

import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
import { extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const TYPES = { '.js': 'text/javascript', '.wasm': 'application/wasm', '.html': 'text/html' };

/** A loopback server for `dir` alone; resolves to its origin. */
export async function serve(dir) {
  const root = resolve(dir);
  const server = createServer((req, res) => {
    // A request line that is no URL, or a path that does not decode, is
    // answered 400 rather than thrown out of the handler.
    let path;
    try {
      path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    } catch {
      res.writeHead(400).end();
      return;
    }
    if (path === '/') {
      res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><title>rustev</title>');
      return;
    }
    const file = normalize(join(root, path));
    if (!file.startsWith(root + sep)) {
      res.writeHead(403).end();
      return;
    }
    try {
      const body = readFileSync(file);
      res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(body);
    } catch {
      res.writeHead(404).end();
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { origin: `http://127.0.0.1:${server.address().port}`, close: () => server.close() };
}

/** A fresh browser and a page on `origin`. */
export async function open(origin) {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  await page.goto(`${origin}/`);
  return { browser, page };
}

/** One cold instantiation, in milliseconds, and the browser's version. */
async function sample(origin) {
  const { browser, page } = await open(origin);
  try {
    const version = browser.version();
    const ms = await page.evaluate(async () => {
      const glue = await import('/web/rustev_wasm.js');
      const bytes = new Uint8Array(await (await fetch('/web/rustev_wasm_bg.wasm')).arrayBuffer());
      const t0 = performance.now();
      glue.initSync({ module: bytes });
      const v = glue.version();
      const t1 = performance.now();
      if (!v) throw new Error('version() returned nothing');
      return t1 - t0;
    });
    return { ms, version };
  } finally {
    await browser.close();
  }
}

async function time(pkg, out) {
  const { origin, close } = await serve(pkg);
  try {
    for (let i = 0; i < 3; i++) await sample(origin);
    const runs = [];
    for (let i = 0; i < 21; i++) runs.push(await sample(origin));
    const samples = runs.map((r) => r.ms);
    const sorted = [...samples].sort((a, b) => a - b);
    const version = runs[0].version;
    const doc = {
      chromium: version,
      samples,
      median: sorted[10],
      p95: sorted[Math.ceil(0.95 * sorted.length) - 1],
    };
    writeFileSync(out, JSON.stringify(doc, null, 2) + '\n');
    console.log(doc.median);
  } finally {
    close();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [cmd, pkg, out] = process.argv.slice(2);
  if (cmd !== 'time' || !pkg || !out) {
    console.error('usage: node chromium.mjs time <package dir> <out.json>');
    process.exit(2);
  }
  await time(pkg, out);
}
