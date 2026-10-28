"""JSON, TOML and subprocess support for the POSIX qualification entry point.

Only public registries are consumed. No fixture bytes or workspace dependencies
are supplied to the consumer. An incomplete run can never produce passed.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

HERE = Path(__file__).resolve().parent
INDEX = 'registry+https://github.com/rust-lang/crates.io-index'
REPO = 'statecrafting/rustev'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_bytes())


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def guard(work):
    for parent in (work, *work.parents):
        if any((parent / name).exists() for name in ('Cargo.toml', 'package.json')):
            raise ValueError(f'work directory is inside a workspace: {parent}')
    # Refuse visible overrides before replacing homes. Never read credentials.
    homes = [Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo'))]
    configs = [h / n for h in homes for n in ('config', 'config.toml')]
    configs += [p / '.cargo' / n for p in (work, *work.parents)
                for n in ('config', 'config.toml')]
    for path in configs:
        if path.exists():
            cfg = tomllib.loads(path.read_text())
            if any(k in cfg for k in ('patch', 'source', 'registries', 'registry', 'paths')):
                raise ValueError(f'forbidden Cargo configuration: {path}')
            if any(k in cfg.get('build', {}) for k in ('rustc', 'rustc-wrapper', 'rustc-workspace-wrapper')):
                raise ValueError(f'compiler override: {path}')


def lock_checks(lock, manifest, complete=False):
    packages = tomllib.loads(lock.read_text())['package']
    # The generated consumer (adoption.sh's `rustev-adoption-*` or the
    # harness's `rustev-qualification-*`) is the lockfile's only package
    # without a source; any other is a path dependency.
    roots = [p['name'] for p in packages if 'source' not in p]
    if len(roots) != 1 or not roots[0].startswith(('rustev-adoption-', 'rustev-qualification-')):
        raise ValueError(f'path dependency: {", ".join(roots) or "no consumer root"}')
    checksums = {}
    expected = {p['name']: p['sha256'] for p in manifest['packages']}
    for p in packages:
        if 'source' not in p:
            continue
        if p['source'] != INDEX or not p.get('checksum'):
            raise ValueError(f'non-public-registry source: {p["name"]}')
        checksums[f'{p["name"]}@{p["version"]}'] = p['checksum']
        if p['name'] in expected:
            if p['version'] != manifest['version'] or p['checksum'] != expected[p['name']]:
                raise ValueError(f'signed checksum/version mismatch: {p["name"]}')
    if complete and not set(expected).issubset({p['name'] for p in packages}):
        raise ValueError('release-set crate missing from lockfile')
    return checksums


class NotRun(Exception):
    pass


class Harness:
    def __init__(self, version, manifest, evidence, work):
        self.version, self.manifest_path = version, manifest.resolve()
        self.m = read(manifest)
        if self.m['schema'] != 'rustev.release-manifest/2' or self.m['version'] != version or self.m['tag'] != 'v' + version:
            raise ValueError('manifest identity differs from requested release')
        self.work, self.evidence = work.resolve(), evidence.resolve()
        guard(self.work)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.env = dict(os.environ)
        for key in list(self.env):
            if key.startswith(('CARGO_', 'NPM_CONFIG_', 'npm_config_', 'RUSTEV_ADOPTION_', 'GIT_CONFIG_')) or key in (
                'NPM_TOKEN', 'NODE_AUTH_TOKEN', 'RUSTUP_TOOLCHAIN', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                'RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTC', 'RUSTDOC', 'NODE_OPTIONS', 'NODE_PATH'):
                self.env.pop(key)
        self.env['RUSTUP_HOME'] = os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup'))
        for key, name in [('HOME', 'home'), ('CARGO_HOME', 'cargo'), ('npm_config_cache', 'npm-cache')]:
            path = self.work / name
            path.mkdir()
            self.env[key] = str(path)
        self.env.update(npm_config_registry='https://registry.npmjs.org',
                        npm_config_userconfig=os.devnull, npm_config_globalconfig=os.devnull,
                        TMPDIR=str(self.work), GIT_CONFIG_NOSYSTEM='1')
        self.env['RUSTUP_AUTO_INSTALL'] = '0'
        self.env['RUSTUP_TOOLCHAIN'] = tomllib.loads((HERE.parents[2] / 'rust-toolchain.toml').read_text())['toolchain']['channel']
        self.os = self.identity('uname', '-s')
        self.arch = self.identity('uname', '-m')
        arch = {'aarch64': 'arm64', 'arm64': 'arm64', 'x86_64': 'x64', 'AMD64': 'x64'}.get(self.arch, self.arch)
        if self.os == 'Darwin':
            self.platform = 'darwin-' + arch
        elif self.os == 'Linux':
            if not shutil.which('ldd'):
                raise ValueError('cannot identify Linux libc: ldd is unavailable')
            libc = subprocess.run(['ldd', '--version'], capture_output=True, check=False)
            output = (libc.stdout + libc.stderr).lower()
            if b'musl' in output:
                suffix = 'musl'
            elif b'glibc' in output or b'gnu libc' in output:
                suffix = 'gnu'
            else:
                raise ValueError('cannot identify Linux libc')
            self.platform = f'linux-{arch}-{suffix}'
        elif self.os.startswith(('MINGW', 'MSYS')):
            self.platform = 'win32-' + arch + '-msvc'
        else:
            self.platform = 'unsupported-' + arch
        self.commands, self.checksums = [], {}
        self.offline_prefix = None

    def identity(self, *cmd):
        try:
            p = subprocess.run(cmd, cwd=self.work, env=self.env, capture_output=True, text=True, check=False)
            return p.stdout.strip() if p.returncode == 0 else 'unavailable: ' + ' '.join(cmd)
        except FileNotFoundError:
            return 'unavailable: ' + cmd[0]

    def run(self, *cmd, cwd=None):
        self.commands.append(' '.join(map(str, cmd)))
        self.log.write(('$ ' + self.commands[-1] + '\n').encode())
        self.log.flush()
        subprocess.run(list(map(str, cmd)), cwd=cwd or self.work, env=self.env,
                       stdout=self.log, stderr=subprocess.STDOUT, check=True)

    def offline_run(self, *cmd, cwd=None):
        # Cargo offline mode also forbids registry access. Deny socket access
        # to the consumer process where the host exposes a usable sandbox.
        if self.offline_prefix is None:
            choices = []
            if shutil.which('sandbox-exec'):
                choices.append(['sandbox-exec', '-p', '(version 1)(allow default)(deny network*)'])
            if shutil.which('unshare'):
                choices.append(['unshare', '-n', '--'])
            self.offline_prefix = []
            for prefix in choices:
                probe = subprocess.run(prefix + ['true'], cwd=self.work, env=self.env,
                                       stdout=self.log, stderr=self.log, check=False)
                if probe.returncode == 0:
                    self.offline_prefix = prefix
                    break
            if not self.offline_prefix:
                self.log.write(b'Network namespace/sandbox unavailable; registry offline mode only.\n')
        self.run(*self.offline_prefix, *cmd, cwd=cwd)

    def need(self, *names):
        for name in names:
            if not shutil.which(name):
                raise NotRun(f'{name} is not installed')

    def verify_manifest(self):
        if not Path(str(self.manifest_path) + '.sig').exists():
            # Spec 058 3.2.5: a tagged run's manifest carries that run's
            # build provenance attestation, not an owner signature.
            self.need('gh')
            self.run('gh', 'attestation', 'verify', self.manifest_path, '--repo', REPO,
                     '--signer-workflow', REPO + '/.github/workflows/release.yml',
                     '--source-ref', 'refs/tags/' + self.m['tag'], '--source-digest', self.m['commit'])
            return
        self.need('ssh-keygen')
        self.commands.append('ssh-keygen -Y verify -f allowed_signers -I 7887446+bartekus@users.noreply.github.com -n rustev-release -s ' + str(self.manifest_path) + '.sig')
        with self.manifest_path.open('rb') as data:
            subprocess.run(['ssh-keygen', '-Y', 'verify', '-f', str(HERE.parent / 'allowed_signers'),
                            '-I', '7887446+bartekus@users.noreply.github.com', '-n', 'rustev-release',
                            '-s', str(self.manifest_path) + '.sig'], stdin=data,
                           stdout=self.log, stderr=subprocess.STDOUT, check=True, env=self.env)

    def project(self, name, packages):
        path = self.work / name
        (path / 'src').mkdir(parents=True)
        deps = '\n'.join(f'{p} = "={self.version}"' for p in packages)
        (path / 'Cargo.toml').write_text(
            f'[package]\nname = "rustev-qualification-{name}"\nversion = "0.0.0"\nedition = "{self.m["edition"]}"\n'
            '[workspace]\n[dependencies]\n' + deps + '\nserde_json = "1"\ntokio = { version = "1", features = ["rt", "macros", "time"] }\n')
        return path

    def example(self):
        if (self.work / 'example/expected.json').exists():
            return self.work / 'example'
        # A step that failed part way leaves a partial project; the next
        # step rebuilds it from nothing rather than trusting or tripping on it.
        shutil.rmtree(self.work / 'example', ignore_errors=True)
        path = self.project('example', ['rustev-pkg-support-routing', 'rustev-backend-rules', 'rustev-contract', 'rustev-core', 'rustev-runtime', 'rustev-remote-http'])
        shutil.copyfile(HERE / 'consumer.rs', path / 'src/lib.rs')
        (path / 'src/main.rs').write_text('fn main() { rustev_qualification_example::export(); }\n')
        self.run('cargo', 'generate-lockfile', cwd=path)
        self.checksums.update(lock_checks(path / 'Cargo.lock', self.m))
        self.run('cargo', 'run', '--locked', cwd=path)
        return path

    def cli_smoke(self, binary, path):
        self.run(binary, 'version')
        self.run(binary, '--version')
        self.run(binary, '--help')
        # Check the reported identity, not just a successful exit.
        got = subprocess.check_output([str(binary), 'version'], env=self.env, cwd=path)
        self.log.write(got)
        if got != f'rustev {self.version}\n'.encode():
            raise ValueError('CLI version mismatch')
        args = ['--definition', 'definition.json', '--rules', 'rules.json', '--calibration', 'calibration.json']
        self.run(binary, 'plan', 'check', *args, cwd=path)
        self.run(binary, 'plan', 'compile', *args, '--out', 'plan.json', cwd=path)
        self.offline_run(binary, 'run', '--plan', 'plan.json', '--rules', 'rules.json', '--calibration', 'calibration.json',
                 '--snapshot', 'snapshot.json', '--evaluation-time', (path / 'time.txt').read_text(),
                 '--decision-id', 'SYNTHETIC-qualification', '--record-out', 'record.json', cwd=path)
        if read(path / 'record.json')['core']['recorded']['judgment']['outcome'] != read(path / 'expected.json')['outcome']:
            raise ValueError('CLI example outcome mismatch')

    def crates(self):
        self.need('cargo', 'rustc', 'rustup')
        msrv = self.m['msrv']
        if msrv.count('.') == 1:
            msrv += '.0'
        toolchains = list(dict.fromkeys([self.env['RUSTUP_TOOLCHAIN'], msrv]))
        for toolchain in toolchains:
            try:
                self.run('rustc', '+' + toolchain, '--version')
            except subprocess.CalledProcessError as error:
                raise NotRun('required toolchain unavailable: ' + toolchain) from error
        names = json.loads((HERE.parent / 'release-set.json').read_text())['upload_order']
        if set(names) != {p['name'] for p in self.m['packages']}:
            raise ValueError('manifest crate set differs from release set')
        path = self.project('all', names)
        (path / 'src/lib.rs').write_text('// All exact release dependencies are fetched.\n')
        self.run('cargo', 'generate-lockfile', cwd=path)
        self.checksums.update(lock_checks(path / 'Cargo.lock', self.m, complete=True))
        self.run('cargo', 'fetch', '--locked', cwd=path)
        cargo_home = Path(self.env['CARGO_HOME'])
        for package in self.m['packages']:
            archives = list(cargo_home.glob('registry/cache/*/' + package['crate_file']))
            if len(archives) != 1 or digest(archives[0]) != package['sha256']:
                raise ValueError('downloaded crate differs from signed manifest: ' + package['name'])
        cli_locks = list(cargo_home.glob('registry/src/*/rustev-cli-' + self.version + '/Cargo.lock'))
        if len(cli_locks) != 1:
            raise ValueError('published CLI locked install has no unique lockfile')
        self.checksums.update(lock_checks(cli_locks[0], self.m))
        example = self.example()
        for toolchain in toolchains:
            self.offline_run('cargo', '+' + toolchain, 'test', '--locked', '--offline', cwd=example)
            self.env['RUSTEV_ADOPTION_VERSION'] = self.version
            self.env['RUSTEV_ADOPTION_MANIFEST'] = str(self.manifest_path)
            self.env['RUSTEV_ADOPTION_EVIDENCE'] = str(self.evidence / (self.platform + '.crates.adoption-' + toolchain))
            previous = self.env['RUSTUP_TOOLCHAIN']
            self.env['RUSTUP_TOOLCHAIN'] = toolchain
            self.run('sh', HERE.parent / 'adoption.sh')
            self.env['RUSTUP_TOOLCHAIN'] = previous
        self.run('cargo', 'install', 'rustev-cli', '--version', '=' + self.version, '--locked')
        executable = 'rustev.exe' if self.platform.startswith('win32') else 'rustev'
        self.cli_smoke(Path(self.env['CARGO_HOME']) / 'bin' / executable, example)

    def npm(self):
        self.need('npm', 'node', 'cargo')
        path = self.work / 'node'
        path.mkdir()
        write(path / 'package.json', {'name': 'rustev-qualification', 'version': '0.0.0', 'private': True})
        self.run('npm', 'install', '--ignore-scripts', '--save-exact', '@rustev/rustev@' + self.version,
                 '@rustev/rustev-wasm@' + self.version, cwd=path)
        audit = path / 'audit.json'
        self.commands.append('npm audit signatures --json --include-attestations')
        with audit.open('wb') as output:
            subprocess.run(['npm', 'audit', 'signatures', '--json', '--include-attestations'], cwd=path,
                           env=self.env, stdout=output, stderr=self.log, check=True)
        self.log.write(audit.read_bytes())
        verified = read(audit).get('verified', [])
        artifacts = {a.get('package'): a for a in self.m['artifacts'] if a['kind'] == 'npm'}
        installed = set()
        for location, p in read(path / 'package-lock.json')['packages'].items():
            if not location or not (path / location).exists():
                continue
            name = read(path / location / 'package.json')['name']
            if not p.get('resolved', '').startswith('https://registry.npmjs.org/'):
                raise ValueError('non-public npm source: ' + name)
            if name.startswith('@rustev/'):
                installed.add(name)
                a = artifacts.get(name)
                if not a or p['version'] != self.version or p['integrity'] != a['integrity']:
                    raise ValueError('npm manifest integrity mismatch: ' + name)
                if not any(v['name'] == name and v['version'] == self.version and
                           v.get('attestations', {}).get('provenance') for v in verified):
                    raise ValueError('missing verified npm provenance: ' + name)
                self.checksums[name] = p['integrity']
        if not {'@rustev/rustev', '@rustev/rustev-wasm'}.issubset(installed):
            raise ValueError('required npm package is not installed')
        shutil.copyfile(HERE / 'smoke.cjs', path / 'smoke.cjs')
        self.offline_run('node', 'smoke.cjs', self.version, self.example(), cwd=path)

    def cli(self):
        self.need('gh', 'cargo')
        name = f'rustev-{self.version}-{self.platform}.tar.gz'
        artifact = next((a for a in self.m['artifacts'] if a['kind'] == 'binary' and a['name'] == name), None)
        if artifact is None:
            raise NotRun('manifest has no CLI archive for this host')
        path = self.work / 'cli'
        path.mkdir()
        self.run('gh', 'release', 'download', self.m['tag'], '--repo', REPO, '--pattern', name,
                 '--pattern', name + '.sha256', '--dir', path)
        archive = path / name
        if digest(archive) != artifact['sha256'] or (path / (name + '.sha256')).read_text() != f'{artifact["sha256"]}  {name}\n':
            raise ValueError('CLI archive checksum mismatch')
        self.checksums[name] = digest(archive)
        self.run('gh', 'attestation', 'verify', archive, '--repo', REPO,
                 '--signer-workflow', REPO + '/.github/workflows/release.yml',
                 '--source-ref', 'refs/tags/' + self.m['tag'], '--source-digest', self.m['commit'])
        import tarfile
        with tarfile.open(archive) as tar:
            members = tar.getmembers()
            directory = name.removesuffix('.tar.gz')
            executable = 'rustev.exe' if self.platform.startswith('win32') else 'rustev'
            expected = {directory, directory + '/LICENSE', directory + '/' + executable}
            if {m.name.rstrip('/') for m in members} != expected or len(members) != 3:
                raise ValueError('unexpected CLI archive layout')
            for member in members:
                if member.name.rstrip('/') == directory:
                    if not member.isdir():
                        raise ValueError('CLI archive root is not a directory')
                elif not member.isfile():
                    raise ValueError('CLI archive contains a link or special file')
            # The extraction filter (PEP 706) is in 3.12 and the 3.11.4
            # security release; an earlier 3.11 has no `filter` argument.
            # The member checks above already refuse links, special files
            # and anything outside the archive's root directory.
            if hasattr(tarfile, 'data_filter'):
                tar.extractall(path, members=members, filter='data')
            else:
                tar.extractall(path, members=members)
        binary = path / directory / executable
        self.run(binary, 'version')
        self.run(binary, '--help')
        self.cli_smoke(binary, self.example())

    def execute(self):
        failed = False
        for kind in ('crates', 'npm', 'cli', 'image'):
            required = bool(self.m['packages']) if kind == 'crates' else any(
                a['kind'] == ('binary' if kind == 'cli' else kind) for a in self.m['artifacts'])
            prefix = self.evidence / f'{self.platform}.{kind}'
            self.commands, self.checksums = [], {}
            status, code = None, None
            with Path(str(prefix) + '.log').open('wb') as self.log:
                try:
                    if not required:
                        named = {'image': 'container image', 'cli': 'binary artifact', 'npm': 'npm artifact'}.get(kind, kind)
                        status = {'excluded': {'reason': 'the signed manifest names no ' + named}}
                    elif os.environ.get('RUSTEV_QUALIFY_DRY') == '1':
                        raise NotRun('dry run')
                    elif kind == 'image':
                        raise NotRun('container qualification is not implemented')
                    elif self.platform.startswith('unsupported'):
                        raise NotRun('unsupported host platform')
                    else:
                        self.verify_manifest()
                        getattr(self, kind)()
                        status, code = 'passed', 0
                except NotRun as e:
                    status = {'not_run': {'reason': str(e)}}
                except (OSError, ValueError, KeyError, subprocess.SubprocessError) as e:
                    status, code = 'failed', getattr(e, 'returncode', 1)
                    self.log.write((str(e) + '\n').encode())
            records = Path(str(prefix) + '.records')
            records.mkdir(exist_ok=True)
            for pattern in ('**/Cargo.lock', '**/package-lock.json', '**/audit.json', '**/.crates*.json', '**/.crates.toml', 'example/*.json', 'example/time.txt', 'cli/*.sha256'):
                for source in self.work.glob(pattern):
                    dest = records / source.relative_to(self.work)
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, dest)
            write(Path(str(prefix) + '.json'), dict(
                schema='rustev.qualification-step/1', manifest_sha256=digest(self.manifest_path), kind=kind,
                platform=self.platform, command=self.commands, exit_status=code,
                output_digest=digest(Path(str(prefix) + '.log')), lockfile_or_install_record=str(records),
                checksums=self.checksums, host=self.identity('uname', '-n'), os=self.os, architecture=self.arch,
                toolchain=self.identity('rustc', '--version'), node=self.identity('node', '--version'), status=status))
            print(f'{self.platform}.{kind}: {status}')
            failed |= required and status != 'passed'
        return int(failed)


if __name__ == '__main__':
    try:
        if sys.argv[1] == 'lock':
            lock_checks(Path(sys.argv[2]), read(Path(sys.argv[3])))
        else:
            sys.exit(Harness(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])).execute())
    except (ValueError, OSError, KeyError) as error:
        print(f'qualify: {error}', file=sys.stderr)
        sys.exit(2)
