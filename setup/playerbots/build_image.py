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

import prepare

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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--base-image', default=BASE_IMAGE)
    parser.add_argument('--jobs', type=positive_int, default=1)
    parser.add_argument('--memory', default='4g', help='BuildKit container RAM limit; swap disabled (default: 4g)')
    parser.add_argument('--cpus', type=positive_int, default=2, help='builder CPU quota (default: 2)')
    parser.add_argument('--debug-profile', choices=['lines', 'full'], default='lines')
    parser.add_argument('--core-cache', type=Path)
    parser.add_argument('--bots-cache', type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r'[1-9][0-9]*[mMgG]', args.memory):
        parser.error('--memory must be a positive number followed by m or g')
    args.memory = args.memory.lower()
    builder = managed_builder(args.memory, args.cpus)
    cache_id = 'playerbots-v1-' + hashlib.sha256(
        (args.base_image + '/' + args.debug_profile).encode()).hexdigest()[:16]
    print(f'Builder {builder}: {args.jobs} compile job(s), {args.memory} RAM, '
          f'{args.cpus} CPUs, swap disabled; debug={args.debug_profile}', flush=True)
    inputs = [*prepare.HERE.glob('*.patch'), prepare.HERE / 'vmangos.cmake',
              prepare.HERE / 'prepare.py', prepare.HERE / 'prepare_sql.py',
              prepare.HERE / 'sources.json', prepare.HERE / 'Dockerfile',
              prepare.HERE / 'build_cached.sh', prepare.HERE / 'build_image.py',
              prepare.ROOT / 'setup/vmangos/skatecraft.patch',
              prepare.ROOT / 'setup/vmangos/tarnished.patch',
              prepare.ROOT / 'setup/vmangos/prepare_patch.py']
    provenance = {'base_image': args.base_image, 'sources': prepare.SOURCES,
                  'build': {'jobs': args.jobs, 'memory': args.memory, 'cpus': args.cpus,
                            'debug_profile': args.debug_profile, 'builder': builder},
                  'repository_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=prepare.ROOT, text=True).strip(),
                  'inputs_sha256': {str(p.relative_to(prepare.ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs}}
    # Temporary context contains source only, never the user's extracted assets or DB.
    with tempfile.TemporaryDirectory(prefix='playerbots-image-') as temp:
        context = Path(temp)
        source = prepare.prepare(context / 'source', args.core_cache, args.bots_cache)
        # Keep the pinned core's Git revision available to vmangos's version generator.
        subprocess.run(['git', 'remote', 'set-url', 'origin', prepare.SOURCES['vmangos']['url']], cwd=source, check=True)
        shutil.rmtree(source / 'src/game/AutonomousBots/.git')
        shutil.copy2(prepare.HERE / 'Dockerfile', context / 'Dockerfile')
        shutil.copy2(prepare.HERE / 'build_cached.sh', context / 'build_cached.sh')
        (context / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
        subprocess.run(['docker', 'buildx', 'build', '--builder', builder, '--load', '--progress=plain',
                        '--tag', args.tag, '--build-arg', f'BASE_IMAGE={args.base_image}',
                        '--build-arg', f'BUILD_CACHE_ID={cache_id}',
                        '--build-arg', f'DEBUG_PROFILE={args.debug_profile}',
                        '--build-arg', f'BUILD_JOBS={args.jobs}', str(context)], check=True)
    print(f'Built {args.tag}; deployment and databases are unchanged.')


if __name__ == '__main__':
    main()
