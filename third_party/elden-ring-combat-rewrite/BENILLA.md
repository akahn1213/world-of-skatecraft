# Vendored: ELDEN RING Combat Rewrite — what is here, and why

Upstream: [`Funny-Bones/ELDEN-RING-Combat-Rewrite`](https://github.com/Funny-Bones/ELDEN-RING-Combat-Rewrite),
MIT (`LICENSE.md`, copyright 2026 Funny-Bones and contributors), pinned at commit
`177a3b5ec14a9bde39b9ab458a3400b4038224e3`.

Only the `src/sim/` state machine is copied here, byte for byte, plus the upstream `build.rs` and
`LICENSE.md`. `src/sim/` is "the whole game as a pure 60 Hz state machine, with no engine types":
`mod.rs`, `data.rs`, `player.rs`, `dummy.rs`, `level.rs` and `tests.rs`. The renderer, audio,
camera, HUD, input and the Python tools are **not** copied — World of Skatecraft drives the
simulation from its own Bevy app and the stock WoW UI, not upstream's sandbox.

## This is code, never data

Nothing generated from ELDEN RING is in this repository, upstream or here. The simulation is
driven by an action table upstream calls `src/sim/extracted.rs` (timings, hit windows, i-frames,
root motion, stamina, motion values, the weapon table) and animation/sound blobs, all generated
from the player's own copy of the game by `tools/*.py`. Those files are in upstream's `.gitignore`
and in ours (`er-files/`, `*.er-profile`); each player generates them after cloning. See
`README.md` ("Tarnished combat") and `INSTALL.md`.

## How World of Skatecraft uses it

These files are **not compiled by the Cargo workspace by default** — upstream's own `build.rs`
exits when `src/sim/extracted.rs` is absent, which it always is here. They are vendored for
provenance and so a fork that has generated the action table can wire the real simulation in
behind the `benilla-tarnished` crate's `er-data` feature, which `include!`s this tree alongside
the generated `extracted.rs`.

The data-independent WoW integration — mapping WoW weapon subclasses onto ELDEN RING weapon
classes, scaling an attack's motion value onto a WoW weapon's damage, the ordered defence
resolver (i-frame → passive dodge/parry → shield guard → damage) and the swept multi-target
contact test — lives in `crates/benilla-tarnished` and reuses this simulation's design and
constants (cited there), so it builds and is tested without any game data. The rewrite labels
guard arc, guard raise time and stamina multiplier as ESTIMATE; they are not extracted facts.
`setup/tarnished.py` retrieves the pinned extraction tools into ignored `.setup/tarnished/`
and generates the action table there. `er-data` compiles build-directory copies of this tree,
substituting glam vector imports and including that table, leaving these vendor files untouched.
The adapter is `crates/benilla-tarnished/src/reference.rs`; the host/controller is
`crates/benilla-app/src/tarnished/`, and the negotiated server routes are in
`setup/vmangos/tarnished.patch`. WoW clips present the actions until an ER skeletal asset adapter
exists; no animation or sound blobs are supplied by the action extractor.

## Checking it against upstream

```sh
REF=177a3b5ec14a9bde39b9ab458a3400b4038224e3
for f in mod data player dummy level tests; do
  diff <(curl -fsSL "https://raw.githubusercontent.com/Funny-Bones/ELDEN-RING-Combat-Rewrite/$REF/src/sim/$f.rs") \
       third_party/elden-ring-combat-rewrite/src/sim/$f.rs
done
```

That must print nothing: every file is byte-identical to the pinned upstream.
