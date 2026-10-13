# Installing World of Skatecraft

`setup/setup.sh` (Linux) or `setup.bat` (Windows, experimental) does all of this for you. This page explains what it does and what it asks, so
you know what to expect. If a step fails, setup stops, says what to fix, and continues from that
step when you run it again.

## 1. Before you start

### Tarnished action table

This is an optional experimental development setup. Supply your own
ELDEN RING install and follow the complete
[README setup and supervised testing guide](README.md#set-up-and-test-tarnished-combat).
Unpack copies of its containers with UXM/WitchyBND into a separate folder; never patch
or write into the original install.
Set `ER_FILES` to that folder (`chr/` and `regulation-bin/*.param`) and `ER_GAME_DIR` to
the game's `Game` folder containing `oo2core_6_win64.dll`. The pinned extractor needs
`c0000.anibnd.dcx` and the `c0000_a00_hi`, `a00_lo`, `a00_md`, `a0x`, `a1x`,
`a2x`, `a3x`, `a4x`, `a5x` animation packs (`.anibnd.dcx`).
Use 64-bit Python 3.10+. On Linux/macOS also supply Wine and set `WINE_PYTHON` to an
existing 64-bit Windows `python.exe`; setup
does not install either.

From the repository root:

```sh
python3 setup/tarnished.py
cargo test -p benilla-tarnished --features er-data
cargo build --release -p benilla --features er-data
setup/build-server.sh
docker compose -p world-of-skatecraft -f server/compose.yaml up -d --force-recreate
./play.sh
```

The script downloads only the extraction tools from the vendor's pinned revision and writes
tools, scratch, the generated table and `assets/player_anims.bin` beneath ignored
`.setup/tarnished/`, outside the input
folders. It reads the install, never writes to it. `ER_EXTRACTED` can instead name an existing
generated Rust action table; only use a trusted local table because Cargo compiles it as code.
`ER_ANIMATIONS` can name an existing baked `player_anims.bin` at runtime. With a custom
`ER_EXTRACTED`, set `ER_ANIMATIONS` to the matching bake from the same extraction.
`ER_EXTRACTED` is compiled into the launcher, not read by the play script; changing it requires
rebuilding the launcher. Merely changing the runtime animation path cannot change combat timings.
Both outputs are required: the client refuses the stance rather than substitute stock action animations when the animation
file is missing. Keep the baked file on disk beside your development checkout or set its absolute
path for a relocated player build. Nothing generated may be committed. Sound baking is not
included. No live combat run is claimed.

Alternatively, set `WOW_TARNISHED=1` together with `ER_FILES` and `ER_GAME_DIR` before running
normal setup: it extracts both inputs if needed and builds the release launcher with `er-data`. Normal
setup reuses already-prepared inputs, including on Windows, but enables `er-data` only with
this explicit flag; leftover extraction files cannot silently change an ordinary build.
`play.sh` / `play.bat` select the baked animation file; they run the release binary
that setup built, without rebuilding, and force `WOW_HOST=localhost`. For a remote server,
launch the release binary directly as described in the README. Upstream is a binary-only
Bevy 0.19 sandbox, so the bridge vendors exact
pinned sources and isolates its math rather than adding an invalid Cargo git library dependency
or upgrading the WoW renderer.
The vmangos image build applies `skatecraft.patch` and then `tarnished.patch`; a stock server
receives only the safe addon discovery message and never an unnegotiated Tarnished opcode.
See [README.md](README.md#tarnished-combat-experimental) for controller and stance controls.
Combat verification must be supervised with GM mode off; do not run an unattended combat probe.
Use ordinary (`--gm 0`) separate accounts. The server build command above requires Linux/Bash;
Windows setup's stock server cannot enable Tarnished combat.

For a supervised combat timing check, set `WOW_MOVE_TRACE` to a log path and
`WOW_MOVE_TRACE_TAGS=tarnished,fct` before starting the client. The `tarnished` contact
line records the attack's current animation frame, extracted active window (`from`/`to`),
and total duration, in 30-fps frames. The `fct` receipt line records the server-confirmed
damage; Tarnished feedback displays at that receipt without waiting for a stock WoW swing.
Tap R1 and R2 separately, then repeat while taking damage: neither attack should require
holding the button through recovery, and ordinary damage should not interrupt it.
Also check R3 with no selection, a friendly selection, and a valid enemy selection; press
again to unlock. Enter the stance with weapons stowed and try the sheath toggle while idle.
After rebuilding the patched server, check both red hostile and yellow neutral creatures,
with friendly and out-of-range targets as rejection controls.

After updating this gameplay integration, rebuild **both** the client and patched server and
relog to learn **Tarnished Stance**. Check activation from the spellbook/action bar and removal
by recasting or cancelling the aura. Test a caster with a staff, a rogue with mixed/paired
weapons, a two-hander, and an unarmed character; change equipment while in stance.
Verify Sinister Strike/Eviscerate are refused, while Heroic Strike/Cleave consume their queue
and power once at contact. Cast Judgement/Hammer of Justice and a timed spell/channel: the
upper body uses WoW animation, walking preserves casts at 60% speed, and attack/block/roll/jump
input cancels timed casts or channels immediately. The next swing resumes
ER animation. Check bow/gun/wand abilities normally, then switch to melee input.

You need:

| | Where it comes from |
|---|---|
| WoW 1.12.1, English, build 5875 | Your own copy. The folder with `WoW.exe` and `Data/` |
| Skate 3 (Xbox 360), extracted | Your own disc or digital copy, extracted so you have `default.xex` and `data/` side by side. For a disc image use [extract-xiso](https://github.com/XboxDev/extract-xiso): `extract-xiso -x "Skate 3.iso"` |
| An Xbox-style controller | Xbox pads on the `xpad` driver work best; Bluetooth and PlayStation pads work too |
| Docker with the compose plugin | Your distribution's packages |
| ~20 GB free | WoW, the server's map data, the build |

Then run `setup/setup.sh --doctor`. It checks everything below and prints the exact install
command for your distribution if something is missing:

- Rust (via [rustup](https://rustup.rs)), a C compiler, pkg-config, ALSA and libudev development
  files, git, curl, Python 3 with venv.
- Docker installed, running, and usable without `sudo` (you are in the `docker` group; log out and
  back in after adding yourself).

On NixOS the scripts enter the repo's `shell.nix` by themselves; you only need Docker enabled in
your system configuration.

## 2. What setup does

1. **Checks your machine** (the doctor above).
2. **Links your WoW client.** It asks for your WoW folder and checks it has the 1.12.1 data
   files. The game reads it directly; nothing in it is changed.
3. **Converts Skate 3 data.** It asks for your extracted Skate 3 folder, downloads the Skate 3
   Rust Engine's converter at a fixed version, and writes the skater, animations and physics
   settings to `skate-data/`. This takes a few minutes.
4. **Converts Skate 3 sounds** into `skate-audio/` (rolling, pops, landings, grinds). It
   downloads vgmstream to decode them. If this fails, skating still works, just silently.
5. **Sets up the local server** ([vmangos](https://github.com/vmangos/core) through
   [vmangos-deploy](https://github.com/mserajnik/vmangos-deploy)) in `server/`, and extracts
   map data from your WoW client. **The extraction takes several hours** and uses a couple of CPU
   cores; leave it running. It only happens once.
6. **Creates your game account.** It asks for a name and a password. The account is a GM.
7. **Builds the game.** The first build takes several minutes. Cargo downloads the skate engine
   from its authors' repository during this step.

Afterwards, start the game with `./play.sh`.

## 3. Where things are

| Folder | What | Safe to delete? |
|---|---|---|
| `WoW` | a link to your WoW folder | yes, setup asks again |
| `skate-data/`, `skate-audio/` | converted from your Skate 3 | yes, setup converts again |
| `server/` | the server, its database (your characters) and map data | only if you want to start over: characters are lost and the maps re-extract for hours |
| `benilla-config/` | game settings, keybindings, addons (`benilla-config/AddOns/`) | yes, settings reset |
| `.setup/`, `setup.log` | setup's tools and log | yes |

## 4. Everyday use

- Play: `./play.sh` (starts the server if it isn't running).
- Stop the server: `docker compose -p world-of-skatecraft -f server/compose.yaml down`.
- Update: `git pull`, then `setup/setup.sh` again (it only rebuilds).
- New account or new password: `python3 tools/make_account.py <name> <password>`.

### Multiplayer skating verification

Use the same client revision on both machines, each with its own converted Skate 3 data, and
a server built with this checkout's `setup/vmangos/skatecraft.patch`. On Linux, rebuild with
`setup/build-server.sh`, then restart with
`docker compose -p world-of-skatecraft -f server/compose.yaml up -d`.
The image tag includes the patch hash, so a changed patch selects a new image.

Both clients must connect to the same server. `WOW_HOST` names the authentication server; its
advertised realm address must also be reachable from each machine. Do not expose an unsecured
development server to the public internet.

Use two distinct accounts and characters: sharing an account can disconnect the other player.
For unattended probes, each checkout must declare its own test identity, set `WOW_UNATTENDED=1`
and `WOW_NOSOUND=1`, and obey `docs/METHOD.md`. Never use a player's active account.

Verify these cases with one skater and one observer, then exchange roles:

- Summon, stand still, push, turn, dismiss; the observer's board and pose follow the correct
  character, and ordinary walking resumes after dismissal.
- Jump, grind, flip and bail; root height and board-to-feet alignment follow the sender rather
  than WoW's walking speed or the observer's terrain collision.
- Enter visibility while the other character is already skating; the observer receives the
  current pose without requiring another summon.
- Teleport, change maps, log out and reconnect; no old board or pose survives the transition.
- Interrupt updates; remote skating expires rather than extrapolating indefinitely.
- Observe with an older client, and connect a new client to an unpatched server; ordinary WoW
  movement remains usable without extension packets being sent to unsupported observers.
- Repeat with different races, gear and non-GM accounts. The default setup creates a GM, which
  is not evidence that skating passes normal movement-anticheat checks.

The relay does not bypass movement anticheat. A server configured to reject excessive speed
or airborne acceleration may still reject ordinary skating movement, affecting world position
and who can see the skater. Turning off the visible GM flag is not the same as using an
unprivileged account. Check the deployed server configuration and test an ordinary account
before treating multiplayer movement as verified.

Measure snapshot age, packet bytes, root position error and board-to-feet offset rather than
judging timing from a video alone. Visual replication is approximate and does not promise
identical collisions, remote audio or authoritative trick results.

## 5. Windows (experimental)

The Windows setup does the same steps as the Linux one, but nobody has run it on a real Windows
PC yet. If you try it, please post how it went in Discussions (see the README).

1. Install the tools. In a terminal (PowerShell or Command Prompt):

   ```bat
   winget install -e --id Git.Git
   winget install -e --id Rustlang.Rustup
   winget install -e --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   winget install -e --id Python.Python.3.12
   winget install -e --id Docker.DockerDesktop
   ```

   Restart Windows afterwards, then start Docker Desktop and let it finish starting (it uses
   WSL 2; it will ask to set that up the first time).
2. Clone to a short path (long paths can break the build) and run setup:

   ```bat
   git clone https://github.com/Kimmo3223/world-of-skatecraft.git C:\skatecraft
   cd C:\skatecraft
   setup.bat
   ```

3. Start the game with `play.bat`.

The folders are the same as on Linux, with `WoW` as a junction (a folder link) instead of a
symlink. Stop the server with `docker compose -p world-of-skatecraft -f server\compose.yaml down`.
