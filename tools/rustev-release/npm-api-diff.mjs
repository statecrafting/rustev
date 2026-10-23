// Spec 031 3.8: pinned declaration comparison and npm baseline helpers.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';

const require = createRequire(import.meta.url);
const fail = message => { throw new Error(`031 3.8: ${message}`); };
export function typescript() {
  let ts;
  try { ts = require('../rustev-conformance/js/node_modules/typescript'); }
  catch { fail('missing TypeScript 5.9.3; run npm ci --prefix tools/rustev-conformance/js'); }
  if (ts.version !== '5.9.3') fail(`need TypeScript 5.9.3, found ${ts.version}`);
  return ts;
}
const json = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const order = (a, b) => a < b ? -1 : a > b ? 1 : 0;
function version(v) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(v)) fail(`invalid release version ${v}`);
  return v.split('.').map(BigInt);
}
function compareVersions(a, b) {
  const x = version(a), y = version(b);
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  return 0;
}
export function versionAllows(oldVersion, newVersion, findings) {
  const a = version(oldVersion), b = version(newVersion);
  return !findings.some(f => f.class === 'breaking') || b[0] > a[0] ||
    (a[0] === 0n && b[0] === 0n && b[1] > a[1]);
}
export function selectBaseline(packument, current) {
  version(current);
  if (!packument.versions || typeof packument.versions !== 'object') fail('invalid npm packument: missing versions');
  const candidates = Object.entries(packument.versions).filter(([v, data]) =>
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(v) &&
    !data.deprecated && compareVersions(v, current) < 0).sort(([a], [b]) => compareVersions(a, b));
  const selected = candidates.at(-1);
  if (selected && selected[1].version !== selected[0]) fail('npm baseline version does not match packument key');
  return selected?.[1] ?? null;
}
export function verifyIntegrity(bytes, integrity) {
  const hashes = String(integrity).split(/\s+/).filter(s => /^sha512-[A-Za-z0-9+/]{86}==$/.test(s));
  const actual = `sha512-${createHash('sha512').update(bytes).digest('base64')}`;
  if (!hashes.includes(actual)) fail('npm tarball sha512 dist.integrity verification failed');
}

// PAX records use byte lengths, including the length prefix and final newline.
// Unknown metadata is ignored; path and size affect the following real entry.
function parsePax(data) {
  const records = new Map();
  let offset = 0;
  while (offset < data.length) {
    const space = data.indexOf(32, offset);
    const lengthText = data.subarray(offset, space).toString('latin1');
    if (space < 0 || !/^[1-9][0-9]*$/.test(lengthText)) fail('invalid npm PAX record length');
    const length = Number(lengthText), end = offset + length;
    if (!Number.isSafeInteger(length) || end > data.length || end <= space + 1 || data[end - 1] !== 10)
      fail('truncated or invalid npm PAX record');
    const record = new TextDecoder('utf-8', { fatal: true }).decode(data.subarray(space + 1, end - 1));
    const equals = record.indexOf('=');
    if (equals <= 0 || record.includes('\0')) fail('invalid npm PAX key or value');
    records.set(record.slice(0, equals), record.slice(equals + 1));
    offset = end;
  }
  return records;
}

// Accept regular files and directories with per-entry PAX metadata. Global
// headers are skipped. Links (including PAX linkpath targets) remain refused,
// so no archive link can escape the fresh extraction root.
export function unpackTarball(bytes, baseline, destination) {
  verifyIntegrity(bytes, baseline.dist?.integrity);
  const tar = gunzipSync(bytes);
  const files = [];
  const names = new Set();
  let offset = 0;
  let pax = null;
  const field = (header, start, length) => header.subarray(start, start + length).toString('utf8').replace(/\0.*$/s, '');
  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every(b => b === 0)) break;
    const octal = (start, length) => {
      const value = field(header, start, length).trim();
      if (!/^[0-7]+$/.test(value)) fail('invalid npm tar header number');
      return parseInt(value, 8);
    };
    const sum = header.reduce((total, b, i) => total + (i >= 148 && i < 156 ? 32 : b), 0);
    if (sum !== octal(148, 8)) fail('invalid npm tar header checksum');
    let size = octal(124, 12);
    const type = header[156];
    const extended = type === 120 || type === 103;
    if (!extended && pax?.has('size') && pax.get('size') !== '') {
      if (!/^[0-9]+$/.test(pax.get('size'))) fail('invalid npm PAX size');
      size = Number(pax.get('size'));
    }
    if (!Number.isSafeInteger(size) || size > tar.length - offset - 512) fail('truncated npm tar member');
    const data = tar.subarray(offset + 512, offset + 512 + size);
    offset += 512 + Math.ceil(size / 512) * 512;
    if (extended) {
      // Metadata names are not extraction paths. Global metadata must not
      // override local paths or consume a pending per-entry extended header.
      if (type === 120) pax = new Map([...(pax ?? []), ...parsePax(data)]);
      continue;
    }
    const prefix = field(header, 345, 155);
    const name = (pax?.get('path') || [prefix, field(header, 0, 100)].filter(Boolean).join('/')).replace(/\/$/, '');
    pax = null;
    if (name !== 'package' && !name.startsWith('package/')) fail('npm tar member outside package/');
    if (name.split('/').some(p => !p || p === '.' || p === '..') || name.includes('\\') || name.includes('\0') || names.has(name)) fail('unsafe or duplicate npm tar member');
    names.add(name);
    if (![0, 48, 53].includes(type)) fail('unsupported npm tar member: only regular files and directories are accepted');
    files.push({ name, directory: type === 53, data });
  }
  if (pax !== null) fail('npm PAX header has no following entry');
  if (!files.length || offset + 1024 > tar.length || !tar.subarray(offset).every(b => b === 0)) fail('invalid npm tar end marker');
  const manifest = files.find(file => file.name === 'package/package.json' && !file.directory);
  if (!manifest) fail('npm tarball is missing package/package.json');
  const pkg = JSON.parse(manifest.data.toString('utf8'));
  if (pkg?.name !== baseline.name || pkg?.version !== baseline.version) fail('npm tarball package identity differs from baseline');
  fs.mkdirSync(destination); // A fresh destination, never a reused extraction.
  try {
    for (const file of files) {
      const target = path.join(destination, file.name);
      fs.mkdirSync(file.directory ? target : path.dirname(target), { recursive: true });
      if (!file.directory) fs.writeFileSync(target, file.data, { flag: 'wx' });
    }
  } catch (error) {
    fs.rmSync(destination, { recursive: true, force: true });
    throw error;
  }
}

function entryPoints(dir) {
  const pkg = json(path.join(dir, 'package.json'));
  if (typeof pkg.name !== 'string') fail('package.json is missing name');
  version(pkg.version);
  const entries = new Map();
  function add(key, target) {
    if (typeof target !== 'string' || !/\.d\.[cm]?ts$/.test(target)) fail(`unsupported declaration entry ${key}: ${target}`);
    if (target.includes('*')) fail(`wildcard declaration entry is not supported: ${key}`);
    const full = path.resolve(dir, target);
    if (!full.startsWith(dir + path.sep) || !fs.existsSync(full)) fail(`missing or external declaration entry ${pkg.name} ${key}: ${target}`);
    if (entries.has(key) && entries.get(key) !== full) add(`${key}#types`, target);
    else entries.set(key, full);
  }
  if (pkg.types || pkg.typings) add('.', pkg.types || pkg.typings);
  function walk(value, key, condition = '') {
    if (typeof value === 'string') {
      if (/\.d\.[cm]?ts$/.test(value)) add(condition ? `${key}#${condition}` : key, value);
      return;
    }
    if (value === null) return;
    if (Array.isArray(value)) { value.forEach((v, i) => walk(v, key, `${condition}[${i}]`)); return; }
    if (typeof value !== 'object') fail(`invalid exports entry ${key}`);
    for (const [k, v] of Object.entries(value).sort(([a], [b]) => order(a, b))) {
      if (k.startsWith('.')) walk(v, k);
      else walk(v, key, k === 'types' ? condition : [condition, k].filter(Boolean).join('/'));
    }
  }
  if (pkg.exports) walk(pkg.exports, '.');
  if (!entries.size) fail(`${pkg.name} has no declaration entry points`);
  return { pkg, entries };
}

// Separate roots prevent package identity and private class symbols from being
// accidentally shared when comparing a directory with itself. Only declarations
// are copied; package code is never loaded or executed.
function copyDeclarations(source, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const e of fs.readdirSync(source, { withFileTypes: true })) {
    if (e.name === 'node_modules' || e.name === '.git') continue;
    const from = path.join(source, e.name), to = path.join(dest, e.name);
    if (e.isSymbolicLink()) fail(`symbolic link in declaration package: ${e.name}`);
    if (e.isDirectory()) copyDeclarations(from, to);
    else if (/\.d\.[cm]?ts$/.test(e.name) || e.name === 'package.json') fs.copyFileSync(from, to);
  }
}

export function diffPackages(oldDir, newDir) {
  const ts = typescript();
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'rustev-npm-api-'));
  try {
    const dirs = ['old', 'new'].map(v => path.join(scratch, v));
    [oldDir, newDir].forEach((d, i) => copyDeclarations(path.resolve(d), dirs[i]));
    const [old, next] = dirs.map(entryPoints);
    if (old.pkg.name !== next.pkg.name) fail('package names differ');
    const program = ts.createProgram([...old.entries.values(), ...next.entries.values()], {
      strict: true, noEmit: true, skipLibCheck: false, types: [],
      target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.NodeNext,
      moduleResolution: ts.ModuleResolutionKind.NodeNext,
    });
    const diagnostics = ts.getPreEmitDiagnostics(program);
    if (diagnostics.length) fail(`declaration diagnostics: ${diagnostics.map(d =>
      `TS${d.code} ${ts.flattenDiagnosticMessageText(d.messageText, ' ')}`).sort(order).join('; ')}`);
    for (const file of program.getSourceFiles()) {
      if (!file.fileName.startsWith(scratch + path.sep) && !program.isSourceFileDefaultLibrary(file))
        fail('declaration imports outside the package are not supported');
    }
    const oldFiles = program.getSourceFiles().filter(f => f.fileName.startsWith(dirs[0] + path.sep));
    const newFiles = program.getSourceFiles().filter(f => f.fileName.startsWith(dirs[1] + path.sep));
    const texts = (files, dir) => files.map(f => [path.relative(dir, f.fileName), f.text]).sort(([a], [b]) => order(a, b));
    const sameEntries = JSON.stringify([...old.entries].map(([k, v]) => [k, path.relative(dirs[0], v)])) ===
      JSON.stringify([...next.entries].map(([k, v]) => [k, path.relative(dirs[1], v)]));
    if (sameEntries && JSON.stringify(texts(oldFiles, dirs[0])) === JSON.stringify(texts(newFiles, dirs[1])))
      return { schema: 'rustev.npm-api-diff/1', package: old.pkg.name, oldVersion: old.pkg.version, newVersion: next.pkg.version, findings: [] };
    const checker = program.getTypeChecker();
    const print = type => checker.typeToString(type, undefined,
      ts.TypeFormatFlags.NoTruncation | ts.TypeFormatFlags.InTypeAlias).replaceAll(scratch, '<package>')
      .replaceAll('<package>/old/', '<package>/').replaceAll('<package>/new/', '<package>/');
    const decl = s => s.valueDeclaration || s.declarations?.[0];
    const typeOf = s => checker.getTypeOfSymbolAtLocation(s, decl(s));
    const optional = s => !!(s.flags & ts.SymbolFlags.Optional);
    const readonly = s => (s.declarations ?? []).some(d =>
      (ts.getCombinedModifierFlags(d) & ts.ModifierFlags.Readonly) ||
      (ts.isVariableDeclaration(d) && (d.parent.flags & ts.NodeFlags.Const)) ||
      (ts.isGetAccessorDeclaration(d) && !(s.flags & ts.SymbolFlags.SetAccessor)));
    const publicMember = s => !(s.declarations ?? []).some(d =>
      (ts.getCombinedModifierFlags(d) & (ts.ModifierFlags.Private | ts.ModifierFlags.Protected)) ||
      (d.name && ts.isPrivateIdentifier(d.name)));
    const findings = [];
    const add = (p, breaking, reason) => findings.push({ export: p, class: breaking ? 'breaking' : 'compatible', reason: `031 3.8: ${reason}` });
    function assign(a, b, p, flow) {
      const ap = print(a), bp = print(b);
      // Unresolved generic/conditional relations and any cannot prove a changed
      // declaration safe. The pinned checker still decides ordinary relations.
      const uncertain = t => !!(t.flags & (ts.TypeFlags.Any | ts.TypeFlags.TypeParameter | ts.TypeFlags.Conditional | ts.TypeFlags.IndexedAccess));
      if (ap !== bp && (uncertain(a) || uncertain(b))) { add(p, true, `cannot decide assignability conservatively (${flow}): ${ap} -> ${bp}`); return; }
      let ok;
      try { ok = checker.isTypeAssignableTo(a, b); }
      catch { add(p, true, `cannot decide assignability conservatively (${flow})`); return; }
      if (!ok) add(p, true, `incompatible ${flow}: ${ap} is not assignable to ${bp}`);
      else if (ap !== bp && !checker.isTypeAssignableTo(b, a)) add(p, false, `compatible ${flow}: ${ap} is assignable to ${bp}`);
    }
    const active = new Set();
    function signatures(a, b, p, kind) {
      const aa = checker.getSignaturesOfType(a, kind), bb = checker.getSignaturesOfType(b, kind);
      const sigText = s => checker.signatureToString(s, undefined, ts.TypeFormatFlags.NoTruncation);
      for (const s of aa) {
        const attempts = bb.map(t => {
          const start = findings.length;
          if (s.typeParameters?.length || t.typeParameters?.length) {
            if (sigText(s) === sigText(t) && checker.isTypeAssignableTo(b, a)) return [];
            add(p, true, 'cannot decide changed generic signature conservatively');
            return findings.splice(start);
          }
          if (t.minArgumentCount > s.minArgumentCount) add(p, true, 'required parameter added or optional parameter made required');
          if (!!s.thisParameter !== !!t.thisParameter) add(p, true, 'explicit this parameter changed');
          else if (s.thisParameter) assign(typeOf(s.thisParameter), typeOf(t.thisParameter), `${p}.this`, 'input');
          s.parameters.forEach((param, i) => {
            const target = t.parameters[i];
            if (target) assign(typeOf(param), typeOf(target), `${p}.parameter[${i}]`, 'input');
            else add(p, true, 'parameter removed');
          });
          assign(checker.getReturnTypeOfSignature(t), checker.getReturnTypeOfSignature(s), `${p}.return`, 'output');
          return findings.splice(start);
        });
        const good = attempts.find(result => !result.some(f => f.class === 'breaking'));
        if (good) findings.push(...good);
        else if (attempts.length) findings.push(...attempts.sort((x, y) => order(JSON.stringify(x), JSON.stringify(y)))[0]);
        else add(p, true, `signature removed: ${sigText(s)}`);
      }
      for (const t of bb) if (!aa.some(s => sigText(s) === sigText(t)) && !findings.some(f => f.export.startsWith(p) && f.class === 'breaking'))
        add(p, false, `signature added or changed compatibly: ${sigText(t)}`);
    }
    function members(a, b, p) {
      const key = `${a.id}:${b.id}`;
      if (active.has(key)) return;
      active.add(key);
      try {
        const aa = new Map(checker.getPropertiesOfType(a).filter(publicMember).map(s => [s.name, s]));
        const bb = new Map(checker.getPropertiesOfType(b).filter(publicMember).map(s => [s.name, s]));
        for (const [name, s] of aa) {
          const t = bb.get(name), at = `${p}.${name}`;
          if (!t) { add(at, true, 'member removed'); continue; }
          if (optional(s) && !optional(t)) add(at, true, 'optional member made required');
          if (!readonly(s) && readonly(t)) add(at, true, 'writable member made readonly');
          const st = typeOf(s), tt = typeOf(t);
          if (checker.getSignaturesOfType(st, ts.SignatureKind.Call).length) signatures(st, tt, at, ts.SignatureKind.Call);
          else {
            // A readonly member only flows out; a writable one flows both
            // ways, so widening or narrowing it is breaking (3.6.5).
            assign(tt, st, at, 'output');
            if (!readonly(s)) assign(st, tt, at, 'input');
            if (st.symbol?.declarations?.some(ts.isTypeLiteralNode) && tt.symbol?.declarations?.some(ts.isTypeLiteralNode)) members(st, tt, at);
          }
        }
        for (const [name, s] of bb) if (!aa.has(name)) add(`${p}.${name}`, !optional(s), optional(s) ? 'optional member added' : 'required member added');
        for (const kind of [ts.SignatureKind.Call, ts.SignatureKind.Construct]) signatures(a, b, p, kind);
        const ai = checker.getIndexInfosOfType(a), bi = checker.getIndexInfosOfType(b);
        for (const s of ai) {
          const t = bi.find(v => print(v.keyType) === print(s.keyType));
          if (!t) add(`${p}.[${print(s.keyType)}]`, true, 'index signature removed');
          else {
            if (!s.isReadonly && t.isReadonly) add(p, true, 'writable index signature made readonly');
            // As for members: values read through it flow out, and a writable
            // one also takes values in (3.6.5).
            assign(t.type, s.type, p, 'output');
            if (!s.isReadonly) assign(s.type, t.type, p, 'input');
          }
        }
      } finally { active.delete(key); }
    }
    function surface(entries) {
      const result = new Map();
      function visit(module, prefix, seen = new Set()) {
        if (seen.has(module)) return;
        const chain = new Set([...seen, module]);
        for (let s of checker.getExportsOfModule(module)) {
          const name = s.name;
          if (s.flags & ts.SymbolFlags.Alias) s = checker.getAliasedSymbol(s);
          const p = `${prefix}.${name}`;
          result.set(p, s);
          if (s.flags & ts.SymbolFlags.NamespaceModule || s.flags & ts.SymbolFlags.ValueModule) visit(s, p, chain);
        }
      }
      for (const [key, file] of entries) {
        const module = checker.getSymbolAtLocation(program.getSourceFile(file));
        if (!module) fail(`declaration entry ${key} is not a module`);
        visit(module, key);
      }
      return result;
    }
    const before = surface(old.entries), after = surface(next.entries);
    for (const key of old.entries.keys()) if (!next.entries.has(key)) add(key, true, 'declaration entry point removed');
    for (const key of next.entries.keys()) if (!old.entries.has(key)) add(key, false, 'declaration entry point added');
    const kind = s => s.flags & (ts.SymbolFlags.Class | ts.SymbolFlags.Interface | ts.SymbolFlags.TypeAlias |
      ts.SymbolFlags.Enum | ts.SymbolFlags.Function | ts.SymbolFlags.Variable | ts.SymbolFlags.Module);
    for (const [p, s] of before) {
      const t = after.get(p);
      if (!t) { add(p, true, 'export removed'); continue; }
      if (kind(s) !== kind(t)) { add(p, true, 'export kind changed'); continue; }
      if (s.flags & (ts.SymbolFlags.Interface | ts.SymbolFlags.Class | ts.SymbolFlags.TypeAlias)) {
        const parameters = symbol => (symbol.declarations ?? []).flatMap(d =>
          (d.typeParameters ?? []).map(param => param.getText())).join(',');
        if (parameters(s) !== parameters(t)) add(p, true, 'cannot decide changed generic declaration conservatively');
        const a = checker.getDeclaredTypeOfSymbol(s), b = checker.getDeclaredTypeOfSymbol(t);
        if (a.flags & ts.TypeFlags.Object && b.flags & ts.TypeFlags.Object && !checker.isArrayType(a) && !checker.isTupleType(a) && !checker.isArrayType(b) && !checker.isTupleType(b)) members(a, b, p);
        else { assign(a, b, p, 'input'); assign(b, a, p, 'output'); }
        if (s.flags & ts.SymbolFlags.Class) members(typeOf(s), typeOf(t), `${p}.static`);
      } else if (s.flags & ts.SymbolFlags.Function) signatures(typeOf(s), typeOf(t), p, ts.SignatureKind.Call);
      else if (s.flags & ts.SymbolFlags.Enum) {
        const a = s.exports, b = t.exports;
        for (const [name, member] of a) {
          const other = b.get(name);
          if (!other) add(`${p}.${name}`, true, 'enum member removed');
          else if (checker.getConstantValue(decl(member)) !== checker.getConstantValue(decl(other))) add(`${p}.${name}`, true, 'enum value changed');
        }
        for (const [name] of b) if (!a.has(name)) add(`${p}.${name}`, false, 'enum member added');
      } else if (!(s.flags & ts.SymbolFlags.Module)) {
        if (!readonly(s) && readonly(t)) add(p, true, 'writable export made const');
        // An exported value is read by consumers; an `export let` is also
        // written, so it is checked both ways (3.6.5).
        assign(typeOf(t), typeOf(s), p, 'output');
        if (!readonly(s)) assign(typeOf(s), typeOf(t), p, 'input');
      }
    }
    for (const p of after.keys()) if (!before.has(p)) add(p, false, 'export added');
    const unique = [...new Map(findings.map(f => [JSON.stringify(f), f])).values()];
    unique.sort((a, b) => order(a.export, b.export) || order(a.class, b.class) || order(a.reason, b.reason));
    return { schema: 'rustev.npm-api-diff/1', package: old.pkg.name, oldVersion: old.pkg.version, newVersion: next.pkg.version, findings: unique };
  } finally { fs.rmSync(scratch, { recursive: true, force: true }); }
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    if (args[0] === '--check-typescript') typescript();
    else if (args[0] === '--baseline') {
      const baseline = selectBaseline(json(args[1]), args[2]);
      if (baseline) {
        if (args[3] && baseline.name !== args[3]) fail('npm baseline package name does not match requested package');
        if (!baseline.dist?.tarball || !baseline.dist?.integrity) fail('npm baseline is missing dist.tarball or dist.integrity');
        if (new URL(baseline.dist.tarball).protocol !== 'https:') fail('npm tarball must use HTTPS');
        process.stdout.write(JSON.stringify(baseline));
      }
    } else if (args[0] === '--unpack') unpackTarball(fs.readFileSync(args[1]), json(args[2]), args[3]);
    else if (args[0] === '--integrity') verifyIntegrity(fs.readFileSync(args[1]), json(args[2]).dist.integrity);
    else if (args[0] === '--version-rule') {
      const report = json(args[1]);
      process.exitCode = versionAllows(report.oldVersion, report.newVersion, report.findings) ? 0 : 2;
    } else {
      if (args.length !== 2) fail('usage: npm-api-diff.mjs <old-package-dir> <new-package-dir>');
      const report = diffPackages(...args);
      console.log(JSON.stringify(report, null, 2));
      process.exitCode = report.findings.some(f => f.class === 'breaking') ? 2 : 0;
    }
  } catch (error) {
    console.error(error.message.startsWith('031 3.8:') ? error.message : `031 3.8: ${error.message}`);
    process.exitCode = 1;
  }
}
