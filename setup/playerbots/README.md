# CMaNGOS Playerbots for vmangos

This opt-in server build integrates the pinned vanilla CMaNGOS Playerbots module into
vmangos, on top of the Skatecraft and Tarnished patches. Bots use ordinary WoW mechanics;
they do not skate or use Tarnished stance. Human clients keep both integrations. No client
rebuild is required for Playerbots.

The port includes autonomous population and progression, class combat strategies, quests,
travel and transports, loot, gathering and professions, equipment, trainers, groups, guilds,
mail, auctions, battlegrounds, dungeon/raid strategies, chat, and the upstream debugging tools.
These are the vanilla features of the pinned source, not the expansion-specific features of
CMaNGOS TBC/WotLK or AzerothCore's `mod-playerbots`.

The server owns persistent socketless bot sessions. AI decisions, packet dispatch, login
selection, auctions, and remote commands run on the world thread after map updates finish.
Navigation uses vmangos's actual maps/vmaps/mmaps, including route queries without a player
object. Native packet handlers still validate bot requests. The native `.bot` commands remain
available; this module uses `.playerbot` and `.rndbot`.

## Build

Prerequisites: Git, Python 3, CMake 3.16+, Ninja, a C++20 compiler, Boost 1.70+ headers,
MariaDB/MySQL development files, OpenSSL, and zlib development files. On Debian/Ubuntu the
additional bot dependency is `libboost-dev`. Use a compiler new enough for C++20.

From the repository root:

```bash
python3 setup/playerbots/prepare.py .setup/playerbots-source
cmake -S .setup/playerbots-source -B .setup/playerbots-build -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$PWD/.setup/playerbots-install" \
  -DCONF_DIR="$PWD/.setup/playerbots-install/etc" \
  -DBUILD_PLAYERBOTS=ON -DCMAKE_EXPORT_COMPILE_COMMANDS=ON \
  -DUSE_SCRIPTS=ON -DUSE_PCH=OFF -DENABLE_CPPTRACE=OFF \
  -DBUILD_EXTRACTORS=OFF -DDEBUG_SYMBOLS=OFF -DBUILD_FOR_HOST_CPU=OFF
cmake --build .setup/playerbots-build --parallel 4
cmake --install .setup/playerbots-build
```

`USE_SCRIPTS=ON` includes vmangos's actual dungeon/raid scripts. The complete upstream
vanilla bot module is compiled and linked into `mangosd`; no failing strategies are excluded.
`BUILD_PLAYERBOTS=OFF` selects the server without the autonomous module. The module uses
a shared precompiled header by default to reduce build time; `PLAYERBOTS_USE_PCH=OFF`
selects ordinary includes for compiler diagnostics. This option is separate from the core
server's `USE_PCH` setting.

`prepare.py` refuses to overwrite an existing destination. It accepts `--core-cache` and
`--bots-cache` pointing to local Git clones while still checking the exact pinned revisions.
It prepares source and first-install SQL; it does not change a running deployment or database.
The ordinary `setup/build-server.sh` image is a separate build and does not automatically
include this opt-in module. Use this `mangosd` for the bot-enabled server.

## Container build for the test server

Build inside the same image used at runtime, rather than copying host-built binaries into
an image with different MySQL/OpenSSL/glibc libraries:

```bash
python3 setup/playerbots/build_image.py --tag world-of-skatecraft/vmangos-playerbots:runtime-fixes
```

The default base is a pinned digest of the Ubuntu 26.04 vmangos-deploy image. To reuse your
existing server image instead, pass `--base-image IMAGE`. Both build and runtime stages use
that same image. Build dependencies are installed as root inside the build stage. `--jobs 4`
controls compilation parallelism; optional `--core-cache`/`--bots-cache` reuse pinned clones.
The script creates source-only build context and does not modify Compose, start containers,
or import/reset any database. Use the resulting image in your existing **test** Compose file
with its copied databases and existing extracted data. Preserve the original deployment.

The image retains vmangos-deploy's wrappers and uses `RelWithDebInfo` and includes unstripped server binaries, gdb,
configuration templates under `/opt/vmangos/config`, and bot SQL under
`/opt/vmangos/share/vmangos/playerbots`. Existing bind mounts can hide the templates, so retain
your already-created configuration files. Existing bot tables do not need reinstallation.
The main server command remains:

```bash
/opt/vmangos/bin/mangosd -c /opt/vmangos/config/mangosd.conf
```

The build verifies DWARF debug sections and GDB source-line lookup for the event loader.
`DEBUG_SYMBOLS=ON` alone is insufficient with the pinned core’s Release configuration.
The image records source pins, patch hashes, compiler version, and binary hashes under
`/opt/vmangos/share/playerbots`. Verify the actual packaged files, rather than relying on a tag:

```bash
docker run --rm --entrypoint sh world-of-skatecraft/vmangos-playerbots:runtime-fixes \
  -c 'cd /opt/vmangos && sha256sum -c share/playerbots/binaries.sha256 && cat share/playerbots/provenance.json'
```

If startup fails again, stop the test service's restart loop and run the same image and mounts
under gdb. This captures function names and source line numbers without host core-dump setup:

```bash
docker compose -f "$TEST/compose.yaml" stop mangosd
docker compose -f "$TEST/compose.yaml" run --rm --no-deps --entrypoint gdb mangosd \
  --batch -ex 'set pagination off' -ex run -ex 'thread apply all bt full' \
  --args /opt/vmangos/bin/mangosd -c /opt/vmangos/config/mangosd.conf \
  > playerbots-gdb.log 2>&1
```

## Database and configuration

Use a separate server instance and copies of your existing vmangos databases for the first
in-world run. The world database must match the pinned vmangos core. Keep your existing
vmangos authentication database and extracted vanilla DBC, maps, vmaps, and mmaps; CMaNGOS
world databases and extracted navigation files are not interchangeable with vmangos's.

Import these **once** into the corresponding vmangos databases, using your existing database
connection settings and actual database names:

```bash
mariadb --database=characters < .setup/playerbots-source/sql/playerbots/characters.sql
mariadb --database=mangos < .setup/playerbots-source/sql/playerbots/world.sql
```

The generated SQL creates only bot-owned tables and includes the vanilla names, text,
equipment weights, enchants, travel nodes, locations, and zone-level seeds. Existing bot
tables cause installation to stop, preserving their contents. Do not use the SQL client's
`--force` option, and do not rerun the initial seed import as an upgrade. Do not bulk-import
the upstream SQL directory: it contains reset statements and schemas for other expansions.
Installed copies of these two scripts are under `share/vmangos/playerbots`.

Copy `aiplayerbot.conf.dist` to `aiplayerbot.conf` alongside the main configuration passed to
`mangosd -c`. Bot configuration lookup uses that directory at runtime, not the build machine's
`CONF_DIR`. Set `PLAYERBOTS_CONFIG_DIR` to override it for both bot configuration files. Missing
configuration errors include the resolved absolute path. Set `AiPlayerbot.Enabled = 1`. Configure `mangosd.conf` with the test database
connections, realm, and `DataDir` pointing to your extracted vmangos data.

For a small initial population, adjust these existing settings in the copied bot config:

```ini
AiPlayerbot.Enabled = 1
AiPlayerbot.RandomBotAutologin = 1
AiPlayerbot.MinRandomBots = 5
AiPlayerbot.MaxRandomBots = 10
AiPlayerbot.RandomBotAccountCount = 2
AiPlayerbot.RandomBotMinLevel = 1
AiPlayerbot.RandomBotMaxLevel = 60
AiPlayerbot.CommandServerPort = 0
```

The upstream distribution defaults to 1,000 bots and 200 generated accounts. Account count
controls provisioning; min/max bots control the online population. Retain the rest of the
distributed settings, then tune population and progression for your machine. First startup
builds item/travel caches and creates bot characters, so it is slower than subsequent starts.

For the module's optional auction population service, copy `playerbots-ahbot.conf.dist` to
`playerbots-ahbot.conf` and configure `AhBot.Enabled`. This is separate from vmangos's own
auction bot and from individual playerbots' buying/selling actions; choose one population
service to avoid two independent services managing the same economy.

Start the installed `mangosd` with your normal vmangos configuration. `.playerbot help`
controls companions; `.playerbot add CharacterName` logs in an eligible existing character.
The random population is controlled with `.rndbot` (GM/console), and `.playerbot-ahbot`
controls the optional auction service. Eligibility still follows account ownership, configured
random/free-alt accounts, guild-bot permission, and the upstream Playerbots security rules.
Logging into a character yourself transfers control from its AI to your session.

If `AiPlayerbot.CommandServerPort` is enabled, the command service listens on **127.0.0.1**.
The upstream line protocol has no authentication; remote administrators can use an SSH port
forward. Connections are nonblocking and commands execute on the world thread.

## Native compatibility contracts

- vmangos `Field::GetString()` returns a nullable borrowed pointer. The module uses
  `GetCppString()` for SQL text so NULL becomes an empty owned string and values survive
  advancing or destroying the query result. In particular, bot events legitimately store
  NULL payloads; their enabled-manager restart path must accept these existing rows.
  This fix requires a server rebuild, not a table reset or data migration.
- SQL formatting does not escape values. Event names/payloads, presets, custom strategies,
  account names, help text and auction categories use the destination database’s native
  escaping. In-memory keys and payloads retain their original contents.
- Channel membership uses the native `Channel` API. No unrelated CMaNGOS-layout class is
  cast over a vmangos object. Navigation reference widths come from native Detour headers.
- GUID and packet fields use vmangos types and native packet decoders. Truncated typed
  bot packets are rejected at the adapter boundary instead of escaping into the world loop.
- Every GNU/Clang bot build treats printf-format mismatches as errors, including SQL and
  logging calls. This checks actual native member types rather than assuming CMaNGOS types.

## Other deployment fixes

- AH proposition cleanup uses native `mail.receiver_guid`; pet queries use `owner_guid`.
  All login-selection query variants use `characters.played_time_total` and remain explicit
  SQL literals so the schema test covers their conditions.
- A missing auction configuration leaves the service disabled with initialized settings.
  Both bot configuration files resolve beside the selected `mangosd.conf`, with an explicit
  directory override available.
- Teleport caches validate map, level and coordinates, clear previous in-memory contents,
  check parallel indexes and use numeric zone keys. Area lookup uses the native terrain
  loader and tolerates missing entries and cyclic zone parents. Startup logs identify each
  cache enhancement phase.
- Travel, taxi and auction logs pass native string members as character pointers,
  avoiding undefined behavior in variadic logging. The format regression check compiles
  every bot translation unit against the configured native headers with format errors fatal.
- The build links the platform dynamic-loader library through the existing CMake signature.
  The container recipe builds against its runtime libraries and retains debugging symbols.
- The Rust animation-loader transformation accepts Windows CRLF checkouts and fails early
  if the pinned source no longer matches its required transformations.

The report's riding-skill, cooldown and guild-leader warnings are not established crash
causes. Native validation for those records remains in effect.

## Validation

Asset-free checks use the configured build's real compiler and native DBC loader:

```bash
python3 setup/playerbots/test_dbc.py .setup/playerbots-build
python3 setup/playerbots/test_config.py .setup/playerbots-build
python3 setup/playerbots/test_contracts.py .setup/playerbots-source
# Optional standalone format audit; normal bot builds already enforce format errors:
python3 setup/playerbots/test_formats.py .setup/playerbots-build
PLAYERBOTS_PREPARED_SOURCE="$PWD/.setup/playerbots-source" \
PLAYERBOTS_UPSTREAM_SOURCE=/path/to/pinned/playerbots/clone \
  python3 -m unittest discover -s setup/playerbots -p test_prepare.py
python3 setup/playerbots/test_sql.py .setup/playerbots-source /path/to/test-mariadb.sock
python3 setup/playerbots/test_database.py .setup/playerbots-build /path/to/test-mariadb.sock
python3 setup/playerbots/test_runtime.py .setup/playerbots-build /path/to/test-mariadb.sock
```

The SQL test requires a disposable MariaDB instance accessed through that explicit Unix
socket. It creates uniquely named test databases, imports the actual native character and login schemas
and bot seeds, checks translated queries and repeated-import preservation, then removes only
its own databases. The DBC test covers localized WMO names, model collision dimensions, and
emote sounds. The configuration test checks relocation, isolation, environment overrides, and failed reloads.
The SQL test preprocesses the vanilla source and validates active SQL literals against the native
character and login schemas, plus bot-owned world tables from the installed seeds.
The full native world database is not bundled and is not synthesized by that test.
The source contract check rejects borrowed SQL string reads,
foreign object-layout casts and module overrides of the native Detour reference ABI.
The linked runtime test starts an enabled manager, saves NULL-payload events, and restarts
twice against the same database. It also checks nullable translations, owned text after
result destruction, signed/unsigned integer limits, quoted/Unicode persistence, native GUID
serialization and truncated packet rejection, locale indexes and channel membership.
Existing mail cleanup, missing auction config, area lookup, invalid cache rows, cache alignment
and repeated cache initialization tests run in those same processes. These tests require
CREATE USER and CREATE DATABASE privileges on the disposable database instance. They contain
no extracted game assets. `test_database.py` runs the same native NULL/ownership/integer-boundary
checks, SQL escaping and prepared-statement bindings against the configured client library.
It needs only the `shared`, `framework` and `g3dlite` build targets. This permits a separate
MySQL/MariaDB library check without rebuilding the whole world server. Add `--sanitize` to `test_runtime.py`
to instrument the affected bot translation units with ASan/UBSan. The rest of the core
remains uninstrumented. Alignment checks are excluded because the pinned native core
uses packed database structures, including string members; leak checking is excluded
because the fixture does not perform full service shutdown.

A successful build and these checks do not establish working in-world behavior. Live testing
requires the actual world database and extracted game data. Exercise creation/login, autonomous
travel and quests, combat/healing/pets, loot/mail/auctions, groups and battlegrounds, logout and
restart persistence, and human takeover of a bot character. Also verify existing native party
bots and human Skatecraft/Tarnished gameplay. The NULL event-payload crash in the supplied `GetEventValue` backtrace was reproduced by
the enabled-manager restart fixture against the unfixed code. This reproduces that failure
without the production database; it does not reproduce the entire production deployment.
A repeated in-world startup/shutdown test remains required before treating this as production-ready.

## Sources and licensing

`sources.json` pins vmangos, `cmangos/playerbots`, and the CMaNGOS Classic hook reference.
`core.patch` adapts vmangos; `module.patch` translates the complete upstream module;
`vmangos.cmake` wires its build and installation. `includes.json` records the initial header
mapping. Upstream sources, SQL, and copyright notices are retained. Server-side code keeps
its upstream GPL licensing, separate from the Rust client's license. No game assets are bundled.
