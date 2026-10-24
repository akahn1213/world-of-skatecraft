#!/usr/bin/env python3
"""Exercise separate compile containers and image builds through the production runner.
Default: small C++/PCH fixture. --source: an already prepared full server tree.
Owned test volumes/images are removed; existing builder/toolchain caches are retained.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid

from build_image import BASE_IMAGE, build_prepared, managed_builder, toolchain_image
import prepare


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-image', default=BASE_IMAGE)
    parser.add_argument('--jobs', type=int, default=1)
    parser.add_argument('--source', type=Path, help='use a full prepared source instead of the fixture')
    args = parser.parse_args()
    args.memory, args.cpus, args.debug_profile = '4g', 2, 'lines'
    args.tag = 'playerbots-build-regression:' + uuid.uuid4().hex[:12]
    volume = 'playerbots-test-objects-' + uuid.uuid4().hex[:12]
    builder = managed_builder(args.memory, args.cpus)
    toolchain = toolchain_image(builder, args.base_image)
    subprocess.run(['docker', 'volume', 'create', volume], check=True)
    try:
        with tempfile.TemporaryDirectory(prefix='playerbots-build-test-') as temp:
            root = Path(temp)
            source = root / 'source'
            if args.source:
                shutil.copytree(args.source, source)
                shutil.rmtree(source / 'src/game/AutonomousBots/.git', ignore_errors=True)
                changed = Path('src/game/Objects/Player.cpp')
            else:
                source.mkdir()
                (source / 'CMakeLists.txt').write_text('''cmake_minimum_required(VERSION 3.16)
project(CacheRegression LANGUAGES C CXX)
file(MAKE_DIRECTORY "${CMAKE_SOURCE_DIR}/src/shared")
file(WRITE "${CMAKE_SOURCE_DIR}/src/shared/migrations_list.h" "#define MIGRATION_VERSION 1\\n")
configure_file(revision.in "${CMAKE_SOURCE_DIR}/src/shared/revision.h")
add_executable(mangosd Player.cpp Shared.cpp)
target_precompile_headers(mangosd PRIVATE "src/shared/migrations_list.h" "src/shared/revision.h")
add_executable(realmd realm.cpp)
install(TARGETS mangosd realmd RUNTIME DESTINATION bin)
''')
                (source / 'revision.in').write_text('#pragma once\n#define REVISION 1\n')
                # Runtime symbol validation intentionally looks for this source filename.
                (source / 'Player.cpp').write_text('''#line 1 "RandomPlayerbotMgr.cpp"
class RandomPlayerbotMgr { public: __attribute__((noinline)) int GetEventValue(); };
int RandomPlayerbotMgr::GetEventValue() { return 0; }
int main() { return RandomPlayerbotMgr().GetEventValue(); }
''')
                (source / 'Shared.cpp').write_text('int untouched() { return 7; }\n')
                (source / 'realm.cpp').write_text('int main() { return 0; }\n')
                changed = Path('Player.cpp')

            def build(label, script=None):
                # Redirect actual subprocess output, not only Python's stdout.
                log = Path(temp).parent / ('playerbots-persistent-' + volume.replace('playerbots-test-objects-', '', 1) + '-' + label + '.log')
                import os
                with log.open('w') as output:
                    sysout, syserr = os.dup(1), os.dup(2)
                    try:
                        os.dup2(output.fileno(), 1); os.dup2(output.fileno(), 2)
                        build_prepared(source, {'revision': label}, args, builder, toolchain, volume, script)
                    finally:
                        os.dup2(sysout, 1); os.dup2(syserr, 2)
                        os.close(sysout); os.close(syserr)
                result = log.read_text()
                print(label + ': ' + str(log), flush=True)
                return result

            cold = build('cold')
            assert 'Building CXX object' in cold
            # Different host paths, new checkout mtimes, same persistent volume.
            fresh = root / 'fresh-checkout'
            if args.source:
                prepare.prepare(fresh, args.source.resolve(), args.source.resolve() / 'src/game/AutonomousBots')
                shutil.rmtree(fresh / 'src/game/AutonomousBots/.git')
            else:
                shutil.copytree(source, fresh, copy_function=shutil.copyfile)
            source = fresh
            noop = build('fresh-checkout')
            assert 'ninja: no work to do.' in noop, noop[-3000:]
            assert 'Building CXX object' not in noop
            assert 'Reusing persistent compiler outputs' in noop
            (source / changed).write_text((source / changed).read_text() + '\nint playerbots_incremental_probe() { return 7; }\n')
            edited = build('one-file')
            compiled = [line for line in edited.splitlines() if 'Building CXX object' in line]
            assert len(compiled) == 1 and 'Player.cpp.o' in compiled[0], compiled
            script = root / 'build_cached.sh'
            script.write_text((prepare.HERE / 'build_cached.sh').read_text() + '\n# diagnostics-only edit\n')
            args.cpus = 1
            cosmetic = build('script-comment', script)
            assert 'ninja: no work to do.' in cosmetic and 'Building CXX object' not in cosmetic
            revision = subprocess.check_output(['docker', 'run', '--rm', '--entrypoint', 'cat', args.tag,
                        '/opt/vmangos/share/playerbots/provenance.json'], text=True)
            assert json.loads(revision)['revision'] == 'script-comment'
            print('Passed: separate containers, fresh checkout no-op, one-file edit, script/resource changes, image metadata, runtime linkage and GDB source lines.')
    finally:
        subprocess.run(['docker', 'volume', 'rm', volume], stdout=subprocess.DEVNULL, check=False)
        subprocess.run(['docker', 'image', 'rm', args.tag], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)


if __name__ == '__main__':
    main()
