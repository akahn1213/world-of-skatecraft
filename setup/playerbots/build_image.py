#!/usr/bin/env python3
"""Build an isolated Playerbots image; does not edit Compose or start a deployment."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

import prepare

BASE_IMAGE = 'ghcr.io/mserajnik/vmangos-server@sha256:af7bb9e62d69a017d42b3de30b13dd13371161d3f0d493a1b2a5ff8b3c8b128f'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--base-image', default=BASE_IMAGE)
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--core-cache', type=Path)
    parser.add_argument('--bots-cache', type=Path)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error('--jobs must be positive')
    inputs = [*prepare.HERE.glob('*.patch'), prepare.HERE / 'vmangos.cmake',
              prepare.HERE / 'prepare.py', prepare.HERE / 'prepare_sql.py',
              prepare.HERE / 'sources.json', prepare.HERE / 'Dockerfile',
              prepare.ROOT / 'setup/vmangos/skatecraft.patch',
              prepare.ROOT / 'setup/vmangos/tarnished.patch',
              prepare.ROOT / 'setup/vmangos/prepare_patch.py']
    provenance = {'base_image': args.base_image, 'sources': prepare.SOURCES,
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
        (context / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
        subprocess.run(['docker', 'build', '--tag', args.tag, '--build-arg', f'BASE_IMAGE={args.base_image}',
                        '--build-arg', f'BUILD_JOBS={args.jobs}', str(context)], check=True)
    print(f'Built {args.tag}; deployment and databases are unchanged.')


if __name__ == '__main__':
    main()
