# CMaNGOS Playerbots port to vmangos — experimental

**This is an incomplete source port, not an installable bot feature.** The full module
currently fails compilation. The existing server builder and deployment are unchanged;
this directory prepares an isolated development tree only. Do not import the upstream SQL
into an existing vmangos database or deploy this tree as a working Playerbots server.

The intended scope is the complete CMaNGOS Playerbots module for vanilla: autonomous
population, questing, travel, progression, combat, groups, and the module's supporting systems.
Bots will use ordinary WoW mechanics. Skatecraft and Tarnished remain available to human
clients through the existing patches, which are applied before this port.

## Sources and adaptation

`sources.json` pins vmangos, the complete `cmangos/playerbots` source, and the CMaNGOS
Classic reference used to identify integration hooks. No moving branch is used to prepare a
build. The upstream bot source, including its SQL/configuration and original copyright
notices, is fetched intact. The server-side code retains its upstream GPL licensing; this
is separate from the Rust client's licensing. No extracted game assets are included.

- `core.patch`: initial vmangos compatibility changes and an opt-in compilation target.
- `module.patch`: explicit header/API translations against the pinned module.
- `includes.json`: the header mapping used during the initial adaptation.
- `vmangos.cmake`: all upstream bot and auction-bot translation units, with vanilla selection.
  No failing strategies are omitted. Objects are **not linked into the runnable server yet**.
- `prepare.py`: fetch/check pins, apply both existing gameplay patches, then apply this port.
  It refuses existing destinations and publishes the prepared tree only after all patches apply.
- `probe.py`: compile the full module or eight important integration units, retaining failures.

## Development build

Prerequisites: Git, Python 3, CMake 3.16+, Ninja, a C++20 compiler, Boost headers,
MariaDB/MySQL development files, OpenSSL, and zlib development files. Use a fresh path
under `.setup`; this does not modify `server/` or any databases.

```bash
python3 setup/playerbots/prepare.py .setup/playerbots-source
cmake -S .setup/playerbots-source -B .setup/playerbots-build -G Ninja \
  -DBUILD_PLAYERBOTS=ON -DCMAKE_EXPORT_COMPILE_COMMANDS=ON \
  -DUSE_PCH=OFF -DENABLE_CPPTRACE=OFF -DUSE_SCRIPTS=OFF \
  -DDEBUG_SYMBOLS=OFF -DBUILD_FOR_HOST_CPU=OFF
python3 setup/playerbots/probe.py .setup/playerbots-build --critical
# The acceptance build includes the complete module:
python3 setup/playerbots/probe.py .setup/playerbots-build
```

`prepare.py` accepts `--core-cache` and `--bots-cache` pointing to local Git clones.
It still checks out the exact revisions; a missing core revision is fetched from its pinned
upstream repository. For the hook reference, check out the CMaNGOS revision in `sources.json`.

## Remaining integration requirements

The module's headers and server facade have initial adaptations, but compilation is not
complete. In particular, the eight-unit probe exposes differences in these interfaces:

- Bot character creation/login/logout, ownership of server sessions, packet parsing and dispatch.
  vmangos now uses typed incoming packets in places where this module invokes raw-packet handlers.
- Spell cooldowns, auras, threat, movement generators, pathfinding and transport APIs.
- World-state and ScriptDevAI integration, which differ from vmangos's systems.
- Spawn/template records and database queries. vmangos supports multiple creature entries per
  spawn; upstream code assuming one entry needs semantic adaptation, not just field renames.
- Quest, inventory, auction and item classification. CMaNGOS's additional consumable subclasses
  are not populated by vmangos's vanilla item table and need an actual classification adapter.
- Area-name overrides and bounded height queries moved out of inline headers still need vmangos
  implementations; they must not be replaced with dummy successful answers.

The lifecycle hooks found in the reference core also need porting and testing: world startup
and updates, player/session creation and teardown, packet observation, group/chat handling,
map updates, navigation, spell events, and bot database initialization. The new Player fields
are not yet wired into those lifecycles. Merely compiling the module will not spawn bots.

Before enabling deployment, require a successful complete server link, isolated database
migrations, bot login/logout and persistence tests, and live tests of autonomous travel,
questing, combat, loot and group behavior. Verify that existing native party bots and human
Skatecraft/Tarnished gameplay continue to work. No live bot behavior has been validated yet.
