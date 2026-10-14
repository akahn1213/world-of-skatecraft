#!/usr/bin/env python3
"""Exercise native database types/lifetimes with the configured MySQL client library.

Requires the shared, framework and g3dlite build targets, not game assets or a full
world build. Uses a uniquely owned disposable database and local database user.
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('build', type=Path)
    parser.add_argument('socket', type=Path)
    parser.add_argument('--client', default='mariadb')
    args = parser.parse_args()
    build = args.build.resolve()
    commands = json.loads((build / 'compile_commands.json').read_text())
    entry = next(c for c in commands if c['file'].endswith('/Database/DatabaseMysql.cpp'))
    source = Path(entry['file']).parents[3]
    command = shlex.split(entry['command'])
    flags, skip = [], False
    for arg in command[1:]:
        if skip:
            skip = False
        elif arg in ('-o', '-MF', '-MT', '-MQ', '-include'):
            skip = True
        elif arg not in ('-c', '-MD', '-MMD', '-DNDEBUG', entry['file']) and not arg.startswith('-O'):
            flags.append(arg)
    mysql = re.search(r'^MYSQL_LIBRARY:[^=]+=(.+)$', (build / 'CMakeCache.txt').read_text(), re.M)[1]
    client = [args.client, '--no-defaults', '--socket=' + str(args.socket.resolve()), '-uroot']

    def sql(text):
        subprocess.run(client, input=text, text=True, capture_output=True, check=True)

    tag = 'pb_contract_' + uuid.uuid4().hex[:10]
    database_created = user_created = False
    with tempfile.TemporaryDirectory(prefix='playerbots-database-') as temp:
        temp = Path(temp)
        test = temp / 'database.cpp'
        test.write_text(r'''
#include "database_contracts.h"
DatabaseType WorldDatabase, CharacterDatabase, LoginDatabase, LogsDatabase;
uint32 realmID = 1;
std::string realmName = "fixture";
char const* g_mainLogFileName = "fixture.log";
int main(int argc, char** argv)
{
    assert(argc == 2);
    assert(CharacterDatabase.Initialize((std::string(".;0;") + argv[1] + ";fixture;" + argv[1]).c_str()));
    CheckNativeDatabaseContracts();
    CharacterDatabase.HaltDelayThread();
}
''')
        executable = temp / 'database-test'
        subprocess.run([command[0], *flags, '-O0', '-std=c++20', '-Werror=format',
                        '-I' + str(source / 'src'), '-I' + str(source / 'src/game/AutonomousBots'),
                        '-I' + str(Path(__file__).resolve().parent / 'tests'), str(test), '-o', str(executable),
                        '-Wl,--start-group', 'src/shared/libshared.a', 'src/framework/libframework.a',
                        'dep/g3dlite/libg3dlite.a', '-Wl,--end-group', mysql,
                        '-lssl', '-lcrypto', '-lz', '-ldl', '-pthread'], cwd=build, check=True)
        try:
            sql(f'CREATE DATABASE `{tag}`')
            database_created = True
            sql(f"CREATE USER '{tag}'@'localhost' IDENTIFIED BY 'fixture'")
            user_created = True
            sql(f"GRANT ALL ON `{tag}`.* TO '{tag}'@'localhost'")
            env = dict(os.environ, MYSQL_UNIX_PORT=str(args.socket.resolve()))
            subprocess.run([str(executable), tag], cwd=temp, env=env, timeout=60, check=True)
            print('Native database contracts passed: NULL, ownership, integer limits, escaping, prepared-statement bindings; library: ' + mysql)
        finally:
            if database_created:
                sql(f'DROP DATABASE `{tag}`')
            if user_created:
                sql(f"DROP USER '{tag}'@'localhost'")


if __name__ == '__main__':
    main()
