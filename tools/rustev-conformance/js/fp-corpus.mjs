// Spec 027, 3.11.3 and 3.11.5: the differential floating-point corpus on
// WebAssembly, in Node and in headless Chromium. The module is the
// `rustev-fp-corpus` crate built for wasm32-unknown-unknown; it has no
// imports. Each site's input and output digests must equal the committed
// ones (`fp-corpus/expected.json`), which the native runner checks on every
// native target; any difference fails.
//
//   node fp-corpus.mjs node|chromium <rustev_fp_corpus.wasm> <result.json>

import { copyFileSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [where, wasm, out] = process.argv.slice(2);
if (!['node', 'chromium'].includes(where) || !wasm || !out) {
  console.error('usage: node fp-corpus.mjs node|chromium <rustev_fp_corpus.wasm> <result.json>');
  process.exit(2);
}
const expected = JSON.parse(readFileSync(join(here, '../fp-corpus/expected.json'), 'utf8'));

/** Every site's digests from an instantiated module's exports. Plain, so
 * it runs in a page by its text as well. */
function runCorpus(e) {
  const hex = (which) => {
    let s = '';
    for (let i = 0; i < 32; i++) {
      const b = e.corpus_digest_byte(which, i);
      // A wasm `i32` result reaches JavaScript signed, so the module's
      // out-of-range sentinel `u32::MAX` arrives as -1: only 0 to 255 is a byte.
      if (!Number.isInteger(b) || b < 0 || b > 255) {
        throw new Error(`digest byte ${which}/${i} out of range (${b})`);
      }
      s += b.toString(16).padStart(2, '0');
    }
    return s;
  };
  const sites = [];
  for (let n = 0; n < e.corpus_sites(); n++) {
    const inputs = e.corpus_run(n);
    sites.push({ inputs, inputs_sha256: hex(0), outputs_sha256: hex(1) });
  }
  return sites;
}

let sites;
let engine;
if (where === 'node') {
  const { instance } = await WebAssembly.instantiate(readFileSync(wasm), {});
  sites = runCorpus(instance.exports);
  engine = `node ${process.version}`;
} else {
  const { serve, open } = await import('./chromium.mjs');
  // Served from a directory holding the module alone.
  const dir = mkdtempSync(join(tmpdir(), 'rustev-fp-corpus-'));
  copyFileSync(wasm, join(dir, 'corpus.wasm'));
  const server = await serve(dir);
  const { browser, page } = await open(server.origin);
  try {
    engine = `chromium ${browser.version()}`;
    await page.addScriptTag({ content: `globalThis.runCorpus = ${runCorpus.toString()};` });
    sites = await page.evaluate(async () => {
      const bytes = await (await fetch('/corpus.wasm')).arrayBuffer();
      const { instance } = await WebAssembly.instantiate(bytes, {});
      return globalThis.runCorpus(instance.exports);
    });
  } finally {
    await browser.close();
    server.close();
  }
}

const failures = [];
if (sites.length !== expected.sites.length) failures.push(`${sites.length} site(s), expected ${expected.sites.length}`);
expected.sites.forEach((want, n) => {
  const got = sites[n];
  if (!got) return;
  got.id = want.id;
  if (got.inputs !== expected.inputs_per_site) failures.push(`${want.id}: ${got.inputs} inputs`);
  for (const k of ['inputs_sha256', 'outputs_sha256']) {
    if (got[k] !== want[k]) failures.push(`${want.id}: ${k} ${got[k]}, expected ${want[k]}`);
  }
});
writeFileSync(out, JSON.stringify({ schema: 'rustev.fp-corpus-result/1', engine, sites, failures }, null, 2) + '\n');
console.log(`fp-corpus ${where}: ${sites.length} site(s), ${failures.length} difference(s)`);
for (const f of failures) console.error(f);
process.exit(failures.length ? 1 : 0);
