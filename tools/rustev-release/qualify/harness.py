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
import time
import tomllib

HERE = Path(__file__).resolve().parent
# The release's own data (release set, adoption subsets, toolchain pin) come
# from the released tree. A later harness qualifying an earlier tag is given
# that tree's checkout; by default it is the harness's own tree (056 3.5).
RELEASE_TREE = Path(os.environ.get('RUSTEV_QUALIFY_RELEASE_TREE') or HERE.parents[2]).resolve()
if not (RELEASE_TREE / 'tools/rustev-release/release-set.json').is_file():
    sys.exit('qualify: RUSTEV_QUALIFY_RELEASE_TREE is not a checkout of a released tree')
INDEX = 'registry+https://github.com/rust-lang/crates.io-index'
REPO = 'statecrafting/rustev'
# Spec 060 3.4.3: the image is qualified on these hosts, as these platforms.
IMAGE_PLATFORMS = {'linux-x64-gnu': 'linux/amd64', 'linux-arm64-gnu': 'linux/arm64'}


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


def lock_checks(lock, manifest, complete=False, root=None):
    packages = tomllib.loads(lock.read_text())['package']
    # The generated consumer (adoption.sh's `rustev-adoption-*` or the
    # harness's `rustev-qualification-*`), or the published package whose
    # own lockfile this is (`root`), is the lockfile's only package without
    # a source; any other is a path dependency.
    roots = [p['name'] for p in packages if 'source' not in p]
    if len(roots) != 1 or not (roots[0] == root if root else roots[0].startswith(('rustev-adoption-', 'rustev-qualification-'))):
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
        # Two distinct empty files: npm refuses one file loaded as both its
        # user and its global configuration ("double-loading config").
        for name in ('npmrc-user', 'npmrc-global'):
            (self.work / name).write_text('')
        self.env.update(npm_config_registry='https://registry.npmjs.org',
                        npm_config_userconfig=str(self.work / 'npmrc-user'),
                        npm_config_globalconfig=str(self.work / 'npmrc-global'),
                        TMPDIR=str(self.work), GIT_CONFIG_NOSYSTEM='1')
        self.env['RUSTUP_AUTO_INSTALL'] = '0'
        self.env['RUSTUP_TOOLCHAIN'] = tomllib.loads((RELEASE_TREE / 'rust-toolchain.toml').read_text())['toolchain']['channel']
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
            p = subprocess.run([self.program(cmd[0]), *cmd[1:]], cwd=self.work, env=self.env,
                               capture_output=True, text=True, check=False)
            return p.stdout.strip() if p.returncode == 0 else 'unavailable: ' + ' '.join(cmd)
        except FileNotFoundError:
            return 'unavailable: ' + cmd[0]

    def program(self, name):
        # Windows installs npm as npm.cmd, which a process starts only by its
        # full path; resolve every program on the consumer environment's PATH.
        return shutil.which(str(name), path=self.env.get('PATH')) or str(name)

    def run(self, *cmd, cwd=None):
        self.commands.append(' '.join(map(str, cmd)))
        self.log.write(('$ ' + self.commands[-1] + '\n').encode())
        self.log.flush()
        argv = list(map(str, cmd))
        subprocess.run([self.program(argv[0])] + argv[1:], cwd=cwd or self.work, env=self.env,
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
        # The example directory is shared by the crates and cli kinds, and the
        # CLI never overwrites an output, so each smoke run starts without them.
        for output in ('plan.json', 'record.json'):
            (path / output).unlink(missing_ok=True)
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
        names = json.loads((RELEASE_TREE / 'tools/rustev-release/release-set.json').read_text())['upload_order']
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
        self.checksums.update(lock_checks(cli_locks[0], self.m, root='rustev-cli'))
        example = self.example()
        for toolchain in toolchains:
            self.offline_run('cargo', '+' + toolchain, 'test', '--locked', '--offline', cwd=example)
            self.env['RUSTEV_ADOPTION_VERSION'] = self.version
            self.env['RUSTEV_ADOPTION_MANIFEST'] = str(self.manifest_path)
            self.env['RUSTEV_ADOPTION_EVIDENCE'] = str(self.evidence / (self.platform + '.crates.adoption-' + toolchain))
            previous = self.env['RUSTUP_TOOLCHAIN']
            self.env['RUSTUP_TOOLCHAIN'] = toolchain
            self.run('sh', RELEASE_TREE / 'tools/rustev-release/adoption.sh')
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
            subprocess.run([self.program('npm'), 'audit', 'signatures', '--json', '--include-attestations'], cwd=path,
                           env=self.env, stdout=output, stderr=self.log, check=True)
        self.log.write(audit.read_bytes())
        # `npm audit signatures --json` lists only failures; it exits nonzero on
        # any, and every installed package is then signature-verified.
        report = read(audit)
        unverified = {v.get('name') for key in ('invalid', 'missing') for v in report.get(key, [])}
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
                # The registry signature must verify; npm provenance is not
                # required (056 3.3, 031 3.9.6, A-74).
                if name in unverified:
                    raise ValueError('npm registry signature not verified: ' + name)
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

    def image_serve(self, ref, platform, plan_id, work):
        """Start `ref` on a read-only root with no network, its configuration
        mounted read-only and its Unix socket in a private directory, register
        the synthetic plan unsigned and decide it once (060 3.4.3). Returns
        the decision document. The socket belongs to user 65532, so the
        host's requests go through `sudo curl`."""
        config, sock = work / 'config', work / 'sock'
        config.mkdir()
        sock.mkdir(mode=0o700)
        shutil.copyfile(HERE / 'image/serve.json', config / 'serve.json')
        config.chmod(0o755)
        (config / 'serve.json').chmod(0o644)
        self.run('sudo', '-n', 'chown', '65532:65532', sock)
        definition = read(HERE / 'image/definition.json')
        report = dict(command='plan compile', out='plan.json', plan_id=plan_id, status='compiled')
        write(work / 'register.json', dict(
            schema='rustev.serve-register/1', signature='none',
            bundle=dict(schema='rustev.plan-bundle/1', definition=definition, execution_policy='none',
                        calibrations=[], bindings={}, compile_report=report)))
        name = 'rustev-qualify-' + platform.replace('/', '-')
        try:
            # Inside the try: once the socket directory belongs to 65532, a
            # failed start still reaches the cleanup below.
            self.run('docker', 'run', '-d', '--name', name, '--platform', platform, '--read-only', '--network', 'none',
                     '-v', f'{config}:/etc/rustev:ro', '-v', f'{sock}:/run/rustev', ref)
            socket = sock / 'serve.sock'
            for _ in range(60):
                if subprocess.run(['sudo', '-n', 'test', '-S', str(socket)], check=False).returncode == 0:
                    break
                time.sleep(1)
            else:
                raise ValueError('the service did not open its socket within 60 s')

            def call(method, path, body, expected):
                out = work / 'response.json'
                # Relative to the socket's directory: a Unix socket path is
                # limited to about 100 bytes. The directory is 0700 and owned
                # by 65532, so the change into it happens under sudo too.
                argv = ['sudo', '-n', 'sh', '-c', 'cd "$1" && shift && exec "$@"', 'sh', str(sock), 'curl', '-sS', '--max-time', '60', '--unix-socket', socket.name, '-X', method,
                        '-H', 'Content-Type: application/json', '-H', 'rustev-principal: cXVhbGlmaWNhdGlvbg',
                        '--data-binary', '@' + str(body), '-o', str(out), '-w', '%{http_code}',
                        'http://localhost' + path]
                self.commands.append(' '.join(argv))
                self.log.write(('$ ' + self.commands[-1] + '\n').encode())
                http = subprocess.run(argv, env=self.env, capture_output=True, text=True, check=True)
                self.log.write(f'HTTP {http.stdout}\n{http.stderr}'.encode())
                document = read(out)
                if http.stdout.strip() != '200':
                    raise ValueError(f'{method} {path} answered HTTP {http.stdout.strip()}: {document}')
                if document.get('schema') == 'rustev.serve-refusal/1':
                    raise ValueError(f'{method} {path} refused: {document}')
                if expected and document.get('schema') != expected:
                    raise ValueError(f'{method} {path} answered {document.get("schema")}')
                return document

            registration = call('PUT', f'/v1/plans/{plan_id}', work / 'register.json', 'rustev.plan-registration/1')
            if registration.get('plan_id') != plan_id or registration.get('signer') != 'unsigned':
                raise ValueError('the registration is not the synthetic plan, unsigned')
            return call('POST', f'/v1/plans/{plan_id}/decisions', HERE / 'image/decide.json', 'rustev.serve-decision/1')
        finally:
            subprocess.run(['docker', 'stop', '-t', '10', name], stdout=self.log, stderr=subprocess.STDOUT, check=False)
            subprocess.run(['docker', 'logs', name], stdout=self.log, stderr=subprocess.STDOUT, check=False)
            subprocess.run(['docker', 'rm', '-f', name], stdout=self.log, stderr=subprocess.STDOUT, check=False)
            # The socket directory belongs to 65532; the work tree is removed later.
            subprocess.run(['sudo', '-n', 'rm', '-rf', str(sock)], stdout=self.log, stderr=subprocess.STDOUT, check=False)

    def image(self):
        """Spec 060 3.4.3 (031 3.9.7 (c)): pull the image by its index digest,
        verify its provenance and SBOM attestations, check it with 042's
        check.sh and serve one decision of an exact-only synthetic plan,
        whose judgment must equal the golden."""
        self.need('docker', 'gh', 'curl', 'sudo')
        image = self.m.get('image')
        platform = IMAGE_PLATFORMS.get(self.platform)
        if not image or not platform:
            raise ValueError('image qualification on a host that needs none')
        entry = next((p for p in image['platforms'] if p['platform'] == platform), None)
        if entry is None:
            raise ValueError(f'the manifest names no {platform} image')
        repository, index = image['repository'], image['index_digest']
        ref = f'{repository}@{index}'
        self.run('docker', 'pull', '--platform', platform, ref)
        got = subprocess.run(['docker', 'image', 'inspect', '--format', '{{json .RepoDigests}}', ref],
                             capture_output=True, text=True, check=True).stdout
        if ref not in json.loads(got):
            raise ValueError(f'the pulled image is not {ref}')
        signer = ['--repo', REPO, '--signer-workflow', REPO + '/.github/workflows/release.yml',
                  '--source-ref', 'refs/tags/' + self.m['tag']]
        self.run('gh', 'attestation', 'verify', 'oci://' + ref, *signer)
        self.run('gh', 'attestation', 'verify', f'oci://{repository}@{entry["digest"]}',
                 '--predicate-type', 'https://cyclonedx.org/bom', *signer)
        self.run('sh', RELEASE_TREE / 'integrations/rustev-serve/image/check.sh', ref, platform)
        # The plan's identity from the release's own verified CLI (the cli step).
        cli = self.work / 'cli' / f'rustev-{self.version}-{self.platform}' / 'rustev'
        if not cli.is_file():
            raise ValueError('the cli step did not leave the verified CLI')
        work = self.work / 'image'
        work.mkdir()
        shutil.copyfile(HERE / 'image/definition.json', work / 'definition.json')
        compiled = subprocess.run([str(cli), 'plan', 'compile', '--definition', 'definition.json', '--out', 'plan.json'],
                                  cwd=work, env=self.env, capture_output=True, check=True)
        self.log.write(compiled.stdout)
        plan_id = json.loads(compiled.stdout)['plan_id']
        decision = self.image_serve(ref, platform, plan_id, work)
        write(work / 'decision.json', decision)
        expected = read(HERE / 'image/judgment.json')
        expected['plan_id'] = plan_id
        if decision.get('status') != 'judged' or decision.get('plan_id') != plan_id:
            raise ValueError(f'the decision is {decision.get("status")}, not judged for the plan')
        if decision.get('judgment') != expected:
            raise ValueError('the judgment differs from the golden')
        self.checksums[f'{repository}@{platform}'] = entry['digest'].removeprefix('sha256:')
        self.checksums[repository] = index.removeprefix('sha256:')

    def execute(self):
        failed = False
        for kind in ('crates', 'npm', 'cli', 'image'):
            if kind == 'crates':
                required = bool(self.m['packages'])
            elif kind == 'image':
                # 060 3.4.3: a named image is qualified on amd64 and arm64 Linux.
                required = bool(self.m.get('image')) and self.platform in IMAGE_PLATFORMS
            else:
                required = any(a['kind'] == ('binary' if kind == 'cli' else kind) for a in self.m['artifacts'])
            prefix = self.evidence / f'{self.platform}.{kind}'
            self.commands, self.checksums = [], {}
            status, code = None, None
            with Path(str(prefix) + '.log').open('wb') as self.log:
                try:
                    if not required and kind == 'image' and self.m.get('image'):
                        status = {'excluded': {'reason': 'the image is qualified on linux/amd64 and linux/arm64 only (060 3.4.3)'}}
                    elif not required:
                        named = {'image': 'container image', 'cli': 'binary artifact', 'npm': 'npm artifact'}.get(kind, kind)
                        status = {'excluded': {'reason': 'the signed manifest names no ' + named}}
                    elif os.environ.get('RUSTEV_QUALIFY_DRY') == '1':
                        raise NotRun('dry run')
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
            for pattern in ('**/Cargo.lock', '**/package-lock.json', '**/audit.json', '**/.crates*.json', '**/.crates.toml', 'example/*.json', 'example/time.txt', 'cli/*.sha256', 'image/*.json'):
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
