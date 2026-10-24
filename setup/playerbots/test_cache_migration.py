#!/usr/bin/env python3
"""Round-trip real Ninja objects through a uniquely owned legacy BuildKit cache.
Checks PAX timestamp preservation and the production legacy fingerprint conversion.
"""
from pathlib import Path
import subprocess
import tempfile
import uuid
from build_image import BASE_IMAGE, LEGACY_SCRIPT_SHA256, managed_builder, toolchain_image, import_legacy_cache
import prepare


def run(*command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def main():
    builder = managed_builder('4g', 2)
    toolchain = toolchain_image(builder, BASE_IMAGE)
    token = uuid.uuid4().hex[:12]
    old, new, container = ('playerbots-migration-' + token + suffix for suffix in ('-old', '-new', '-export'))
    legacy = 'playerbots-migration-' + token
    for volume in (old, new): run('docker', 'volume', 'create', volume)
    try:
        with tempfile.TemporaryDirectory(prefix='playerbots-migrate-') as temp:
            root = Path(temp)
            source = root / 'source'
            source.mkdir()
            (source / 'CMakeLists.txt').write_text('''cmake_minimum_required(VERSION 3.16)
project(Migration LANGUAGES C CXX)
add_executable(mangosd main.cpp)
add_executable(realmd main.cpp)
install(TARGETS mangosd realmd RUNTIME DESTINATION bin)
''')
            (source / 'main.cpp').write_text('int main() { return 0; }\n')
            def compile(volume):
                return subprocess.check_output(['docker', 'run', '--rm', '--memory', '4g', '--cpus', '2',
                    '--network', 'none', '--entrypoint', 'sh',
                    '--mount', 'type=volume,src=' + volume + ',dst=/build/cache',
                    '--mount', 'type=bind,src=' + str(source) + ',dst=/build/source,readonly',
                    '--mount', 'type=bind,src=' + str(prepare.HERE / 'build_cached.sh') + ',dst=/build/build_cached.sh,readonly',
                    toolchain, '/build/build_cached.sh'], text=True, stderr=subprocess.STDOUT)
            assert 'Building CXX object' in compile(old)
            # Restore the old on-disk key format using the same installed toolchain.
            script = f'''set -eu
(c++ --version; cmake --version; dpkg-query -W; printf '%s  /build/build_cached.sh\\n' '{LEGACY_SCRIPT_SHA256}'; printf '%s\\n' '-O2 -g1 -DNDEBUG') | sha256sum | cut -d' ' -f1 > /build/cache/toolchain
sed -i 's|/[^/]*$||' /build/cache/configuration-key
stat -c '%y' /build/cache/output/CMakeFiles/mangosd.dir/main.cpp.o > /stamp
# Preserve nanoseconds, which Ninja stores in its dependency log.
tar --format=posix --exclude=./build.lock -C /build/cache -cf /legacy.tar .
'''
            run('docker', 'run', '--name', container, '--network', 'none', '--entrypoint', 'sh',
                '--mount', 'type=volume,src=' + old + ',dst=/build/cache', toolchain, '-c', script)
            run('docker', 'cp', container + ':/legacy.tar', str(root / 'legacy.tar'))
            run('docker', 'cp', container + ':/stamp', str(root / 'stamp'))
            (root / 'Dockerfile').write_text(f'''# syntax=docker/dockerfile:1
FROM {BASE_IMAGE}
COPY legacy.tar /legacy.tar
RUN --mount=type=cache,id={legacy},target=/build/cache,sharing=locked tar -xf /legacy.tar -C /build/cache
''')
            run('docker', 'buildx', 'build', '--builder', builder, '--output', 'type=cacheonly', str(root))
            import_legacy_cache(builder, BASE_IMAGE, 'lines', new, toolchain, legacy)
            stamp = subprocess.check_output(['docker', 'run', '--rm', '--entrypoint', 'stat',
                '--mount', 'type=volume,src=' + new + ',dst=/build/cache', toolchain,
                '-c', '%y', '/build/cache/output/CMakeFiles/mangosd.dir/main.cpp.o'], text=True)
            assert stamp == (root / 'stamp').read_text(), (stamp, (root / 'stamp').read_text())
            result = compile(new)
            assert 'Imported legacy objects: toolchain fingerprint matches.' in result, result
            assert 'ninja: no work to do.' in result and 'Building CXX object' not in result, result
            print('Passed: production legacy cache export/import preserves timestamps and performs zero compilation.')
    finally:
        subprocess.run(['docker', 'rm', '-f', container], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for volume in (old, new):
            subprocess.run(['docker', 'volume', 'rm', volume], stdout=subprocess.DEVNULL)


if __name__ == '__main__':
    main()
