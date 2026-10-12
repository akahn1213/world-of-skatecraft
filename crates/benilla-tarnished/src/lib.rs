//! Tarnished combat: the data-independent half of benilla's ELDEN RING combat extension.
//!
//! This crate is the fork's own layer on top of benilla; it is not benilla core. It turns the
//! negotiated wire protocol of [`benilla_protocol::tarnished`] into gameplay: it maps the
//! character's real WoW armaments onto ELDEN RING weapon classes ([`weapon`]), scales an attack's
//! motion value onto the WoW weapon's damage ([`scale`]), tests a swept blade against several
//! enemies at once ([`attack`]), and resolves an incoming hit in the fixed defence order
//! ([`defence`]). None of that needs a single game file, so it always compiles and is fully
//! tested.
//!
//! The faithful timings, movesets and animation-derived hit shapes live in the vendored rewrite
//! (`third_party/elden-ring-combat-rewrite`), which is driven by an action table generated from
//! the player's own copy of the game. That table is never in the repository. The `er-data`
//! feature wires the real simulation in behind the [`Backend`] trait; without it the feature is
//! unavailable at runtime with a precise message ([`unavailable`]), and an ordinary build touches
//! no game-derived data.

pub mod attack;
pub mod defence;
pub mod profile;
pub mod scale;
pub mod weapon;

pub use weapon::{ErWeapon, Hand, Loadout, Offhand};

/// How many moments along the blade's path a single tick's sweep is sampled at, matching the
/// rewrite's `SWEEP_STEPS` (`third_party/elden-ring-combat-rewrite`, `src/sim/mod.rs`). Kept here
/// so the data-independent swept-contact test uses the same resolution as the real simulation.
pub const SWEEP_STEPS: usize = 5;

/// The data-independent fallback is always present; the faithful simulation only when `er-data`
/// is compiled in with the player's generated action table.
pub const fn er_data_compiled() -> bool {
    cfg!(feature = "er-data")
}

/// The precise reason Tarnished combat is unavailable in a build without the generated action
/// table. Shown to the player instead of a silent no-op, so a missing-asset build is explained,
/// not mysteriously inert — the runtime twin of the `er-data` build-time error in `build.rs`.
pub fn unavailable() -> &'static str {
    "Tarnished combat is unavailable: this build has no ELDEN RING action table.\n\
     It is generated from your own copy of the game and is never shipped. Follow the \
     \"Tarnished combat\" section of README.md / INSTALL.md to unpack your ELDEN RING files and \
     run the setup, then build benilla with the `er-data` feature. The rest of benilla, \
     including skating, is unaffected."
}

/// The faithful, game-derived simulation benilla-app drives for a local Tarnished character:
/// swing timings, movesets and animation-derived hit shapes. It is implemented only when the
/// vendored rewrite is compiled (`er-data`); the trait lets benilla-app depend on the shape
/// without depending on the game data, and fall back to [`unavailable`] when it is absent.
///
/// The methods here are the seam the real state machine (`Player::step`, `active_hit`,
/// `receive_hit`) is adapted onto; the adapter itself lives behind `er-data` so the default build
/// never references the vendored module.
pub trait Backend {
    /// Advance the local character's state machine by one fixed 60 Hz tick.
    fn step(&mut self, input: BackendInput);
    /// The blade's swept path this tick, if an attack is in its active frames.
    fn active_swing(&self) -> Option<attack::Swing>;
    /// The motion value of the attack currently in its active frames, for damage scaling.
    fn active_motion_value(&self) -> Option<f32>;
    /// The guard held this instant, if any, for the defence resolver.
    fn guard(&self) -> Option<defence::Guard>;
    /// A dodge-roll i-frame is open this instant.
    fn invincible(&self) -> bool;
}

/// One tick of controller intent handed to a [`Backend`]. Engine-free so the trait stays in the
/// data-independent crate; benilla-app fills it from the reused XInput transport.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackendInput {
    /// Desired planar move direction in the character's frame, length 0..1.
    pub move_x: f32,
    pub move_y: f32,
    pub light_attack: bool,
    pub heavy_attack: bool,
    pub guard: bool,
    pub dodge: bool,
    /// Seconds since the previous tick, for a variable-rate host to sub-step onto 60 Hz.
    pub dt: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_names_the_feature_and_the_fix() {
        let msg = unavailable();
        assert!(msg.contains("Tarnished combat is unavailable"));
        assert!(msg.contains("er-data"));
    }

    #[test]
    fn sweep_steps_matches_the_rewrite() {
        assert_eq!(SWEEP_STEPS, 5);
    }

    #[test]
    fn er_data_off_by_default() {
        // The workspace build never turns the feature on; the data-independent layer stands alone.
        assert_eq!(er_data_compiled(), cfg!(feature = "er-data"));
    }
}
