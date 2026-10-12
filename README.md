# World of Skatecraft

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

> **Unofficial fan project.** Not affiliated with Blizzard Entertainment or Electronic Arts.
> **No game files are included.** You need your own copies of both games.

> **Vibe coded, just for laughs.** This whole thing was put together with an AI coding assistant
> (Claude Code) for fun, on top of other people's serious work (see Credits). Expect rough edges,
> weird bugs and a skater who doesn't always know where his sword goes. Don't take it seriously.

## What you need

### Tarnished combat (experimental)

`benilla-tarnished` contains weapon mapping, swept contacts and defender policy. Its optional
`er-data` adapter drives the pure simulation pinned at
`177a3b5ec14a9bde39b9ab458a3400b4038224e3`. Build the launcher with `--features er-data`
and use the patched vmangos server. F8 (pad Start) opts a living, unmounted warrior with a
one-handed sword and shield into the stance; skating and ordinary movement cannot own the pad
at the same time. RB attacks, RT charges a heavy attack, LB holds guard, and tapping B rolls
while holding B sprints. Keyboard equivalents are U, I, O and Space. F9 (R3) locks the selected
attackable target; removal, death, range or occlusion clears the lock.

The attacker owns swept contacts; the defender answers server-correlated hits in iframe,
passive dodge/parry, held front guard, hit order. NPC and stock clients retain native defence.
The generated table supplies timing, stamina and root/blade motion; presentation currently uses
WoW animation clips, not ER skeletal or sound assets. Stock incoming stamina is normalized onto
the reference sword and the pinned dummy's damage/stamina ratio. This is a fork policy, not an
extracted WoW or ER rule. There has been no supervised live two-client combat verification.

Generate the action table from your own ELDEN RING files with `python3 setup/tarnished.py`;
see [INSTALL.md](INSTALL.md#tarnished-action-table). No ELDEN RING data is distributed.
The reference is this pinned rewrite, which explicitly marks several tuning constants as
estimates; those are not verified facts about the original ELDEN RING client.

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
| D-pad | skate abilities: ↑ Ollie Boost, → Rocket Boost, ← Speed Demon, ↓ Moon Jump |

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
build time and is not part of this repository. No World of Warcraft or Skate 3 content is
included or distributed: the game reads your WoW install, and setup converts your own Skate 3
files on your machine. World of Warcraft is a trademark of Blizzard Entertainment, Inc.; Skate is
a trademark of Electronic Arts Inc.
