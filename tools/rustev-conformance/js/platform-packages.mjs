// Spec 027, 3.13.1 and 3.13.2: the native package's platform packages, in
// napi-rs's optional-dependency layout under integrations/rustev-node/npm/.
//
//   node platform-packages.mjs check
//       every optional dependency of the main package has its platform
//       package, with the same version, the os, cpu and C library its name
//       names, the addon as its only file and no script, and the main
//       package has no install-time script
//   node platform-packages.mjs stage <cargo target directory>
//       put this host's release addon into its platform package
//   node platform-packages.mjs fresh <directory>
//       pack the main package and this host's platform package and install
//       both into a fresh project there, from the tarballs alone; prints
//       the installed main package's directory
//
// Nothing is published and nothing is downloaded: the fresh install omits
// the optional dependencies, so npm never fetches another platform's
// package, and the host's own is installed from its tarball.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { addonName, builtAddon } from './copy-addon.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const nodePkg = resolve(here, '../../../integrations/rustev-node');
const npmDir = join(nodePkg, 'npm');
const read = (p) => JSON.parse(readFileSync(p, 'utf8'));
const main = read(join(nodePkg, 'package.json'));
const PREFIX = '@rustev/rustev-';
// npm's lifecycle scripts that run when a package is installed (3.13.2:
// no install-time code). A platform package has no script at all.
const INSTALL_SCRIPTS = ['preinstall', 'install', 'postinstall', 'prepare'];
// Marks a directory `fresh` made, so a rerun may replace it.
const MARKER = '.rustev-fresh';

/** This host's platform name, or a clean exit naming the platform. */
function hostPlatform() {
  try {
    return addonName().replace(/^rustev\./, '').replace(/\.node$/, '');
  } catch (e) {
    console.error(`platform-packages: ${e.message}`);
    process.exit(1);
  }
}

/** npm, without a shell: on Windows `npm` is a `.cmd` shim that only a
 * shell runs, so npm's own entry point runs under this Node instead, and
 * no path is ever parsed by cmd.exe. */
function npm(args, options) {
  if (process.platform !== 'win32') return execFileSync('npm', args, options);
  const cli = join(dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js');
  if (!existsSync(cli)) {
    console.error(`platform-packages: no npm beside this Node (${cli})`);
    process.exit(1);
  }
  return execFileSync(process.execPath, [cli, ...args], options);
}

// The os, cpu and C library a platform name stands for (the loader's
// names, `index.js` platformName).
function expected(platform) {
  const m = /^(linux|darwin|win32)-(x64|arm64)(?:-(gnu|musl|msvc))?$/.exec(platform);
  if (!m) return null;
  const [, os, cpu, abi] = m;
  if ((os === 'linux') !== (abi === 'gnu' || abi === 'musl')) return null;
  if ((os === 'win32') !== (abi === 'msvc')) return null;
  return { os, cpu, libc: os === 'linux' ? (abi === 'gnu' ? 'glibc' : 'musl') : undefined };
}

function check() {
  const failures = [];
  const deps = Object.entries(main.optionalDependencies ?? {});
  if (deps.length === 0) failures.push('the main package names no platform package');
  for (const k of INSTALL_SCRIPTS) {
    if (main.scripts?.[k] !== undefined) failures.push(`${main.name}: has the install-time script ${k} (3.13.2)`);
  }
  // Platform package directories only: a stray file (such as macOS's
  // .DS_Store) is not a package.
  const dirs = existsSync(npmDir)
    ? readdirSync(npmDir, { withFileTypes: true })
        .filter((e) => e.isDirectory() && !e.name.startsWith('.'))
        .map((e) => e.name)
        .sort()
    : [];
  const named = [];
  for (const [name, version] of deps) {
    if (!name.startsWith(PREFIX)) {
      failures.push(`${name}: not a ${PREFIX}<platform> package`);
      continue;
    }
    const platform = name.slice(PREFIX.length);
    named.push(platform);
    const want = expected(platform);
    if (!want) {
      failures.push(`${name}: ${platform} is no platform the loader names`);
      continue;
    }
    const file = join(npmDir, platform, 'package.json');
    if (!existsSync(file)) {
      failures.push(`${name}: no ${file}`);
      continue;
    }
    const p = read(file);
    const addon = `rustev.${platform}.node`;
    const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
    if (p.name !== name) failures.push(`${file}: name ${p.name}, the main package names ${name}`);
    if (version !== main.version || p.version !== main.version) {
      failures.push(`${name}: version ${p.version} (named ${version}), the main package is ${main.version}`);
    }
    if (!same(p.os, [want.os])) failures.push(`${name}: os ${JSON.stringify(p.os)}, want ["${want.os}"]`);
    if (!same(p.cpu, [want.cpu])) failures.push(`${name}: cpu ${JSON.stringify(p.cpu)}, want ["${want.cpu}"]`);
    if (!same(p.libc, want.libc && [want.libc])) {
      failures.push(`${name}: libc ${JSON.stringify(p.libc)}, want ${JSON.stringify(want.libc && [want.libc])}`);
    }
    if (p.main !== addon || !same(p.files, [addon])) failures.push(`${name}: main and files must be ${addon} alone`);
    if (p.scripts) failures.push(`${name}: has scripts (3.13.2 allows none)`);
    if (p.license !== main.license || !same(p.engines, main.engines)) {
      failures.push(`${name}: license and engines must be the main package's`);
    }
    if (p.private !== main.private) failures.push(`${name}: private must be the main package's (${main.private})`);
  }
  for (const d of dirs) {
    if (!named.includes(d)) failures.push(`npm/${d}: no optional dependency of the main package names it`);
  }
  for (const f of failures) console.error(f);
  if (failures.length) process.exit(1);
  console.log(`platform packages: ${named.length} checked`);
}

function stage(target) {
  const platform = hostPlatform();
  const name = `rustev.${platform}.node`;
  const dir = join(npmDir, platform);
  if (!existsSync(join(dir, 'package.json'))) {
    console.error(`stage: no platform package for ${platform}`);
    process.exit(1);
  }
  try {
    copyFileSync(builtAddon(target), join(dir, name));
  } catch (e) {
    console.error(`stage: ${e.message}`);
    process.exit(1);
  }
  console.log(`staged ${name} into npm/${platform}`);
  return { dir, platform };
}

function pack(dir, into) {
  // `--json` names the tarball; `--ignore-scripts` although none exist.
  const out = npm(['pack', '--json', '--ignore-scripts', '--pack-destination', into, dir], { encoding: 'utf8' });
  // npm names the tarball it wrote; anything but one plain file name in
  // `into` is refused by name, never joined.
  let filename;
  try {
    filename = JSON.parse(out)[0]?.filename;
  } catch (e) {
    console.error(`fresh: npm pack ${dir} printed no JSON: ${e.message}`);
    process.exit(1);
  }
  if (typeof filename !== 'string' || basename(filename) !== filename || filename === '..' || !filename.endsWith('.tgz')) {
    console.error(`fresh: npm pack ${dir} named no tarball file: ${JSON.stringify(filename)}`);
    process.exit(1);
  }
  const tgz = join(into, filename);
  if (!existsSync(tgz)) {
    console.error(`fresh: npm pack ${dir} wrote no ${tgz}`);
    process.exit(1);
  }
  return tgz;
}

function fresh(into) {
  const platform = hostPlatform();
  const name = `rustev.${platform}.node`;
  const platDir = join(npmDir, platform);
  if (!existsSync(join(platDir, name))) {
    console.error(`fresh: npm/${platform}/${name} is not staged (run stage first)`);
    process.exit(1);
  }
  // Only a directory this command made (it holds the marker) is replaced;
  // any other existing path is refused, never deleted.
  if (existsSync(into)) {
    if (!existsSync(join(into, MARKER))) {
      console.error(`fresh: ${into} exists and was not made by this command; remove it or name another directory`);
      process.exit(1);
    }
    rmSync(into, { recursive: true, force: true });
  }
  mkdirSync(into, { recursive: true });
  writeFileSync(join(into, MARKER), 'made by platform-packages.mjs fresh\n');
  const tarballs = join(into, 'tarballs');
  mkdirSync(tarballs);
  const mainTgz = pack(nodePkg, tarballs);
  const platTgz = pack(platDir, tarballs);
  writeFileSync(join(into, 'package.json'), JSON.stringify({ name: 'rustev-fresh-consumer', private: true }) + '\n');
  npm(
    ['install', '--omit=optional', '--ignore-scripts', '--no-audit', '--no-fund', '--no-save', mainTgz, platTgz],
    // npm's output goes to stderr: stdout is the installed directory alone.
    { cwd: into, stdio: ['ignore', 2, 2] },
  );
  const installed = join(into, 'node_modules', main.name);
  // The installed main package carries no addon of its own: it loads the
  // platform package, as a consumer's install does.
  const local = readdirSync(installed).filter((f) => f.endsWith('.node'));
  if (local.length) {
    console.error(`fresh: the main package's tarball carries ${local.join(', ')}`);
    process.exit(1);
  }
  console.log(installed);
}

const [cmd, arg] = process.argv.slice(2);
if (cmd === 'check') check();
else if (cmd === 'stage' && arg) stage(arg);
else if (cmd === 'fresh' && arg) fresh(resolve(arg));
else {
  console.error('usage: node platform-packages.mjs check | stage <cargo target directory> | fresh <directory>');
  process.exit(2);
}
