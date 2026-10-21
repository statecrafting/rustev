// Spec 027, 3.11: put the native addon `cargo build --release` produced
// beside the package as `rustev.<platform>.node`, named as the platform
// packages name it, so the package loads its local build.
//
//   node copy-addon.mjs <cargo target directory> <package dir>
//
// `addonName` and `builtAddon` are shared with platform-packages.mjs, which
// stages the same file into this host's platform package (3.13.1).

import { copyFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** `rustev.<platform>.node` for this host, as the loader names it. */
export function addonName() {
  const { platform, arch } = process;
  if (!{ darwin: 1, linux: 1, win32: 1 }[platform]) {
    throw new Error(`no native package for platform ${platform}`);
  }
  // The diagnostic report names the C library; it runs once, on Linux only,
  // without its network section (the costly part) where this Node lets it
  // be left out. `excludeNetwork` is an own property of process.report only
  // on a Node that supports leaving it out; elsewhere assigning it would
  // add a plain property the report never reads, so it is not assigned and
  // the report runs whole, which is slower but correct.
  let glibc;
  if (platform === 'linux') {
    if (typeof process.report?.getReport !== 'function') {
      throw new Error('this Node has no diagnostic report to tell glibc from musl');
    }
    const had = Object.hasOwn(process.report, 'excludeNetwork') ? process.report.excludeNetwork : undefined;
    if (had !== undefined) process.report.excludeNetwork = true;
    glibc = process.report.getReport().header?.glibcVersionRuntime;
    if (had !== undefined) process.report.excludeNetwork = had;
  }
  const libc =
    platform === 'linux'
      ? glibc
        ? '-gnu'
        : '-musl'
      : platform === 'win32'
        ? '-msvc'
        : '';
  return `rustev.${platform}-${arch}${libc}.node`;
}

/** The release addon cargo built for this host under `target`. */
export function builtAddon(target) {
  const { platform } = process;
  const ext = { darwin: 'dylib', linux: 'so', win32: 'dll' }[platform];
  if (!ext) throw new Error(`no native package for platform ${platform}`);
  const lib = platform === 'win32' ? 'rustev_node' : 'librustev_node';
  return join(target, 'release', `${lib}.${ext}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [target, pkg] = process.argv.slice(2);
  if (!target || !pkg) {
    console.error('usage: node copy-addon.mjs <cargo target directory> <package dir>');
    process.exit(2);
  }
  let name;
  try {
    name = addonName();
  } catch (e) {
    console.error(`copy-addon: ${e.message}`);
    process.exit(1);
  }
  copyFileSync(builtAddon(target), join(pkg, name));
  console.log(`copied ${name}`);
}
