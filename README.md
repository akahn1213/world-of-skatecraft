# World of Skatecraft

> Test note: this README edit verifies the branch editing workflow.

Skate 3 skateboarding in World of Warcraft 1.12.1. Summon your skateboard like a mount and your
character drops onto it, with Skate 3's own physics: flick-it tricks, grinds on any ledge,
powerslides and bails, anywhere in Azeroth. Your own character does the tricks, in your own gear.
Skateboarding is a profession: land tricks to level it, earn experience and unlock skate abilities
such as Rocket Boost and Moon Jump.

[![Summoning the skateboard in Goldshire](docs/media/skateboarding.webp)](docs/media/skateboarding.mp4)

*Click for the video with sound.*

Built on [benilla](https://github.com/samwhosung/benilla), a from-scratch WoW 1.12.1 client in
Rust, and the [Skate 3 Rust Engine](https://github.com/SK8-ENGINE/skate-3-rust-engine). It runs
fully offline against a local server.

> **Unofficial fan project.** Not affiliated with Blizzard Entertainment, Electronic Arts,
> FromSoftware or Bandai Namco.
> **No game files are included.** You need your own WoW and Skate 3 copies, plus ELDEN RING
> for the optional Tarnished combat mode.

> **Vibe coded, just for laughs.** This whole thing was put together with an AI coding assistant
> (Claude Code) for fun, on top of other people's serious work (see Credits). Expect rough edges,
> weird bugs and a skater who doesn't always know where his sword goes. Don't take it seriously.

## Tarnished combat (experimental)

`benilla-tarnished` contains weapon mapping, swept contacts and defender policy. Its optional
`er-data` host bridge directly drives the upstream simulation, animation selection,
authored crossfades, guard arm layers and orbit camera pinned at
`177a3b5ec14a9bde39b9ab458a3400b4038224e3`. Build the launcher with `--features er-data`
and use the patched vmangos server. Every class learns **Tarnished Stance** in the spellbook
on login. Put it on an action bar: casting it enters the stance, and casting again or cancelling
its aura leaves. F8 and pad Start are shortcuts to the same ability. Enter alive, unmounted,
out of animal form and off the skateboard; skating and ordinary movement cannot own the pad
at the same time. Control changes only after the server approves the ordered stance request;
visual pose updates cannot enable combat. Equipped weapons select the moveset, including
one-handed, two-handed, mixed dual wield, paired weapons and fists. RB attacks, RT charges a
heavy attack, and LB guards with a shield/two-hander or attacks with the off hand (paired
weapons use their paired chain). Tapping B rolls; holding B sprints. **A jumps**, using the
extracted ER jump arc and animations with WoW terrain collision. Keyboard equivalents are
U, I, O and Space. F9 (R3) locks a selected eligible enemy or acquires one using TAB priority;
removal, death, range or occlusion clears the lock. While locked on, flick the **right stick
left/right** to select the nearest angular neighbour on that side. Recenter between switches;
the left stick still moves the character.

**Y interacts** with nearby NPCs or loots a nearby corpse, prioritizing lootable corpses.
It takes all available money and eligible items (including bind-on-pickup items), without
changing mouse auto-loot settings. Group-loot restrictions and full bags still apply. This
also works outside Tarnished stance; chat, loading, targeting a spell and skating suppress it.

The D-pad activates the keyboard number bindings: **Left = 1, Up = 2, Right = 3, Down = 4**.
Put abilities in those action-bar slots to use them from the controller. This works outside
Tarnished stance too and follows your key bindings and current action-bar page. Each press
activates once; chat input, loading screens and an unfocused window suppress activation.

Normal WoW spells and ranged attacks remain available. Direct melee abilities such as
Sinister Strike and Eviscerate are disabled in this stance. Heroic Strike, Cleave and other
next-swing abilities remain queued until the next successful contact request, replacing that
swing with the native spell and its costs, effects and threat. The ER motion value carries into
its weapon damage; additional contacts from that same swing do not duplicate the spell.
Casts and channels allow movement at **60% of normal speed** while in Tarnished stance;
normal speed returns when casting ends. Movement and turning do not interrupt them, while
other interruption rules still apply. **Attacking, blocking, rolling/dodging or jumping cancels
the cast/channel and performs the action.** Queued next-swing modifiers remain queued.
WoW casting and ranged
animations own the upper body while ER drives the lower body. A new melee action can cut an
instant spell's animation tail and stops ranged auto-repeat. Ranged weapons continue to fire
through WoW abilities; R1/R2 use the melee slots or fists. Books/held items provide no off-hand
weapon. Staves use the two-handed Twinblade moveset; fishing poles fall back to fists.

A raised shield or two-handed guard blocks incoming melee hits regardless of WoW class or
shield skill, including a druid holding a staff and attacks with elemental damage components.
The guard still needs stamina and covers the front. Roll iframe avoidance registers as
**Dodge**; a roll outside its extracted iframe window does not protect you.

Attack hitboxes use **0.30 × character height + weapon range × multiplier**, in metres.
The multiplier is **1.5 for Greatsword** (WoW two-handed swords, including Ashkandi) and
**1.1 for other movesets**. Character height comes from the standing model bounds and its live
render scale, with collision height as a fallback while the model loads. A normal human male's
roughly 1.84-metre height adds about 0.55 metres; a 2-metre character adds 0.6 metres.
The source blade samples already include character-relative hand positions; this is an explicit
gameplay allowance. The near blade endpoint stays in place to avoid gaps at shorter ranges;
blade height and root motion are preserved. Actual contacts
still follow the sampled blade path during active hit frames: an extracted endpoint outside
that window does not itself make a target hittable.
During active hit frames, a separate fixed 0.6-metre radius around the player's torso
also catches enemies slightly behind or beside you after a forward lunge. Outside that close
area, hits still require the blade's path. The close area is also bounded vertically.

The attacker owns swept contacts; the defender answers server-correlated hits in iframe,
passive dodge/parry, held front guard, hit order. NPC and stock clients retain native defence.
The generated table supplies timing, stamina and root/blade motion. Baked upstream ER animation
joints are retargeted onto the actual WoW character and gear locally and relayed/interpolated for
remote characters; active actions no longer use stock animation placeholders. Relayed poses
include their reference fitting frames, so observing clients need no ER install. The right
stick or held mouse look drives the upstream camera, with WoW geometry clipping its boom.
Tarnished uses the same native XInput (Windows) / evdev (64-bit Linux) transport mechanism as
Skate 3, not Bevy's disabled gamepad backend. Since the pinned bridge hides raw axes, a minimal
licensed adapter reuses its platform polling code; Skate 3 keeps its native frame pipeline.
Buttons, both sticks and RT always come from the first successfully polled controller.
Movement and camera retain the reference radial deadzones (0.1 and 0.15); positive stick Y
means forward/up, with camera inversion applied once by the reference camera. RT presses at
75% and releases at 65%. Disconnects clear held input, and a neutral/disconnected stick leaves
keyboard movement available. Other platforms retain keyboard/mouse controls.
Stock incoming stamina is normalized onto
the equipped reference armament and the pinned dummy's damage/stamina ratio. This is a fork policy, not an
extracted WoW or ER rule. There has been no supervised live two-client combat verification.

Generate both the action table and baked animations from your own ELDEN RING files with
`python3 setup/tarnished.py`;
follow [Set up and test Tarnished combat](#set-up-and-test-tarnished-combat) below.
No ELDEN RING data is distributed.
The reference is this pinned rewrite, which explicitly marks several tuning constants as
estimates; those are not verified facts about the original ELDEN RING client.

## What you need

- **Linux**, or **Windows (experimental, testers wanted)**: the Windows setup is new and untested
  on real machines; please report how it goes (see Help).
- **World of Warcraft 1.12.1**, English client (build 5875).
- **Skate 3 for Xbox 360, extracted**: the folder with `default.xex` and `data/` (an ISO file
  doesn't work; extract it first).
- **An Xbox-style controller.**
- **Docker**, about **20 GB** free disk, and a few hours once for the server's map extraction
  (plus a few minutes to build the server with the Skateboarding profession).

## Install

Linux:

```sh
git clone https://github.com/Kimmo3223/world-of-skatecraft.git
cd world-of-skatecraft
setup/setup.sh
```

Windows (experimental): install [Git](https://git-scm.com), then in a terminal:

```bat
git clone https://github.com/Kimmo3223/world-of-skatecraft.git C:\skatecraft
cd C:\skatecraft
setup.bat
```

Setup checks your machine and tells you exactly what to install if something is missing, asks
for your WoW and Skate 3 folders, converts what it needs from them, sets up the local server,
creates your game account and builds the game. Step-by-step details: [INSTALL.md](INSTALL.md).

## Play

```sh
./play.sh        # Linux
play.bat         # Windows
```

Log in with the account you made during setup and create a character.

| Key | |
|---|---|
| **J** | summon / dismiss your skateboard |
| **K** | switch between the WoW camera and the Skate 3 camera |
| controller | skate (Skate 3 controls) |
| D-pad | action-bar number bindings: ← 1, ↑ 2, → 3, ↓ 4 (LB + D-pad keeps Skate 3 session markers) |

### Skateboarding, the profession

Every character knows **Summon Skateboard** (spellbook, General tab): it casts like a mount and
puts you on the board until you cancel the buff or press **J** again. Skateboarding is a
secondary skill, 1 to 300, in your Skills tab. Each line of tricks you land raises it (bigger
lines raise it faster) and gives your character experience; the line's score, the experience and
the skill it earned show at the top of the screen, Skate 3 style. As the skill grows you learn skate
abilities, buffs to drag onto your action bar; they work only while you are on the board:

| Skill | Ability | Effect | Lasts | Cooldown |
|---|---|---|---|---|
| 1 | Summon Skateboard | ride | until cancelled | none |
| 75 | Ollie Boost | pop much higher | 30 s | 2 min |
| 150 | Rocket Boost | a burst forward | instant | 30 s |
| 225 | Speed Demon | push harder, roll faster | 30 s | 3 min |
| 300 | Moon Jump | lighter gravity in the air | 20 s | 5 min |

The profession needs the patched server that the Linux setup builds (the Windows setup still runs
the stock server). On another server **J** just hops on and off the board.

### Multiplayer skating

Clients with this fork's skating relay support can display nearby players' character and board
poses on a server built with this fork's patch. Each client still runs only its own skating
physics. The server retains and relays visual snapshots; it does not simulate or judge tricks.
Ordinary WoW movement still supplies the server's character position and visibility.

The extension negotiates support before sending poses. Stock servers and older clients keep
ordinary WoW movement, without replicated boards or skating poses. Each observing client needs
its own converted Skate 3 data. Remote poses are interpolated, not exact copies of every rendered
frame; remote skating audio and shared trick scoring are not part of the visual relay.
`WOW_SKATE_NET_HZ` sets the snapshot rate (default 20, clamped to 1–20 updates per second).

Updating the client alone is not enough: rebuild the patched server and restart it as described
in [INSTALL.md](INSTALL.md#multiplayer-skating-verification). The Windows setup's stock server
does not provide the relay.

Your account is a GM, so chat commands like `.tele stormwind`, `.levelup 59` and
`.additem <id>` work.

## Set up and test Tarnished combat

This repository is standalone: **do not clone or build the upstream ELDEN RING sandbox
separately**. The simulation sources are vendored, and `setup/tarnished.py` downloads its
extraction tools at revision `177a3b5ec14a9bde39b9ab458a3400b4038224e3`.
It does not download game data or unpack the game's encrypted archives.

### 1. Prepare the local installation

Complete the ordinary [Install](#install) steps first, including your own WoW 1.12.1 data,
converted Skate 3 data and the local vmangos database/maps. You also need:

- Your own legally obtained **ELDEN RING PC installation**, including
  `Game/oo2core_6_win64.dll`. Keep all game installs read-only; work on copies outside them.
  Do not upload or commit archives, parameters, DLLs or generated data.
- Rust and the native build prerequisites checked by setup's doctor (C compiler; Linux
  pkg-config, ALSA and udev development files), Docker/Compose, and **64-bit Python 3.10+**.
  The pinned extraction tools use Python's standard library; no pip packages are required.
- [UXM Selective Unpack](https://github.com/Nordgaren/UXM-Selective-Unpack) and
  [WitchyBND](https://github.com/ividyon/WitchyBND), installed with the prerequisites stated
  by their releases. These are external preparation tools, not supplied by setup.
- **Linux extraction:** Wine on `PATH` and a separately supplied **64-bit Windows
  `python.exe`** (for example, an extracted Python embeddable distribution). Native Linux
  Python runs setup; Windows Python under Wine loads the game's Oodle DLL.
  Setup installs neither Wine nor Windows Python.
- **Windows extraction:** native 64-bit Python; Wine and `WINE_PYTHON` are not needed.
  Windows client setup remains experimental, and its stock server is not sufficient for combat.

### 2. Unpack only the required inputs

Use a separate working copy for external unpackers if they write beside their inputs.
In UXM use **Unpack only, never Patch**. Select the following ten files from `chr/`,
retaining their `.anibnd.dcx` containers; the extractor opens those containers itself.
Use WitchyBND to unpack a **copy** of `regulation.bin` into `regulation-bin/`.
Collect the results in the ignored `er-files/` folder at this checkout's root:

```text
er-files/
  chr/
    c0000.anibnd.dcx
    c0000_a00_hi.anibnd.dcx
    c0000_a00_lo.anibnd.dcx
    c0000_a00_md.anibnd.dcx
    c0000_a0x.anibnd.dcx
    c0000_a1x.anibnd.dcx
    c0000_a2x.anibnd.dcx
    c0000_a3x.anibnd.dcx
    c0000_a4x.anibnd.dcx
    c0000_a5x.anibnd.dcx
  regulation-bin/
    AtkParam_Pc.param
    BehaviorParam_PC.param
    CalcCorrectGraph.param
    CharaInitParam.param
    EquipParamWeapon.param
    ... (keep the other unpacked .param files too)
```

Folder names and parameter capitalization matter on Linux. A raw `regulation.bin` alone
is not enough. Keep bundles and parameters from the same game version; compatibility with
every ELDEN RING update is not guaranteed. No sound banks or sound baking are required here.

### 3. Generate the action table and animations

Run from this repository's root, replacing example paths with your own. Unlike upstream's
defaults, **this setup wrapper requires both `ER_FILES` and `ER_GAME_DIR` explicitly**.
`ER_FILES` contains `chr/` and `regulation-bin/`; `ER_GAME_DIR` contains the game's DLL,
not the unpacked parameters.

Linux:

```sh
export ER_FILES="$PWD/er-files"
export ER_GAME_DIR="$HOME/.local/share/Steam/steamapps/common/ELDEN RING/Game"
export WINE_PYTHON="$PWD/.setup/windows-python/python.exe"
python3 setup/tarnished.py
```

Place your extracted Windows Python distribution at the example `WINE_PYTHON` location
first, or use an absolute path to your existing `python.exe`. The script uses its own
Wine prefix at `.setup/tarnished/wine`; do not rely on Python installed only in another
prefix. `WINE_PYTHON` must be an existing file, not just a command name.

Windows PowerShell:

```powershell
$env:ER_FILES = "$PWD\er-files"
$env:ER_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\ELDEN RING\Game"
python setup/tarnished.py
```

Internet access is needed for the pinned tool downloads. The wrapper reads the input
folders and writes only under ignored `.setup/tarnished/`:

| Path | Purpose |
|---|---|
| `tools/` | Downloaded pinned Python tools |
| `scratch/` | Extraction scratch |
| `src/sim/extracted.rs` | Generated action table, required at build time |
| `assets/player_anims.bin` | Baked animations, required at runtime |

Both generated files must be nonempty. `ER_EXTRACTED` optionally overrides the action
table path when building; use an absolute path to a **trusted local** table, since Cargo
compiles it as Rust code. `ER_ANIMATIONS` optionally overrides the baked file at runtime.
Their defaults are the two generated paths above. Use a matching pair from the same
extraction; changing the table requires rebuilding, not merely changing `ER_ANIMATIONS`.
For a relocated binary, set `ER_ANIMATIONS` to the bake's absolute path.

### 4. Build the feature-enabled client and patched server

These Cargo commands work on Linux and Windows after extraction:

```sh
cargo test -p benilla-tarnished --features er-data
cargo build --release -p benilla --features er-data
```

The executable is **`target/release/benilla`** (Windows: `target\release\benilla.exe`).
This is the release profile, not `cargo play`'s play-profile binary. A default build
without `--features er-data` cannot enter Tarnished stance.

On Linux, rebuild the server from this checkout and restart it:

```sh
setup/build-server.sh
docker compose -p world-of-skatecraft -f server/compose.yaml up -d --force-recreate
```

The builder applies `setup/vmangos/skatecraft.patch` followed by `tarnished.patch`,
pins the core revision and selects a patch-hashed image in `server/compose.yaml`.
Do not use only a new client with an old running server. If you set
`SKATECRAFT_SERVER_PROJECT`, use that project name instead in Compose commands.
The builder is a Bash/Linux script; on Windows use a Linux/WSL environment with Docker
access and a prepared vmangos-deploy directory (`setup/build-server.sh /path/to/deploy`),
or connect to a separately prepared Linux server. `setup.bat` alone deploys a stock server
and cannot negotiate Tarnished combat.

For the local server, launch with `./play.sh` (Linux) or `play.bat` (Windows). These
scripts **do not rebuild**: they run the release binary and supply the default animation
path. If you rerun normal setup, set `WOW_TARNISHED=1` first to retain `er-data`;
otherwise setup rebuilds without it. With that flag, setup also extracts missing inputs.

For a server on another machine, run the release binary directly from the checkout
instead: the play scripts overwrite `WOW_HOST` with `localhost`. For example on Linux:

```sh
WOW_HOST="your-lan-server:3724" WOW_DATA="/path/to/WoW/Data" \
WOW_SKATE_ASSETS="$PWD/skate-data/assets" WOW_SKATE_AUDIO="$PWD/skate-audio" \
ER_ANIMATIONS="$PWD/.setup/tarnished/assets/player_anims.bin" \
WOW_GM=off target/release/benilla
```

On PowerShell set the same variables with `$env:NAME = "value"`, then run
`.\target\release\benilla.exe`. Both authentication and the advertised realm address
must be reachable; keep development servers off the public internet.

### 5. Supervised local and two-client checks

**No supervised live two-client combat verification has been completed.** The following
is a manual verification procedure, not a claim that it passes. Never automate combat or
run an unattended combat probe.

Normal setup creates a GM account. For these checks create **two separate ordinary
accounts**, not merely GM accounts with their visible flag disabled:

```sh
python3 tools/make_account.py tarnished1 '<your-test-password>' --gm 0
python3 tools/make_account.py tarnished2 '<another-test-password>' --gm 0
```

On Windows use `python`. Replace the placeholders locally; do not commit credentials.
Log in manually and create different characters. Set `WOW_GM=off` before launching
(`export WOW_GM=off` on Linux, `$env:WOW_GM = "off"` in PowerShell).
Use an alive, unmounted, non-skating character of any class with the desired equipment.
Obtain equipment normally or provision it separately,
then test on ordinary accounts. Both clients must use this fork's relay-compatible revision;
an observer needs no ER files to display relayed poses, but a participant entering stance
needs the feature-enabled build and its own generated data.

| Action | Xbox-style pad | Keyboard / mouse |
|---|---|---|
| Enter / leave stance | Start | Cast **Tarnished Stance** / F8 shortcut |
| Move / look | Left / right stick | WASD / hold either mouse button and move |
| Light attack | RB | U |
| Heavy attack (hold to charge) | RT | I |
| Guard / off-hand attack | LB | O (hold to guard) |
| Roll / sprint | Tap / hold B | Tap / hold Space |
| Jump | A | — |
| Acquire / lock / unlock an enemy | R3 | F9 |
| Switch locked target | Flick right stick left / right | — |
| Interact / take all nearby corpse loot | Y | Right-click (normal loot settings) |

Keep the game focused and chat closed. Wait for `tarnished: server approved equipped
stance` before testing; a request or visible pose alone does not grant combat ownership.
The lock clears on target death, removal, range or occlusion. Do not substitute the
upstream sandbox's bindings: crouch, weapon cycling and grip changes are not exposed
by this bridge.

First check locally, then repeat with a second client on the same server, swapping roles.
Use a consensual duel for player-versus-player tests and suitable nearby NPCs for PvE:

- Enter/exit stance; walk, strafe, sprint, roll, attack and charge. Check stamina spending,
  regeneration and exhausted-action refusal on the HUD. Leaving stance restores ordinary
  WoW control; mounting, skating or ineligible equipment must not retain combat ownership.
- Against incoming hits, hold **LB/O without other buttons**: front guard should absorb
  and spend stamina; releasing it or being struck from behind must not grant active guard.
  Repeated guarded hits should reach guard break. Distinguish passive WoW dodge/parry
  from active guard; merely equipping a shield does not enable active auto-block.
- Roll through a hit and compare with one before/after its iframe window. Confirm actual
  health/combat-log outcomes, not just a rolling animation. NPCs and stock defenders
  retain native defence; passive dodge/parry applies to melee, not spells.
- Swing through multiple attackable targets. Check one contact per target per hit window,
  misses outside blade reach, and no extra damage from holding an attack or repeated pose
  updates. Check damage on each target, not only the selected one.
- Have the observer inspect sword/shield alignment, guard, rolls and attack poses while
  standing and moving. Leave/re-enter visibility, toggle stance, die, log out/reconnect
  and change maps: no stale ER pose should remain.
- In a supervised, controlled interruption, stop one client briefly and resume it.
  Remote poses expire rather than freeze indefinitely (client timeout is two seconds);
  unanswered defender requests fall back to native server resolution (350 ms), not
  permanent invulnerability. Separate pose expiry from disconnect/despawn effects.

Read the preflight banner first and record the build, gear, target, health/stamina changes
and combat-log results. Measure timeout/iframe timing with timestamped traces, not video
alone. Ordinary movement remains subject to server anticheat; successful GM movement is
not evidence for an ordinary account.

### Troubleshooting and limits

- `Set ER_FILES ... containing chr` / `Set ER_GAME_DIR ... containing oo2core_6_win64.dll`:
  use the containing folders and set both variables in the shell running setup.
- Missing `.anibnd.dcx` / `.param`: compare the full layout above; avoid an extra nested
  `regulation-bin/` or renamed/exploded animation containers.
- Wine / `WINE_PYTHON` errors or Oodle communication/load errors: verify Wine is on `PATH`,
  the Windows Python file exists and is 64-bit, and the game's own DLL is present.
  Do not download replacement game DLLs. The wrapper disables upstream's automatic
  Windows Python bootstrap by requiring your supplied file.
- Download failures: check internet access to the pinned GitHub raw URLs and rerun setup.
- `ELDEN RING action table is missing`: extract first, or correct `ER_EXTRACTED`, then
  rebuild with `er-data`. `tarnished: cannot load ...`: correct `ER_ANIMATIONS` or regenerate
  both outputs; the client refuses stance rather than substitute stock action animations.
- Stance unavailable: verify the release binary was built with `er-data`, the patched
  server was actually restarted, the character has learned Tarnished Stance, and the mount/form/focus requirements hold.
  A stock server receives only safe discovery and never enables the extension.

This is an experimental equipment-driven combat bridge, not the full ELDEN RING game or a
server-authoritative ER simulation. The attacker owns swept contacts and the defender
answers correlated hits; WoW still owns terrain and health. Animation retargeting and
remote interpolation are approximate. ER sound baking is not integrated. Several upstream
constants and this fork's stamina/damage normalization are estimates or fork policy,
not verified original-game rules.

## Help

This is a hobby project with **best-effort help only**. If something breaks:

1. Run the doctor (`setup/setup.sh --doctor`, or on Windows
   `powershell -ExecutionPolicy Bypass -File setup\setup.ps1 -Doctor`) and read
   [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
2. Still stuck? Ask in [Discussions](../../discussions) (Q&A) and attach your `setup.log` and the
   doctor output. Questions without them may go unanswered.

**Windows testers:** whether it works or not, a post in Discussions with your Windows version,
GPU, controller and `setup.log` helps a lot.

## Credits

- **[benilla](https://github.com/samwhosung/benilla)** by samwhosung and contributors: the WoW
  1.12.1 client this is built on. Its own README is in [docs/BENILLA.md](docs/BENILLA.md).
- **[Skate 3 Rust Engine](https://github.com/SK8-ENGINE/skate-3-rust-engine)** by SK8-ENGINE /
  Chasm: the skateboarding, built on **dumbad**'s years of Skate 3 reverse engineering
  ([DumbadsSkate3ModdingTools](https://github.com/Ethanw05/DumbadsSkate3ModdingTools)).
- **[2010 Rust Rewrite Mashup](https://github.com/chasmlol/2010-rust-rewrite-mashup)** by chasmlol,
  on [IW4L](https://github.com/vladtrc/iw4L) by vladtrc: the Skate 3 mode for MW2 this one
  follows.
- **[vmangos](https://github.com/vmangos/core)** and
  **[vmangos-deploy](https://github.com/mserajnik/vmangos-deploy)** (mserajnik): the local server.
  **[vgmstream](https://vgmstream.org)**: decodes the Skate 3 sounds during setup.

## Legal

The code here is benilla's licence, MIT or Apache 2.0 at your option, except where a file says
otherwise; see [NOTICE](NOTICE). The skate engine is downloaded from its authors' repository at
build time and is not part of this repository. No World of Warcraft, Skate 3 or ELDEN RING
content is included or distributed: the game reads your WoW install, and setup converts your
own Skate 3 and optional ELDEN RING files on your machine. World of Warcraft is a trademark
of Blizzard Entertainment, Inc.; Skate is a trademark of Electronic Arts Inc. ELDEN RING
belongs to its respective rights holders, FromSoftware and Bandai Namco.
