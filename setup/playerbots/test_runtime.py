#!/usr/bin/env python3
"""Link regression fixtures to the actual built core; run cold/warm cache and mail paths.

Uses uniquely owned disposable databases and an ephemeral local DB user. No game assets
are used, so this does not validate navigation geometry or live gameplay.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import tempfile
import uuid

from prepare_sql import prepare_sql


def compile_fixture(build, output, sanitize=False):
    commands = json.loads((build / 'compile_commands.json').read_text())
    c = next(c for c in commands if c['file'].endswith('/VMangosBotConfig.cpp'))
    command = shlex.split(c['command'])
    flags, skip = [], False
    for arg in command[1:]:
        if skip:
            skip = False
        elif arg in ('-o', '-MF', '-MT', '-MQ', '-include'):
            skip = True
        elif arg not in ('-c', '-MD', '-MMD', '-DNDEBUG', c['file']) and not arg.startswith('-O'):
            flags.append(arg)
    libraries = ['src/game/libgame.a', 'src/scripts/libscripts.a', 'src/shared/libshared.a',
                 'src/framework/libframework.a', 'dep/g3dlite/libg3dlite.a',
                 'dep/recastnavigation/Detour/libDetour.a']
    cache = (build / 'CMakeCache.txt').read_text()
    mysql = re.search(r'^MYSQL_LIBRARY:[^=]+=(.+)$', cache, re.M)[1]
    source = Path(c['file']).parents[4]
    common = [command[0], *flags, '-O0', '-g', '-fno-access-control',
              '-Wno-deprecated-enum-enum-conversion']
    objects = []
    if sanitize:
        # The pinned native core deliberately packs its database structures, including
        # CreatureInfo strings. Its x86 build uses unaligned accesses; alignment
        # instrumentation cannot validate that ABI. Keep all other UB/ASan checks.
        common += ['-fsanitize=address,undefined,float-cast-overflow',
                   '-fno-sanitize=alignment', '-fno-omit-frame-pointer']
        module = source / 'src/game/AutonomousBots'
        # Instrument the affected production translation units, not a mock of them.
        # The remaining native core is linked from the normal build.
        for relative in ['playerbot/WorldPosition.cpp', 'playerbot/RandomPlayerbotMgr.cpp',
                         'ahbot/AhBot.cpp', 'ahbot/AhBotConfig.cpp']:
            obj = output.parent / (Path(relative).stem + '.o')
            subprocess.run([*common, '-include', str(module / 'botpch.h'), '-c',
                            str(module / relative), '-o', str(obj)], cwd=build, check=True)
            objects.append(str(obj))
    # Tests inspect cache state through private APIs, without production test hooks.
    subprocess.run([*common, str(Path(__file__).resolve().parent / 'tests/runtime.cpp'),
                    *objects, '-o', str(output), '-Wl,--start-group', *libraries,
                    '-Wl,--end-group', mysql, '-lssl', '-lcrypto', '-lz', '-ldl', '-pthread'],
                   cwd=build, check=True)
    return source



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('build', type=Path)
    parser.add_argument('socket', type=Path)
    parser.add_argument('--client', default='mariadb')
    parser.add_argument('--sanitize', action='store_true', help='instrument affected bot code with ASan/UBSan')
    args = parser.parse_args()
    client = [args.client, '--no-defaults', '--socket=' + str(args.socket.resolve()), '-uroot']

    def sql(query):
        subprocess.run(client, input=query, text=True, capture_output=True, check=True)

    tag = 'pb_runtime_' + uuid.uuid4().hex[:10]
    world, chars = tag + '_world', tag + '_characters'
    created, user_created = [], False
    with tempfile.TemporaryDirectory(prefix='playerbots-runtime-') as temp:
        temp = Path(temp)
        executable = temp / 'runtime-test'
        source = compile_fixture(args.build.resolve(), executable, args.sanitize)
        try:
            sql(f"CREATE USER '{tag}'@'localhost' IDENTIFIED BY 'fixture'")
            user_created = True
            for db in [world, chars]:
                sql(f'CREATE DATABASE `{db}`')
                created.append(db)
                sql(f"GRANT ALL ON `{db}`.* TO '{tag}'@'localhost'")
            prepare_sql(source / 'src/game/AutonomousBots', temp / 'sql')
            sql(f'USE `{chars}`;\n' + (source / 'sql/characters.sql').read_text()
                + (temp / 'sql/characters.sql').read_text())
            env = dict(os.environ, MYSQL_UNIX_PORT=str(args.socket.resolve()))
            env.pop('PLAYERBOTS_CONFIG_DIR', None)
            if args.sanitize:
                # Native process-global service shutdown is not exercised by this
                # fixture; this run checks invalid accesses, not leak accounting.
                env['ASAN_OPTIONS'] = 'detect_leaks=0:abort_on_error=1'
                env['UBSAN_OPTIONS'] = 'halt_on_error=1:print_stacktrace=1'
            # Separate processes use the same binary and persisted database.
            for mode in ['cold', 'warm', 'warm']:
                subprocess.run([str(executable), world, chars, tag, mode], cwd=temp,
                               env=env, timeout=90, check=True)
            print('Linked runtime tests passed: cold start, two restarts, cache bounds, area names, missing config, mail cleanup')
        finally:
            for db in created:
                sql(f'DROP DATABASE `{db}`')
            if user_created:
                sql(f"DROP USER '{tag}'@'localhost'")


if __name__ == '__main__':
    main()
