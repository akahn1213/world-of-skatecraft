#!/usr/bin/env python3
"""Build an isolated Playerbots image; does not edit Compose or start a deployment."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import re
import subprocess
import tempfile
import uuid

import prepare

# build_cached.sh used by the previous BuildKit-cache implementation.
LEGACY_SCRIPT_SHA256 = '70047de1896c6dbfe3e2623c59173385d942edf5e8debb74bfbe5ccee9eb6b87'

BASE_IMAGE = 'ghcr.io/mserajnik/vmangos-server@sha256:af7bb9e62d69a017d42b3de30b13dd13371161d3f0d493a1b2a5ff8b3c8b128f'


def positive_int(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError('must be positive')
    return number


def managed_builder(memory, cpus):
    # Names encode the limits; never modify the user's default builder.
    name = 'playerbots-' + hashlib.sha256(f'v2/{memory}/{cpus}'.encode()).hexdigest()[:12]
    inspected = subprocess.run(['docker', 'buildx', 'inspect', name], capture_output=True)
    if inspected.returncode:
        subprocess.run(['docker', 'buildx', 'create', '--name', name,
                        '--driver', 'docker-container', '--driver-opt', 'network=host',
                        '--driver-opt', f'memory={memory}',
                        '--driver-opt', f'memory-swap={memory}',
                        '--driver-opt', 'cpu-period=100000',
                        '--driver-opt', f'cpu-quota={cpus * 100000}'], check=True)
    subprocess.run(['docker', 'buildx', 'inspect', '--bootstrap', name], check=True)
    return name


def persistent_cache(base_image, profile):
    # Docker-daemon scope, independent of sudo/Buildx config and resource limits.
    return 'playerbots-objects-' + hashlib.sha256((base_image + '/' + profile).encode()).hexdigest()[:16]


def toolchain_image(builder, base_image):
    recipe = prepare.HERE / 'Dockerfile.toolchain'
    key = hashlib.sha256(base_image.encode() + recipe.read_bytes()).hexdigest()[:20]
    tag = 'world-of-skatecraft/playerbots-toolchain:' + key
    if subprocess.run(['docker', 'image', 'inspect', tag], capture_output=True).returncode:
        subprocess.run(['docker', 'buildx', 'build', '--builder', builder, '--load',
                        '--progress=plain', '--file', str(recipe), '--tag', tag,
                        '--build-arg', 'BASE_IMAGE=' + base_image, str(prepare.HERE)], check=True)
    else:
        print('Reusing installed toolchain image: ' + tag, flush=True)
    return tag


def import_legacy_cache(builder, base_image, profile, volume, toolchain, legacy_id=None):
    legacy_id = legacy_id or 'playerbots-v1-' + hashlib.sha256((base_image + '/' + profile).encode()).hexdigest()[:16]
    with tempfile.TemporaryDirectory(prefix='playerbots-legacy-') as temp:
        root = Path(temp)
        (root / 'Dockerfile').write_text(f"""# syntax=docker/dockerfile:1
ARG BASE_IMAGE
FROM ${{BASE_IMAGE}} AS export
ARG MIGRATION_NONCE
RUN --mount=type=cache,id={legacy_id},target=/build/cache,sharing=locked mkdir /export && if [ -f /build/cache/output/build.ninja ]; then tar --format=posix --exclude=./build.lock -C /build/cache -cf /export/legacy.tar .; else touch /export/EMPTY; fi
FROM scratch
COPY --from=export /export/ /
""")
        # --no-cache can empty cache mounts. Invalidate only this export RUN.
        subprocess.run(['docker', 'buildx', 'build', '--builder', builder, '--progress=plain',
                        '--build-arg', 'MIGRATION_NONCE=' + uuid.uuid4().hex, '--build-arg', 'BASE_IMAGE=' + base_image, '--output', 'type=local,dest=' + str(root / 'export'), str(root)], check=True)
        archive = root / 'export/legacy.tar'
        if not archive.exists():
            print('No previous BuildKit object cache found; a cold build is necessary.', flush=True)
            return
        # Lock and recheck: another invocation might have initialized this volume.
        command = f"""flock /build/cache/build.lock sh -c 'if [ ! -f /build/cache/output/build.ninja ]; then tar -xf /import/legacy.tar -C /build/cache; printf "%s\\n" "{LEGACY_SCRIPT_SHA256}" > /build/cache/legacy-script-sha; fi'"""
        subprocess.run(['docker', 'run', '--rm', '--network', 'none', '--entrypoint', 'sh',
                        '--mount', 'type=volume,src=' + volume + ',dst=/build/cache',
                        '--mount', 'type=bind,src=' + str(archive.parent) + ',dst=/import,readonly',
                        toolchain, '-c', command], check=True)


def build_prepared(source, provenance, args, builder, toolchain, volume, build_script=None):
    """Same runner for real sources and regression fixtures; each call is a new container."""
    name = 'playerbots-compile-' + uuid.uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix='playerbots-package-') as temp:
        context = Path(temp)
        try:
            subprocess.run(['docker', 'run', '--name', name, '--memory', args.memory,
                            '--memory-swap', args.memory, '--cpus', str(args.cpus), '--network', 'none',
                            '--mount', 'type=volume,src=' + volume + ',dst=/build/cache',
                            '--mount', 'type=bind,src=' + str(source.resolve()) + ',dst=/build/source,readonly',
                            '--mount', 'type=bind,src=' + str(build_script or prepare.HERE / 'build_cached.sh') + ',dst=/build/build_cached.sh,readonly',
                            '--env', 'BUILD_JOBS=' + str(args.jobs), '--env', 'DEBUG_PROFILE=' + args.debug_profile,
                            '--entrypoint', 'sh', toolchain, '/build/build_cached.sh'], check=True)
            # docker cp creates files owned by the caller, including non-root Docker users.
            subprocess.run(['docker', 'cp', name + ':/package', str(context / 'package')], check=True)
        finally:
            subprocess.run(['docker', 'rm', '-f', name], stdout=subprocess.DEVNULL, check=False)
        metadata = context / 'package/opt/vmangos/share/playerbots/provenance.json'
        metadata.write_text(json.dumps(provenance, indent=2) + '\n')
        shutil.copy2(prepare.HERE / 'Dockerfile', context / 'Dockerfile')
        toolchain_id = subprocess.check_output(['docker', 'image', 'inspect', '--format', '{{.Id}}', toolchain], text=True).strip()
        subprocess.run(['docker', 'buildx', 'build', '--builder', builder, '--load', '--progress=plain',
                        '--tag', args.tag, '--build-arg', 'BASE_IMAGE=' + args.base_image,
                        '--build-arg', 'TOOLCHAIN_ID=' + toolchain_id, str(context)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--base-image', default=BASE_IMAGE)
    parser.add_argument('--jobs', type=positive_int, default=1)
    parser.add_argument('--memory', default='4g', help='RAM limit per build container; swap disabled (default: 4g)')
    parser.add_argument('--cpus', type=positive_int, default=2, help='CPU quota per build container (default: 2)')
    parser.add_argument('--debug-profile', choices=['lines', 'full'], default='lines')
    parser.add_argument('--core-cache', type=Path)
    parser.add_argument('--bots-cache', type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r'[1-9][0-9]*[mMgG]', args.memory):
        parser.error('--memory must be a positive number followed by m or g')
    args.memory = args.memory.lower()
    builder = managed_builder(args.memory, args.cpus)
    volume = persistent_cache(args.base_image, args.debug_profile)
    existed = subprocess.run(['docker', 'volume', 'inspect', volume], capture_output=True).returncode == 0
    subprocess.run(['docker', 'volume', 'create', '--label', 'world-of-skatecraft.playerbots-build=true', volume], check=True)
    toolchain = toolchain_image(builder, args.base_image)
    print('Persistent compiler volume: ' + volume + (' (existing)' if existed else ' (new)'), flush=True)
    probe = ['docker', 'run', '--rm', '--network', 'none', '--entrypoint', 'sh',
             '--mount', 'type=volume,src=' + volume + ',dst=/build/cache', toolchain, '-c']
    initialized = subprocess.run(probe + ['test -f /build/cache/output/build.ninja || test -f /build/cache/import-checked']).returncode == 0
    if not initialized:
        import_legacy_cache(builder, args.base_image, args.debug_profile, volume, toolchain)
        subprocess.run(probe + ['touch /build/cache/import-checked'], check=True)
    print(f'Builder {builder}: {args.jobs} compile job(s), {args.memory} RAM, '
          f'{args.cpus} CPUs, swap disabled; debug={args.debug_profile}', flush=True)
    inputs = [*prepare.HERE.glob('*.patch'), prepare.HERE / 'vmangos.cmake',
              prepare.HERE / 'prepare.py', prepare.HERE / 'prepare_sql.py',
              prepare.HERE / 'sources.json', prepare.HERE / 'Dockerfile',
              prepare.HERE / 'build_cached.sh', prepare.HERE / 'build_image.py', prepare.HERE / 'Dockerfile.toolchain',
              prepare.ROOT / 'setup/vmangos/skatecraft.patch',
              prepare.ROOT / 'setup/vmangos/tarnished.patch',
              prepare.ROOT / 'setup/vmangos/prepare_patch.py']
    provenance = {'base_image': args.base_image, 'sources': prepare.SOURCES,
                  'build': {'jobs': args.jobs, 'memory': args.memory, 'cpus': args.cpus,
                            'debug_profile': args.debug_profile, 'builder': builder, 'volume': volume,
                            'toolchain_image': subprocess.check_output(['docker', 'image', 'inspect', '--format', '{{.Id}}', toolchain], text=True).strip()},
                  'repository_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=prepare.ROOT, text=True).strip(),
                  'inputs_sha256': {str(p.relative_to(prepare.ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs}}
    # Temporary context contains source only, never the user's extracted assets or DB.
    with tempfile.TemporaryDirectory(prefix='playerbots-image-') as temp:
        context = Path(temp)
        source = prepare.prepare(context / 'source', args.core_cache, args.bots_cache)
        # Keep the pinned core's Git revision available to vmangos's version generator.
        subprocess.run(['git', 'remote', 'set-url', 'origin', prepare.SOURCES['vmangos']['url']], cwd=source, check=True)
        shutil.rmtree(source / 'src/game/AutonomousBots/.git')
        build_prepared(source, provenance, args, builder, toolchain, volume)
    print(f'Built {args.tag}; deployment and databases are unchanged.')


if __name__ == '__main__':
    main()
