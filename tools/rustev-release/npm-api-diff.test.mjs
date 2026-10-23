// Spec 031 3.8: declaration compatibility, registry helpers and version policy.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { diffPackages, selectBaseline, verifyIntegrity, versionAllows, unpackTarball } from './npm-api-diff.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const cli = path.join(root, 'tools/rustev-release/npm-api-diff.mjs');
function fixture(t, oldText, newText, extra = {}) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'rustev-api-test-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  return [oldText, newText].map((text, i) => {
    const dir = path.join(temp, String(i));
    fs.mkdirSync(dir);
    fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'fixture', version: '0.1.0', types: 'index.d.ts' }));
    fs.writeFileSync(path.join(dir, 'index.d.ts'), text);
    for (const [name, content] of Object.entries(extra)) fs.writeFileSync(path.join(dir, name), content);
    return dir;
  });
}
const cases = [
  ['identical', 'export interface A { value: string; optional?: number }', 'export interface A { value: string; optional?: number }', null],
  ['export removed', 'export const a: string; export const b: number;', 'export const a: string;', 'breaking'],
  ['interface member removed', 'export interface A { x: string; y?: number }', 'export interface A { x: string }', 'breaking'],
  ['optional made required', 'export interface A { x?: string }', 'export interface A { x: string }', 'breaking'],
  ['parameter narrowed', 'export function f(x: string | number): void;', 'export function f(x: string): void;', 'breaking'],
  ['return widened', 'export function f(): string;', 'export function f(): string | number;', 'breaking'],
  ['required parameter added', 'export function f(): void;', 'export function f(x: string): void;', 'breaking'],
  ['export added', 'export const a: string;', 'export const a: string; export const b: number;', 'compatible'],
  ['optional member added', 'export interface A { x: string }', 'export interface A { x: string; y?: number }', 'compatible'],
  ['parameter widened', 'export function f(x: string): void;', 'export function f(x: string | number): void;', 'compatible'],
  ['optional parameter added', 'export function f(): void;', 'export function f(x?: string): void;', 'compatible'],
  ['return narrowed', 'export function f(): string | number;', 'export function f(): string;', 'compatible'],
  ['readonly widened', 'export interface A { readonly x: string }', 'export interface A { readonly x: string | number }', 'breaking'],
  ['writable widened', 'export interface A { x: string }', 'export interface A { x: string | number }', 'breaking'],
  ['writable narrowed', 'export interface A { x: string | number }', 'export interface A { x: string }', 'breaking'],
  ['readonly narrowed', 'export interface A { readonly x: string | number }', 'export interface A { readonly x: string }', 'compatible'],
  ['writable index widened', 'export interface A { [k: string]: string }', 'export interface A { [k: string]: string | number }', 'breaking'],
  ['readonly index narrowed', 'export interface A { readonly [k: string]: string | number }', 'export interface A { readonly [k: string]: string }', 'compatible'],
  ['let widened', 'export let x: string;', 'export let x: string | number;', 'breaking'],
  ['let narrowed', 'export let x: string | number;', 'export let x: string;', 'breaking'],
  ['const narrowed', 'export const x: string | number;', 'export const x: string;', 'compatible'],
  ['const widened', 'export const x: string;', 'export const x: string | number;', 'breaking'],
  ['namespace member removed', 'export namespace N { const a: string; const b: number; }', 'export namespace N { const a: string; }', 'breaking'],
  ['class member removed', 'export class A { x: string; y: number; }', 'export class A { x: string; }', 'breaking'],
  ['enum member removed', 'export enum E { A, B }', 'export enum E { A }', 'breaking'],
  ['enum value changed', 'export enum E { A = 1 }', 'export enum E { A = 2 }', 'breaking'],
  ['alias member removed', 'export type A = { x: string; y: number };', 'export type A = { x: string };', 'breaking'],
  ['overload removed', 'export function f(x: string): string; export function f(x: number): number;', 'export function f(x: string): string;', 'breaking'],
  ['generic uncertain', 'export function f<T>(x: T): T;', 'export function f<T>(x: T): T | undefined;', 'breaking'],
];
for (const [name, a, b, classification] of cases) test(`031 3.8: ${name}`, t => {
  const report = diffPackages(...fixture(t, a, b));
  assert.equal(report.schema, 'rustev.npm-api-diff/1');
  if (classification === null) assert.deepEqual(report.findings, []);
  else {
    assert.ok(report.findings.some(f => f.class === classification), JSON.stringify(report));
    if (classification === 'compatible') assert.ok(report.findings.every(f => f.class === 'compatible'), JSON.stringify(report));
  }
});
test('031 3.8: follows re-exports and detects changes in the second file', t => {
  const dirs = fixture(t, 'export { A } from "./second";', 'export { A } from "./second";', { 'second.d.ts': 'export interface A { x: string; y: number }' });
  assert.deepEqual(diffPackages(...dirs).findings, []);
  fs.writeFileSync(path.join(dirs[1], 'second.d.ts'), 'export interface A { x: string }');
  assert.ok(diffPackages(...dirs).findings.some(f => f.export === '..A.y' && f.class === 'breaking'));
});
test('031 3.8: deterministic CLI JSON and exit codes', t => {
  const dirs = fixture(t, 'export const a: string;', 'export {};');
  const run = () => spawnSync(process.execPath, [cli, ...dirs], { encoding: 'utf8' });
  const a = run(), b = run();
  assert.equal(a.status, 2, a.stderr);
  assert.equal(a.stdout, b.stdout);
  assert.equal(b.status, 2);
  assert.equal(JSON.parse(a.stdout).findings[0].class, 'breaking');
  fs.unlinkSync(path.join(dirs[0], 'index.d.ts'));
  const missing = run();
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /031 3.8: missing/);
});
test('031 3.8: breaking version position', () => {
  const findings = [{ class: 'breaking' }];
  for (const [a, b, expected] of [['0.1.0', '0.1.1', false], ['0.1.0', '0.2.0', true], ['1.2.0', '1.3.0', false], ['1.2.0', '2.0.0', true], ['0.9.0', '1.0.0', true]])
    assert.equal(versionAllows(a, b, findings), expected);
  assert.equal(versionAllows('0.1.0', '0.1.0', [{ class: 'compatible' }]), true);
});
test('031 3.8: baseline selection excludes current, future, deprecated and prerelease', () => {
  const versions = Object.fromEntries(['0.1.0', '0.2.0', '0.3.0', '0.4.0', '1.0.0', '0.3.1-beta.1'].map(version => [version, { version }]));
  versions['0.3.0'].deprecated = 'withdrawn';
  assert.equal(selectBaseline({ versions }, '0.4.0').version, '0.2.0');
  assert.equal(selectBaseline({ versions }, '0.1.0'), null);
  assert.throws(() => selectBaseline({}, '0.1.0'), /031 3.8/);
});
test('031 3.8: sha512 integrity is mandatory and checked', () => {
  const bytes = Buffer.from('tarball');
  const integrity = `sha512-${createHash('sha512').update(bytes).digest('base64')}`;
  verifyIntegrity(bytes, integrity);
  assert.throws(() => verifyIntegrity(Buffer.from('tampered'), integrity), /031 3.8/);
  assert.throws(() => verifyIntegrity(bytes, 'sha1-ignored'), /031 3.8/);
});
for (const name of ['rustev-node', 'rustev-wasm']) test(`031 3.8: real ${name} self-comparison`, t => {
  const dir = path.join(root, 'integrations', name);
  if (name === 'rustev-wasm') {
    const missing = ['web', 'bundler', 'node'].filter(target => !fs.existsSync(path.join(dir, target, 'rustev_wasm.d.ts')));
    if (missing.length) {
      assert.throws(() => diffPackages(dir, dir), /031 3.8: missing/);
      t.skip(`031 3.8: generated declarations absent: ${missing.join(', ')}; generating them is outside task territory`);
      return;
    }
  }
  assert.deepEqual(diffPackages(dir, dir).findings, []);
});

function archive(files) {
  const blocks = [];
  for (const [name, text, type = '0'] of files) {
    const bytes = Buffer.from(text), header = Buffer.alloc(512);
    header.write(name);
    header.write('0000644\0', 100);
    header.write('0000000\0', 108);
    header.write('0000000\0', 116);
    header.write(bytes.length.toString(8).padStart(11, '0') + '\0', 124);
    header.write('00000000000\0', 136);
    header.fill(32, 148, 156);
    header.write(type, 156);
    header.write('ustar\0', 257);
    header.write('00', 263);
    const sum = header.reduce((a, b) => a + b, 0);
    header.write(sum.toString(8).padStart(6, '0') + '\0 ', 148);
    blocks.push(header, bytes, Buffer.alloc((512 - bytes.length % 512) % 512));
  }
  return gzipSync(Buffer.concat([...blocks, Buffer.alloc(1024)]));
}
test('031 3.8: verified archive extraction and traversal/link rejection', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'rustev-tar-test-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const pkg = { name: 'fixture', version: '0.1.0', types: 'index.d.ts' };
  const baseline = bytes => ({ ...pkg, dist: { integrity: `sha512-${createHash('sha512').update(bytes).digest('base64')}` } });
  const bytes = archive([['package/package.json', JSON.stringify(pkg)], ['package/index.d.ts', 'export const x: string;']]);
  unpackTarball(bytes, baseline(bytes), path.join(temp, 'good'));
  assert.equal(fs.readFileSync(path.join(temp, 'good/package/index.d.ts'), 'utf8'), 'export const x: string;');
  for (const [name, type] of [['package/../../escape', '0'], ['package/link', '2'], ['package/link', '1']]) {
    const bad = archive([[name, '', type]]);
    assert.throws(() => unpackTarball(bad, baseline(bad), path.join(temp, 'bad')), /031 3.8/);
    assert.equal(fs.existsSync(path.join(temp, 'bad')), false);
  }
  assert.throws(() => unpackTarball(bytes, { ...baseline(bytes), version: '0.2.0' }, path.join(temp, 'identity')), /identity differs/);
  assert.equal(fs.existsSync(path.join(temp, 'identity')), false);
  const conflict = archive([
    ['package/package.json', JSON.stringify(pkg)],
    ['package/file', 'contents'],
    ['package/file/child', 'contents'],
  ]);
  assert.throws(() => unpackTarball(conflict, baseline(conflict), path.join(temp, 'conflict')));
  assert.equal(fs.existsSync(path.join(temp, 'conflict')), false);
});
test('031 3.8: declaration exports conditions and typings', t => {
  const dirs = fixture(t, 'export const x: string;', 'export const x: string;', { 'second.d.ts': 'export const y: number;' });
  for (const dir of dirs) fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'fixture', version: '0.1.0', typings: 'index.d.ts', exports: { './extra': { import: { types: './second.d.ts' } } } }));
  assert.deepEqual(diffPackages(...dirs).findings, []);
  fs.writeFileSync(path.join(dirs[1], 'second.d.ts'), 'export {};');
  assert.ok(diffPackages(...dirs).findings.some(f => f.export === './extra#import.y' && f.class === 'breaking'));
});
test('031 3.8: version-rule CLI exit codes', t => {
  const dirs = fixture(t, 'export {};', 'export {};');
  const file = path.join(dirs[0], 'report.json');
  for (const [oldVersion, newVersion, status] of [['0.1.0', '0.1.1', 2], ['0.1.0', '0.2.0', 0], ['1.2.0', '1.3.0', 2], ['1.2.0', '2.0.0', 0]]) {
    fs.writeFileSync(file, JSON.stringify({ oldVersion, newVersion, findings: [{ class: 'breaking' }] }));
    assert.equal(spawnSync(process.execPath, [cli, '--version-rule', file]).status, status);
  }
});
test('031 3.8: missing and wrong TypeScript pin are named', t => {
  const dirs = fixture(t, 'export {};', 'export {};');
  const copied = path.join(dirs[0], 'npm-api-diff.mjs');
  fs.copyFileSync(cli, copied);
  let result = spawnSync(process.execPath, [copied, '--check-typescript'], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing TypeScript 5.9.3/);
  const module = path.join(dirs[0], '../rustev-conformance/js/node_modules/typescript');
  fs.mkdirSync(module, { recursive: true });
  fs.writeFileSync(path.join(module, 'index.js'), "module.exports = { version: '5.8.0' };\n");
  result = spawnSync(process.execPath, [copied, '--check-typescript'], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /need TypeScript 5.9.3, found 5.8.0/);
});

function paxRecord(key, value) {
  const body = ` ${key}=${value}\n`;
  let length = Buffer.byteLength(body) + 1;
  while (String(length).length + Buffer.byteLength(body) !== length)
    length = String(length).length + Buffer.byteLength(body);
  return `${length}${body}`;
}

test('031 3.8: PAX long UTF-8 path applies to exactly the next entry', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'rustev-pax-test-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const pkg = { name: 'fixture', version: '0.1.0', types: 'index.d.ts' };
  const longPath = `package/types/${'declarations-'.repeat(12)}é.d.ts`;
  const bytes = archive([
    ['package/package.json', JSON.stringify(pkg)],
    ['PaxHeader', paxRecord('mtime', '123') + paxRecord('path', longPath), 'x'],
    ['package/placeholder.d.ts', 'export const long: string;'],
    ['package/index.d.ts', 'export const next: number;'],
  ]);
  const baseline = { ...pkg, dist: { integrity: `sha512-${createHash('sha512').update(bytes).digest('base64')}` } };
  unpackTarball(bytes, baseline, path.join(temp, 'out'));
  assert.equal(fs.readFileSync(path.join(temp, 'out', longPath), 'utf8'), 'export const long: string;');
  assert.equal(fs.readFileSync(path.join(temp, 'out/package/index.d.ts'), 'utf8'), 'export const next: number;');
  assert.equal(fs.existsSync(path.join(temp, 'out/package/placeholder.d.ts')), false);
});

test('031 3.8: global PAX headers are skipped without consuming local metadata', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'rustev-pax-global-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const pkg = { name: 'fixture', version: '0.1.0', types: 'index.d.ts' };
  const bytes = archive([
    ['GlobalHeader', paxRecord('path', '/ignored-global-path'), 'g'],
    ['package/package.json', JSON.stringify(pkg)],
    ['PaxHeader', paxRecord('path', 'package/index.d.ts'), 'x'],
    ['GlobalHeader', paxRecord('mtime', '123'), 'g'],
    ['package/placeholder.d.ts', 'export const value: string;'],
  ]);
  unpackTarball(bytes, { ...pkg, dist: { integrity: `sha512-${createHash('sha512').update(bytes).digest('base64')}` } }, path.join(temp, 'out'));
  assert.equal(fs.readFileSync(path.join(temp, 'out/package/index.d.ts'), 'utf8'), 'export const value: string;');
});

test('031 3.8: PAX cannot bypass path, link, duplicate or size checks', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'rustev-pax-unsafe-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const pkg = { name: 'fixture', version: '0.1.0' };
  const cases = [
    [paxRecord('path', '/absolute'), '0'],
    [paxRecord('path', 'package/../escape'), '0'],
    [paxRecord('path', 'package/dir/../../escape'), '0'],
    [paxRecord('path', 'package/\\escape'), '0'],
    [paxRecord('path', 'package/nu\0ll'), '0'],
    [paxRecord('path', 'package/package.json'), '0'],
    [paxRecord('linkpath', '../../escape'), '1'],
    [paxRecord('linkpath', '/absolute'), '2'],
    [paxRecord('size', '999999999999999999999'), '0'],
    [paxRecord('size', '100000'), '0'],
    [paxRecord('size', '-1'), '0'],
    ['999 path=package/index.d.ts\n', '0'],
    ['0 path=package/index.d.ts\n', '0'],
    ['3 x', '0'],
  ];
  for (const [i, [metadata, type]] of cases.entries()) {
    const bytes = archive([
      ['package/package.json', JSON.stringify(pkg)],
      ['PaxHeader', metadata, 'x'],
      ['package/placeholder', '', type],
    ]);
    const destination = path.join(temp, String(i));
    assert.throws(() => unpackTarball(bytes, { ...pkg, dist: { integrity: `sha512-${createHash('sha512').update(bytes).digest('base64')}` } }, destination), /031 3.8/);
    assert.equal(fs.existsSync(destination), false);
  }
});
