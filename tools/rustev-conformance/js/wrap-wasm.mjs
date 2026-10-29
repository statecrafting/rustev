// Stage the public entry points after wasm-bindgen has emitted all targets.
import { appendFileSync, copyFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { wrapBindings } from '../../../integrations/rustev-wasm/wrapper.mjs';

export function stageWrappers(pkg) {
  for (const target of ['web', 'bundler', 'node']) {
    const source = target === 'node'
      ? `const glue = require('./rustev_wasm.js');\nmodule.exports = { ...glue, ...(${wrapBindings.toString()})(glue) };\n`
      : `import * as glue from './rustev_wasm.js';\nexport * from './rustev_wasm.js';\n${target === 'web' ? "export { default } from './rustev_wasm.js';\n" : ''}export const { Engine, RustevError, InvalidInput } = (${wrapBindings.toString()})(glue);\n`;
    writeFileSync(resolve(pkg, target, 'index.js'), source);
    copyFileSync(new URL('../../../integrations/rustev-wasm/index.d.ts', import.meta.url), resolve(pkg, target, 'index.d.ts'));
    if (target === 'web') {
      // wasm-bindgen's web target also exposes asynchronous initialization.
      appendFileSync(resolve(pkg, target, 'index.d.ts'), "export { default } from './rustev_wasm.js';\n");
    }
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2]) throw new Error('usage: node wrap-wasm.mjs <package dir>');
  stageWrappers(process.argv[2]);
}
