#!/usr/bin/env python3
"""Check every compiled bot source for unsafe printf arguments using the real headers."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import shlex
import subprocess


def check(entry):
    command = shlex.split(entry['command'])
    args, skip = [command[0]], False
    for arg in command[1:]:
        if skip:
            skip = False
        elif arg in ('-o', '-MF', '-MT', '-MQ'):
            skip = True
        elif arg not in ('-c', '-MD', '-MMD'):
            args.append(arg)
    # Keep the configured include paths, macros and PCH. Empty log lines are legal;
    # mismatched types and nonliteral format strings without arguments are not.
    args += ['-fsyntax-only', '-Werror=format', '-Werror=format-security',
             '-Wno-format-zero-length']
    result = subprocess.run(args, cwd=entry['directory'], text=True, capture_output=True)
    return entry['file'], result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('build', type=Path)
    parser.add_argument('--jobs', type=int, default=4)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error('--jobs must be positive')
    entries = [c for c in json.loads((args.build / 'compile_commands.json').read_text())
               if '/AutonomousBots/' in c['file'] and c['file'].endswith('.cpp')]
    assert entries, 'No bot translation units found; configure BUILD_PLAYERBOTS=ON'
    failures = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for index, (filename, result) in enumerate(pool.map(check, entries), 1):
            if result.returncode:
                failures.append(filename)
                print(filename + '\n' + result.stderr, flush=True)
            if index % 100 == 0:
                print(f'Checked {index}/{len(entries)} bot sources', flush=True)
    assert not failures, f'{len(failures)} bot sources failed format checking'
    print(f'Format argument checks passed for all {len(entries)} compiled bot sources')


if __name__ == '__main__':
    main()
