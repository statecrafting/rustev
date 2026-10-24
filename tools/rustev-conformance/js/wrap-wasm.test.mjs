// Exercise the packaged entry points without a local WebAssembly toolchain.
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import { stageWrappers } from './wrap-wasm.mjs';

test('all packaged targets preserve commands and expose typed explanation refusals', async () => {
  const pkg = mkdtempSync(join(tmpdir(), 'rustev-wasm-wrapper-'));
  try {
    const metadata = JSON.parse(readFileSync(new URL('../../../integrations/rustev-wasm/package.json', import.meta.url)));
    writeFileSync(join(pkg, 'package.json'), JSON.stringify(metadata));
    for (const target of ['web', 'bundler', 'node']) {
      mkdirSync(join(pkg, target));
      writeFileSync(join(pkg, target, 'package.json'), JSON.stringify({ type: target === 'node' ? 'commonjs' : 'module' }));
      const mock = `
        class Inputs {
          files = {};
          add(name, bytes) { this.files[name] = bytes; }
          free() { this.freed = true; }
        }
        class Engine {
          explain(inputs, args, resolve) {
            const { evidence, query, options } = JSON.parse(args);
            this.lastInputs = inputs;
            this.lastArgs = { evidence, query, options, resolve };
            this.lastResult = {
              code: query === 'ok' ? 0 : 2,
              out: new TextEncoder().encode(JSON.stringify({ status: 'invalid-options', detail: { subject: 'options' } })),
              artifact: (name) => name === 'explanation' ? inputs.files[evidence.record] : new TextEncoder().encode('markdown'),
              free() { this.freed = true; },
            };
            return this.lastResult;
          }
          free() { return 'freed'; }
        }
        const command = () => 'command';
        const initSync = () => 'initialized';
      `;
      writeFileSync(join(pkg, target, 'rustev_wasm.js'), mock + (target === 'node'
        ? 'module.exports = { Engine, Inputs, command, initSync };'
        : `export { Engine, Inputs, command, initSync }; ${target === 'web' ? 'export default initSync;' : ''}`));
      writeFileSync(join(pkg, target, 'rustev_wasm.d.ts'), `
        export class Engine { explain(evidence: unknown, query: unknown, options?: unknown): Promise<unknown>; free(): string; }
        export function command(): string;
        export function initSync(): string;
        ${target === 'web' ? 'export default initSync;' : ''}
      `);
    }
    stageWrappers(pkg);
    const declarations = [];
    for (const target of ['web', 'bundler', 'node']) {
      const entry = metadata.exports[`./${target}`];
      const api = target === 'node'
        ? createRequire(join(pkg, 'probe.cjs'))(`${metadata.name}/node`)
        : await import(pathToFileURL(join(pkg, entry.import)).href);
      declarations.push(join(pkg, entry.types));
      assert.equal(api.command(), 'command');
      assert.equal(api.initSync(), 'initialized');
      if (target === 'web') assert.equal(api.default(), 'initialized');
      const engine = new api.Engine();
      const evidence = new Uint8Array([1, 2]);
      const options = { scope: 'retained' };
      const success = await engine.explain({ record: evidence }, 'ok', options);
      assert.equal(success.explanation, evidence);
      assert.equal(success.markdown, 'markdown');
      assert.deepEqual(engine.lastArgs.options, options);
      assert.equal(engine.lastInputs.freed, true);
      assert.equal(engine.lastResult.freed, true);
      await assert.rejects(engine.explain({ record: evidence }, 'refusal'), (e) => {
        assert.ok(e instanceof api.InvalidInput);
        assert.ok(e instanceof api.RustevError);
        assert.ok(e instanceof Error);
        assert.equal(e.name, 'InvalidInput');
        assert.equal(e.code, 'invalid-options');
        assert.deepEqual(e.detail, { subject: 'options' });
        assert.equal(e.message, 'invalid-options: {"subject":"options"}');
        return true;
      });
      assert.equal(engine.lastInputs.freed, true);
      assert.equal(engine.lastResult.freed, true);
      for (const options of [null, [], { resolve: undefined }, { redaction: 'not bytes' }]) {
        await assert.rejects(engine.explain({ record: evidence }, 'ok', options), api.InvalidInput);
      }
      const resolve = () => evidence;
      await engine.explain({ record: evidence }, 'ok', { resolve, redaction: evidence, unknown: undefined });
      assert.equal(engine.lastArgs.resolve, resolve);
      assert.deepEqual(engine.lastArgs.options, { redaction: 'redaction', unknown: null });
      assert.equal(engine.lastInputs.files.redaction, evidence);
      assert.equal(engine.free(), 'freed');
    }
    execFileSync(process.execPath, [new URL('./node_modules/typescript/bin/tsc', import.meta.url).pathname,
      '--noEmit', '--strict', '--lib', 'es2022,dom', ...declarations], { stdio: 'inherit' });
  } finally {
    rmSync(pkg, { recursive: true, force: true });
  }
});
