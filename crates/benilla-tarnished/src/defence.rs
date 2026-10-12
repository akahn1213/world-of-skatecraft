//! The defender's client owns how an incoming hit is answered. Every melee or spell aimed at a
//! Tarnished defender is resolved here, in one fixed order, and the chosen [`DefenceOutcome`] is
//! what the server applies — it never rolls avoidance again on top.
//!
//! The order is: an active dodge-roll i-frame, then an eligible passive WoW dodge or parry (melee
//! only, the stock percentages kept), then an active shield or two-handed guard, then the hit
//! itself. An equipped shield grants no automatic block in Tarnished mode: a block happens only
//! while the guard is actually held and the blow comes from the front, with the stamina cost and
//! guard-break of the rewrite (`third_party/elden-ring-combat-rewrite`, `src/sim/player.rs`:
//! `receive_hit`, where a standing guard spends stamina and negates the hit, and an empty stamina
//! bar breaks the guard).

use benilla_protocol::tarnished::{DefenceOutcome, Incoming};

/// Share of a hit's stamina damage a raised guard still takes (`GUARD_STAMINA_TAKEN`).
pub const GUARD_STAMINA_TAKEN: f32 = 0.55;
/// Half-angle of the frontal arc a guard covers (`GUARD_ARC_DEG`).
pub const GUARD_ARC_DEG: f32 = 80.0;
/// The physical school index a guard fully absorbs by default; spells leak through.
pub const SCHOOL_PHYSICAL: u8 = 0;
/// ESTIMATE: stamina a guard spends is scaled from the blow's damage, since the forwarded
/// [`Incoming`] carries damage, not the attacker's stamina figure. Tuned so a solid hit costs a
/// handful of stamina; a fork with a generated profile can override it.
pub const GUARD_STAMINA_PER_DAMAGE: f32 = 0.2;

/// The shield's or guard's absorption, per school. Physical defaults to full (the rewrite negates
/// a standing guard's hit entirely); a fork can lower it for a buckler or raise magic cover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Absorption {
    pub physical: f32,
    pub magic: f32,
}

impl Default for Absorption {
    fn default() -> Self {
        // The rewrite's shield fully negates a blocked hit's HP damage.
        Self {
            physical: 1.0,
            magic: 0.0,
        }
    }
}

impl Absorption {
    fn for_school(self, school: u8) -> f32 {
        if school == SCHOOL_PHYSICAL {
            self.physical
        } else {
            self.magic
        }
        .clamp(0.0, 1.0)
    }
}

/// A guard held this instant.
#[derive(Clone, Copy, Debug)]
pub struct Guard {
    /// Stamina available to spend, in points.
    pub stamina: f32,
    pub absorption: Absorption,
    /// Cosine of the angle between the defender's facing and the direction to the source; a blow
    /// outside the frontal arc is not covered. `1.0` is dead ahead.
    pub source_facing_dot: f32,
}

impl Guard {
    fn covers(&self) -> bool {
        self.source_facing_dot >= GUARD_ARC_DEG.to_radians().cos()
    }
}

/// Everything the defender's client knows this instant, enough to resolve one incoming hit.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefenceState {
    /// A dodge-roll's invincibility window is open.
    pub iframe: bool,
    /// Passive WoW dodge chance, 0..1 (melee only).
    pub dodge_chance: f32,
    /// Passive WoW parry chance, 0..1 (melee only, and only when the defender can parry).
    pub parry_chance: f32,
    pub can_parry: bool,
    /// The guard is held this instant, if any.
    pub guard: Option<Guard>,
}

/// Resolve one incoming hit to the outcome the server will apply. `roll` is the caller's uniform
/// 0..1 draw for the passive avoidance step, kept as a parameter so the policy is deterministic
/// and testable.
pub fn resolve(incoming: &Incoming, state: &DefenceState, roll: f32) -> DefenceOutcome {
    // 1. An active i-frame beats everything.
    if state.iframe {
        return DefenceOutcome::Iframe;
    }

    // 2. Passive WoW avoidance, melee only, the stock percentages. Dodge then parry share the
    //    one roll so their chances add rather than compound.
    if incoming.melee {
        let dodge = state.dodge_chance.clamp(0.0, 1.0);
        if roll < dodge {
            return DefenceOutcome::Dodge;
        }
        if state.can_parry {
            let parry = state.parry_chance.clamp(0.0, 1.0);
            if roll < dodge + parry {
                return DefenceOutcome::Parry;
            }
        }
    }

    // 3. An actively held guard, from the front only. No shield auto-blocks on its own.
    if let Some(guard) = state.guard {
        if guard.covers() {
            let absorb_frac = guard.absorption.for_school(incoming.school);
            let absorbed = (incoming.amount as f32 * absorb_frac).round() as u32;
            let stamina_cost_f =
                incoming.amount as f32 * GUARD_STAMINA_PER_DAMAGE * GUARD_STAMINA_TAKEN;
            let guard_broken = guard.stamina < stamina_cost_f;
            let stamina_cost = stamina_cost_f.min(guard.stamina).round().max(0.0) as u16;
            let taken = incoming.amount.saturating_sub(absorbed);
            return DefenceOutcome::Block {
                absorbed,
                stamina_cost,
                guard_broken,
                taken,
            };
        }
    }

    // 4. Nothing stopped it.
    DefenceOutcome::Hit {
        taken: incoming.amount,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn melee(amount: u32) -> Incoming {
        Incoming {
            attack_id: 1,
            source_guid: 9,
            school: SCHOOL_PHYSICAL,
            amount,
            melee: true,
        }
    }

    #[test]
    fn iframe_beats_everything() {
        let state = DefenceState {
            iframe: true,
            dodge_chance: 0.0,
            guard: Some(Guard {
                stamina: 100.0,
                absorption: Absorption::default(),
                source_facing_dot: 1.0,
            }),
            ..Default::default()
        };
        assert_eq!(resolve(&melee(200), &state, 0.99), DefenceOutcome::Iframe);
    }

    #[test]
    fn passive_dodge_then_parry_share_one_roll() {
        let state = DefenceState {
            dodge_chance: 0.1,
            parry_chance: 0.1,
            can_parry: true,
            ..Default::default()
        };
        assert_eq!(resolve(&melee(100), &state, 0.05), DefenceOutcome::Dodge);
        assert_eq!(resolve(&melee(100), &state, 0.15), DefenceOutcome::Parry);
        // Past dodge+parry, neither avoids.
        assert!(matches!(
            resolve(&melee(100), &state, 0.5),
            DefenceOutcome::Hit { .. }
        ));
    }

    #[test]
    fn dodge_and_parry_do_not_apply_to_spells() {
        let state = DefenceState {
            dodge_chance: 0.9,
            parry_chance: 0.9,
            can_parry: true,
            ..Default::default()
        };
        let spell = Incoming {
            school: 2,
            melee: false,
            ..melee(100)
        };
        assert!(matches!(
            resolve(&spell, &state, 0.0),
            DefenceOutcome::Hit { .. }
        ));
    }

    #[test]
    fn a_frontal_guard_absorbs_physical_and_spends_stamina() {
        let state = DefenceState {
            guard: Some(Guard {
                stamina: 100.0,
                absorption: Absorption::default(),
                source_facing_dot: 1.0,
            }),
            ..Default::default()
        };
        match resolve(&melee(200), &state, 0.99) {
            DefenceOutcome::Block {
                absorbed,
                taken,
                guard_broken,
                stamina_cost,
            } => {
                assert_eq!(absorbed, 200);
                assert_eq!(taken, 0);
                assert!(!guard_broken);
                assert!(stamina_cost > 0);
            }
            other => panic!("expected a block, got {other:?}"),
        }
    }

    #[test]
    fn a_guard_from_behind_does_not_cover() {
        let state = DefenceState {
            guard: Some(Guard {
                stamina: 100.0,
                absorption: Absorption::default(),
                source_facing_dot: -1.0,
            }),
            ..Default::default()
        };
        assert!(matches!(
            resolve(&melee(200), &state, 0.99),
            DefenceOutcome::Hit { taken: 200 }
        ));
    }

    #[test]
    fn an_empty_stamina_bar_breaks_the_guard() {
        let state = DefenceState {
            guard: Some(Guard {
                stamina: 1.0,
                absorption: Absorption::default(),
                source_facing_dot: 1.0,
            }),
            ..Default::default()
        };
        match resolve(&melee(500), &state, 0.99) {
            DefenceOutcome::Block { guard_broken, .. } => assert!(guard_broken),
            other => panic!("expected a broken guard, got {other:?}"),
        }
    }

    #[test]
    fn a_buckler_absorption_leaves_a_remainder() {
        let state = DefenceState {
            guard: Some(Guard {
                stamina: 100.0,
                absorption: Absorption {
                    physical: 0.5,
                    magic: 0.0,
                },
                source_facing_dot: 1.0,
            }),
            ..Default::default()
        };
        match resolve(&melee(200), &state, 0.99) {
            DefenceOutcome::Block {
                absorbed, taken, ..
            } => {
                assert_eq!(absorbed, 100);
                assert_eq!(taken, 100);
            }
            other => panic!("expected a partial block, got {other:?}"),
        }
    }
}
