#!/usr/bin/env python3
"""Exercise the real Dockerfile/cache script with a small C++ project, not game assets.
Requires Docker Buildx; does not build or deploy the game server.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid

from build_image import BASE_IMAGE, managed_builder


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-image', default=BASE_IMAGE)
    parser.add_argument('--incremental-only', action='store_true',
                        help='run only the compiler-cache regression stage')
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    builder = managed_builder('4g', 2)
    limits = json.loads(subprocess.check_output(['docker', 'inspect', 'buildx_buildkit_' + builder + '0']))[0]['HostConfig']
    assert limits['Memory'] == limits['MemorySwap'] == 4 * 1024**3
    assert limits['CpuQuota'] == 200000 and limits['CpuPeriod'] == 100000
    tag = 'playerbots-build-regression:' + uuid.uuid4().hex[:12]
    try:
        with tempfile.TemporaryDirectory(prefix='playerbots-build-test-') as temp:
            root = Path(temp)
            source = root / 'source'
            source.mkdir()
            (root / 'Dockerfile').write_text((here / 'Dockerfile').read_text() + '''
FROM build AS regression
COPY regression.sh /build/regression.sh
RUN --mount=type=cache,id=${BUILD_CACHE_ID},target=/build/cache,sharing=locked sh /build/regression.sh
''')
            shutil.copy2(here / 'build_cached.sh', root / 'build_cached.sh')
            (source / 'CMakeLists.txt').write_text('''cmake_minimum_required(VERSION 3.16)
project(CacheRegression LANGUAGES C CXX)
file(MAKE_DIRECTORY "${CMAKE_SOURCE_DIR}/src/shared")
configure_file(revision.in "${CMAKE_SOURCE_DIR}/src/shared/revision.h")
file(GLOB extra_sources "extra/*.cpp")
add_executable(mangosd RandomPlayerbotMgr.cpp ${extra_sources})
target_precompile_headers(mangosd PRIVATE "src/shared/revision.h")
add_executable(realmd realm.cpp)
install(TARGETS mangosd realmd RUNTIME DESTINATION bin)
''')
            (source / 'revision.in').write_text('#pragma once\n#define REVISION 1\n')
            cpp = '''class RandomPlayerbotMgr { public: __attribute__((noinline)) int GetEventValue(); };
int RandomPlayerbotMgr::GetEventValue() { return 0; }
int main() { return RandomPlayerbotMgr().GetEventValue(); }
'''
            (source / 'RandomPlayerbotMgr.cpp').write_text(cpp)
            (source / 'realm.cpp').write_text('int main() { return 0; }\n')
            (source / 'removed.txt').write_text('stale source marker')
            (root / 'provenance.json').write_text('{"revision":1}\n')
            def build(label, target='runtime'):
                cmd = ['docker', 'buildx', 'build', '--builder', builder, '--progress=plain', '--target', target,
                       '--tag', tag, '--build-arg', 'BASE_IMAGE=' + args.base_image,
                       '--build-arg', 'BUILD_CACHE_ID=' + tag.replace(':', '-'), str(root)]
                log = Path(temp).parent / ('playerbots-build-' + label + '.log')
                with log.open('w') as output:
                    export = ['--output', 'type=cacheonly'] if target == 'regression' else ['--load']
                    result = subprocess.run(cmd[:-1] + export + cmd[-1:],
                                            text=True, stdout=output, stderr=subprocess.STDOUT)
                assert result.returncode == 0, f'{label}: see {log}'
                return log.read_text()
            (root / 'regression.sh').write_text(r"""set -eu
cd /build/source
# Cache mounts may be evicted between separate Docker builds. Warm within this
# single locked RUN so this test measures reuse, not Docker's GC policy.
sh /build/build_cached.sh > /tmp/warm.log 2>&1 || { cat /tmp/warm.log; exit 1; }
# Same source contents with new mtimes, plus deletion of a non-code input.
find . -type f -exec touch {} +
rm removed.txt
sh /build/build_cached.sh > /tmp/noop.log 2>&1 || { cat /tmp/noop.log; exit 1; }
cat /tmp/noop.log
grep -q 'ninja: no work to do.' /tmp/noop.log
! grep -q 'Building CXX object' /tmp/noop.log
cp RandomPlayerbotMgr.cpp /tmp/original.cpp
sed -i 's/return 0;/return 0 + 0;/' RandomPlayerbotMgr.cpp
mkdir extra
printf 'int added_function() { return 7; }\n' > extra/added.cpp
sh /build/build_cached.sh > /tmp/incremental.log 2>&1 || { cat /tmp/incremental.log; exit 1; }
cat /tmp/incremental.log
grep -q 'Building CXX object CMakeFiles/mangosd.dir/RandomPlayerbotMgr.cpp.o' /tmp/incremental.log
grep -q 'Building CXX object CMakeFiles/mangosd.dir/extra/added.cpp.o' /tmp/incremental.log
! grep -q 'Building CXX object CMakeFiles/realmd.dir/realm.cpp.o' /tmp/incremental.log
printf 'deliberate compile error\n' > RandomPlayerbotMgr.cpp
if sh /build/build_cached.sh > /tmp/failed.log 2>&1; then exit 1; fi
cp /tmp/original.cpp RandomPlayerbotMgr.cpp
sh /build/build_cached.sh
""")
            if not args.incremental_only:
                cold = build('cold')
                assert 'Building CXX object' in cold
                (root / 'provenance.json').write_text('{"revision":2}\n')
                metadata = build('metadata')
                assert 'Building CXX object' not in metadata
                assert 'Maximum resident set size' not in metadata # Compilation RUN was cached.
                revision = subprocess.check_output(['docker', 'run', '--rm', '--entrypoint', 'cat', tag,
                            '/opt/vmangos/share/playerbots/provenance.json'], text=True)
                assert json.loads(revision)['revision'] == 2
            build('incremental', target='regression')
            print('Passed: ' + ('enforced builder limits and compiler-cache checks; ' if args.incremental_only else
                  'enforced builder limits, cold build, metadata-only cache; ') + 'fresh-checkout no-op, '
                  'incremental compilation and newly added sources, failed-build recovery.' +
                  ('' if args.incremental_only else ' Runtime linkage and GDB source lines verified.'))
    finally:
        subprocess.run(['docker', 'image', 'rm', tag], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)


if __name__ == '__main__':
    main()
