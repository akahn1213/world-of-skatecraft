#!/usr/bin/env python3
"""Reject unsafe compatibility shortcuts in the prepared Playerbots module.

Native type, lifetime, wire-format and database behavior is exercised separately by
test_runtime.py. These source checks prevent the audited shortcuts being restored.
"""
import argparse
from pathlib import Path
import re
import subprocess


def active_source(text):
    text = re.sub(r'^\s*#\s*include[^\n]*', '', text, flags=re.M)
    return subprocess.check_output(
        ['c++', '-E', '-P', '-x', 'c++', '-DCMANGOS', '-DVMANGOS',
         '-DMANGOSBOT_ZERO', '-DPLATFORM=0', '-DPLATFORM_WINDOWS=1', '-'],
        input=text, text=True, stderr=subprocess.PIPE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    args = parser.parse_args()
    module = args.source / 'src/game/AutonomousBots'
    assert (module / 'botpch.h').is_file()
    failures = []
    for path in sorted(module.rglob('*')):
        if path.suffix not in ('.h', '.cpp'):
            continue
        text = path.read_text()
        name = str(path.relative_to(module))
        if re.search(r'^\s*#\s*define\s+DT_POLYREF64', text, re.M):
            failures.append(f'{name}: Detour layout must come from the native headers')
        if not any(token in text for token in ('GetString(', 'reinterpret_cast', 'ChannelAcces')):
            continue
        active = active_source(text)
        # ObjectGuid::GetString is an owning formatter, not the nullable Field API.
        if name == 'playerbot/strategy/actions/GossipHelloAction.cpp':
            active = active.replace('guid.GetString()', 'guid_formatter')
        if re.search(r'(?:\.|->)\s*GetString\s*\(', active):
            failures.append(f'{name}: use owning, null-safe Field::GetCppString')
        if 'ChannelAcces' in active:
            failures.append(f'{name}: use the native Channel membership API')
        # The only audited byte reinterpretation reads into an already-sized string.
        if name == 'playerbot/strategy/actions/EmoteAction.cpp':
            active = active.replace('reinterpret_cast<uint8*>(&nam[0])', 'string_bytes')
        if 'reinterpret_cast' in active:
            failures.append(f'{name}: native object layouts cannot be reinterpreted')
    assert not failures, '\n'.join(failures)
    print('Native contract guards passed: database strings, channel access, Detour ABI, object casts')


if __name__ == '__main__':
    main()
