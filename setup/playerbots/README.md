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

Copy `aiplayerbot.conf.dist` to `aiplayerbot.conf` in the configured `CONF_DIR`, alongside your
`mangosd.conf`. Set `AiPlayerbot.Enabled = 1`. Configure `mangosd.conf` with the test database
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

## Validation

Asset-free checks use the configured build's real compiler and native DBC loader:

```bash
python3 setup/playerbots/test_dbc.py .setup/playerbots-build
python3 setup/playerbots/test_config.py .setup/playerbots-build
PLAYERBOTS_PREPARED_SOURCE="$PWD/.setup/playerbots-source" \
PLAYERBOTS_UPSTREAM_SOURCE=/path/to/pinned/playerbots/clone \
  python3 -m unittest discover -s setup/playerbots -p test_prepare.py
python3 setup/playerbots/test_sql.py .setup/playerbots-source /path/to/test-mariadb.sock
```

The SQL test requires a disposable MariaDB instance accessed through that explicit Unix
socket. It creates uniquely named test databases, imports the actual native character schema
and bot seeds, checks translated queries and repeated-import preservation, then removes only
its own databases. The DBC test covers localized WMO names, model collision dimensions, and
emote sounds. The configuration test checks isolation, environment overrides, and failed reloads.

A successful build and these checks do not establish working in-world behavior. Live testing
requires the actual world database and extracted game data. Exercise creation/login, autonomous
travel and quests, combat/healing/pets, loot/mail/auctions, groups and battlegrounds, logout and
restart persistence, and human takeover of a bot character. Also verify existing native party
bots and human Skatecraft/Tarnished gameplay. No live gameplay validation has been completed
in this workspace without those inputs.

## Sources and licensing

`sources.json` pins vmangos, `cmangos/playerbots`, and the CMaNGOS Classic hook reference.
`core.patch` adapts vmangos; `module.patch` translates the complete upstream module;
`vmangos.cmake` wires its build and installation. `includes.json` records the initial header
mapping. Upstream sources, SQL, and copyright notices are retained. Server-side code keeps
its upstream GPL licensing, separate from the Rust client's license. No game assets are bundled.
