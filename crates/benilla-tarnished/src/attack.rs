//! The attacker's client owns which enemies a swing reached. A single swept swing can touch
//! several, so this tests the blade's path against every candidate body and builds one
//! [`AttackTarget`] per enemy it connected with.
//!
//! The hit geometry follows the rewrite (`third_party/elden-ring-combat-rewrite`,
//! `src/sim/mod.rs::player_attacks`): a blow lands when a capsule swept along the blade's path
//! this tick comes within the hit radius plus the body radius of the target, tested against the
//! body (feet-to-shoulders) and the head. Here it is tested against many bodies at once.

use glam::Vec3;

use benilla_protocol::tarnished::AttackTarget;

use crate::scale::{scale_damage, target_flags, Outcome, WowWeapon};

/// Shortest distance between segment a–b and segment c–d. Ported from the rewrite's
/// `segment_distance` (`src/sim/mod.rs`).
pub fn segment_distance(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> f32 {
    let (u, v, w) = (b - a, d - c, a - c);
    let (uu, uv, vv, uw, vw) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
    let denom = uu * vv - uv * uv;
    let mut s = if denom > 1e-8 {
        ((uv * vw - vv * uw) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let t = if vv > 1e-8 {
        ((uv * s + vw) / vv).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if uu > 1e-8 {
        s = ((uv * t - uw) / uu).clamp(0.0, 1.0);
    }
    (a + u * s).distance(c + v * t)
}

/// A candidate enemy's body, a vertical capsule plus a head sphere, as the rewrite models the
/// sparring dummy (`src/sim/dummy.rs`).
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub guid: u64,
    pub pos: Vec3,
    pub radius: f32,
    pub height: f32,
    pub head_height: f32,
    pub head_radius: f32,
}

impl Body {
    /// The vertical body segment (feet to shoulders) and the head point.
    fn segments(&self) -> ((Vec3, Vec3), Vec3) {
        let feet = self.pos + Vec3::Y * self.radius;
        let shoulders = self.pos + Vec3::Y * (self.height - self.radius);
        let head = self.pos + Vec3::Y * self.head_height;
        ((feet, shoulders), head)
    }
}

/// The live swing this tick: the blade's swept path (its two ends at several moments) and the hit
/// capsule's thickness.
#[derive(Clone, Copy, Debug)]
pub struct Swing {
    pub radius: f32,
    /// The blade's two ends at each sampled moment from the last tick to this one.
    pub sweep: [(Vec3, Vec3); crate::SWEEP_STEPS],
}

impl Swing {
    fn touches(&self, body: &Body) -> bool {
        let ((feet, shoulders), head) = body.segments();
        self.sweep.iter().any(|&(a, b)| {
            segment_distance(a, b, feet, shoulders) <= self.radius + body.radius
                || segment_distance(a, b, head, head) <= self.radius + body.head_radius
        })
    }
}

/// How one contact is rolled for crit / glancing, decided per target by the attacker's client.
pub type RollOutcome = Outcome;

/// Resolve a swept swing against every candidate body, producing one [`AttackTarget`] per enemy
/// it reached. `weapon` and `motion_value` scale the damage; `outcome_for` rolls crit / glancing
/// for a given target guid (the caller's RNG), so the geometry stays deterministic and testable.
pub fn resolve_swing(
    swing: &Swing,
    weapon: WowWeapon,
    motion_value: f32,
    bodies: &[Body],
    mut outcome_for: impl FnMut(u64) -> RollOutcome,
) -> Vec<AttackTarget> {
    let mut out = Vec::new();
    for body in bodies {
        if !swing.touches(body) {
            continue;
        }
        let outcome = outcome_for(body.guid);
        out.push(AttackTarget {
            guid: body.guid,
            damage: scale_damage(weapon, motion_value, outcome),
            flags: target_flags(outcome),
        });
        if out.len() >= benilla_protocol::tarnished::MAX_TARGETS {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(guid: u64, x: f32) -> Body {
        Body {
            guid,
            pos: Vec3::new(x, 0.0, 0.0),
            radius: 0.4,
            height: 1.8,
            head_height: 1.7,
            head_radius: 0.2,
        }
    }

    fn straight_blade(x0: f32, x1: f32, y: f32) -> Swing {
        // A horizontal blade sweeping along x at chest height, barely moving over the tick.
        let a = Vec3::new(x0, y, 0.0);
        let b = Vec3::new(x1, y, 0.0);
        Swing {
            radius: 0.15,
            sweep: [(a, b); crate::SWEEP_STEPS],
        }
    }

    #[test]
    fn a_wide_swing_sweeps_several_enemies() {
        let bodies = [body(1, 0.5), body(2, 1.2), body(3, 5.0)];
        let swing = straight_blade(0.0, 2.0, 1.0);
        let weapon = WowWeapon {
            min_damage: 90.0,
            max_damage: 110.0,
        };
        let hits = resolve_swing(&swing, weapon, 1.0, &bodies, |_| Outcome::default());
        let guids: Vec<u64> = hits.iter().map(|h| h.guid).collect();
        // The two within the blade's reach are hit; the far one is not.
        assert_eq!(guids, vec![1, 2]);
        assert!(hits.iter().all(|h| h.damage == 100));
    }

    #[test]
    fn a_thrust_that_falls_short_misses() {
        let bodies = [body(1, 3.0)];
        let swing = straight_blade(0.0, 1.0, 1.0);
        let hits = resolve_swing(
            &swing,
            WowWeapon {
                min_damage: 10.0,
                max_damage: 10.0,
            },
            1.0,
            &bodies,
            |_| Outcome::default(),
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn per_target_crit_is_applied() {
        let bodies = [body(1, 0.5), body(2, 1.2)];
        let swing = straight_blade(0.0, 2.0, 1.0);
        let weapon = WowWeapon {
            min_damage: 100.0,
            max_damage: 100.0,
        };
        let hits = resolve_swing(&swing, weapon, 1.0, &bodies, |guid| Outcome {
            crit: guid == 2,
            glancing: false,
        });
        assert_eq!(hits[0].damage, 100);
        assert!(!hits[0].crit());
        assert_eq!(hits[1].damage, 200);
        assert!(hits[1].crit());
    }
}
