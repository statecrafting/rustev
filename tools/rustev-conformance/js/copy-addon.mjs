// Spec 027, 3.11: put the native addon `cargo build --release` produced
// beside the package as `rustev.<platform>.node`, named as the platform
// packages name it, so the package loads its local build.
//
//   node copy-addon.mjs <cargo target directory> <package dir>

import { copyFileSync } from 'node:fs';
import { join } from 'node:path';

const [target, pkg] = process.argv.slice(2);
if (!target || !pkg) {
  console.error('usage: node copy-addon.mjs <cargo target directory> <package dir>');
  process.exit(2);
}
const { platform, arch } = process;
const ext = { darwin: 'dylib', linux: 'so', win32: 'dll' }[platform];
if (!ext) {
  console.error(`copy-addon: no native package for platform ${platform}`);
  process.exit(1);
}
const lib = platform === 'win32' ? 'rustev_node' : 'librustev_node';
// The diagnostic report names the C library; it runs once, on Linux only,
// without its network section (the costly part).
let glibc;
if (platform === 'linux') {
  if (typeof process.report?.getReport !== 'function') {
    console.error('copy-addon: this Node has no diagnostic report to tell glibc from musl');
    process.exit(1);
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
const name = `rustev.${platform}-${arch}${libc}.node`;
copyFileSync(join(target, 'release', `${lib}.${ext}`), join(pkg, name));
console.log(`copied ${name}`);
