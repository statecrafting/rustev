// SYNTHETIC package example, exported from the exact registry crate.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const native = require('@rustev/rustev');
const wasm = require('@rustev/rustev-wasm/node');
const [version, example] = process.argv.slice(2);
assert.equal(native.version(), version);
assert.equal(wasm.version(), version);
const files = Object.fromEntries(['definition.json', 'descriptor.json', 'calibration.json']
  .map(name => [name, fs.readFileSync(path.join(example, name))]));
(async () => {
  const engine = await native.Engine.create({
    runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 3, sinkTimeoutMs: 1000 },
    sink: { queueCapacity: 4, deliver: async () => 'SYNTHETIC-consumer' },
  });
  try {
    const result = engine.check(files, { definition: 'definition.json',
      descriptors: ['descriptor.json'], calibrations: ['calibration.json'] });
    assert.equal(result.code, 0);
    const inputs = new wasm.Inputs();
    for (const [name, bytes] of Object.entries(files)) inputs.add(name, bytes);
    const checked = wasm.check(inputs, 'definition.json', ['descriptor.json'], ['calibration.json'], undefined);
    assert.equal(checked.code, 0);
  } finally { await engine.shutdown(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
