//! The attacker's client owns which enemies a swing reached. A single swept swing can touch
//! several, so this tests the blade's path against every candidate body and builds one
//! [`AttackTarget`] per enemy it connected with.
//!
//! The hit geometry follows the rewrite (`third_party/elden-ring-combat-rewrite`,
//! `src/sim/mod.rs::player_attacks`): a blow lands when a capsule swept along the blade's path
//! this tick comes within the hit radius plus the body radius of the target, tested against the
//! body (feet-to-shoulders) and the head. Here it is tested against many bodies at once.
//! Fork policy: add 30% of character height to extracted horizontal weapon reach, multiplied
//! by 1.5 for Greatsword or 1.1 otherwise. A separate fixed torso capsule protects close hits.

use glam::Vec3;

use benilla_protocol::tarnished::AttackTarget;

use crate::scale::{scale_damage, target_flags, Outcome, WowWeapon};

/// Extra horizontal allowance as a fraction of the host character's standing height.
pub const HEIGHT_ALLOWANCE: f32 = 0.30;
/// Overshoot protection in metres, independent of character size and equipped weapon.
pub const CLOSE_CONTACT_RADIUS: f32 = 0.60;

/// WoW two-handed swords (including Ashkandi) map to the extracted Greatsword class.
pub fn reach_multiplier(weapon_class: u8) -> f32 {
    if weapon_class == crate::ErWeapon::Greatsword.code() {
        1.5
    } else {
        1.1
    }
}

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
    /// The attacker's collision-resolved feet position, in reference metres.
    pub origin: Vec3,
    pub radius: f32,
    /// The blade's two ends at each sampled moment from the last tick to this one.
    pub sweep: [(Vec3, Vec3); crate::SWEEP_STEPS],
}

impl Swing {
    /// Extend the extracted blade tip horizontally, retaining its height and the original
    /// near endpoint so increased reach doesn't open a gap between the player and the blade.
    pub fn from_extracted(
        origin: Vec3,
        radius: f32,
        sweep: [(Vec3, Vec3); crate::SWEEP_STEPS],
        character_height: f32,
        weapon_class: u8,
    ) -> Self {
        let multiplier = reach_multiplier(weapon_class);
        let allowance = character_height * HEIGHT_ALLOWANCE;
        let extend = |point: Vec3| {
            let horizontal = (point - origin).with_y(0.0);
            point + horizontal * (multiplier - 1.0) + horizontal.normalize_or_zero() * allowance
        };
        Self {
            origin,
            radius: radius * multiplier,
            sweep: sweep.map(|(a, b)| (a, extend(b))),
        }
    }

    fn touches(&self, body: &Body) -> bool {
        let ((feet, shoulders), head) = body.segments();
        let touches = |a, b, radius| {
            segment_distance(a, b, feet, shoulders) <= radius + body.radius
                || segment_distance(a, b, head, head) <= radius + body.head_radius
        };
        self.sweep.iter().any(|&(a, b)| touches(a, b, self.radius))
            // Small all-direction overlap forgiveness, bounded vertically as well as sideways.
            // Beyond this capsule, only the extracted blade path can connect.
            || touches(
                self.origin + Vec3::Y * CLOSE_CONTACT_RADIUS,
                self.origin + Vec3::Y * 1.2,
                CLOSE_CONTACT_RADIUS,
            )
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
        if body.guid == 0
            || out.iter().any(|hit: &AttackTarget| hit.guid == body.guid)
            || !swing.touches(body)
        {
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
            origin: Vec3::ZERO,
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
    fn additive_reach_keeps_the_blade_shape_and_a_small_rear_radius() {
        let original = straight_blade(1.0, 2.0, 1.0);
        let swing = Swing::from_extracted(
            original.origin,
            original.radius,
            original.sweep,
            2.0,
            crate::ErWeapon::Longsword.code(),
        );
        let added_reach = body(1, 3.30);
        assert!(!original.touches(&added_reach));
        assert!(swing.touches(&added_reach));
        assert!(!swing.touches(&body(2, 3.40)));
        assert!(
            swing.touches(&body(5, 1.20)),
            "extending the tip must not lose nearer targets"
        );
        let mut beside = body(3, 0.0);
        beside.pos.z = 1.10;
        assert!(
            !swing.touches(&beside),
            "long reach follows the blade, not a circle"
        );
        assert!(!swing.touches(&body(4, -1.10)), "rear reach stays small");
    }

    #[test]
    fn character_radius_is_added_after_the_weapon_margin() {
        // Thin targets just inside/outside the requested envelope. The same 0.6 m must be
        // added to each weapon, not multiplied by 1.1 or by the weapon's original range.
        for (range, expected) in [(1.0, 1.70), (2.16, 2.976), (2.75, 3.625)] {
            let original = straight_blade(0.5, range - 0.15, 1.0);
            let swing = Swing::from_extracted(
                original.origin,
                original.radius,
                original.sweep,
                2.0,
                crate::ErWeapon::Longsword.code(),
            );
            let mut target = body(1, expected - 0.01);
            target.radius = 0.005;
            target.head_radius = 0.005;
            assert!(swing.touches(&target), "inside {expected} m envelope");
            target.pos.x = expected + 0.01;
            assert!(!swing.touches(&target), "outside {expected} m envelope");
        }
    }

    #[test]
    fn height_and_weapon_change_reach_but_not_overshoot_protection() {
        use crate::ErWeapon::{Greatsword, Longsword};
        let original = straight_blade(1.0, 2.0, 1.0);
        for (height, weapon, expected) in [
            (1.0, Longsword, 2.665),
            (3.0, Longsword, 3.265),
            (1.0, Greatsword, 3.525),
            (3.0, Greatsword, 4.125),
        ] {
            let swing = Swing::from_extracted(
                original.origin,
                original.radius,
                original.sweep,
                height,
                weapon.code(),
            );
            let mut target = body(1, expected - 0.01);
            target.radius = 0.005;
            target.head_radius = 0.005;
            assert!(
                swing.touches(&target),
                "inside {weapon:?} reach at height {height}"
            );
            target.pos.x = expected + 0.01;
            assert!(
                !swing.touches(&target),
                "outside {weapon:?} reach at height {height}"
            );
            // 0.6 m around the player plus this target's 0.4 m body radius, for every height.
            assert!(swing.touches(&body(2, -0.9)));
            assert!(!swing.touches(&body(3, -1.1)));
        }
    }

    #[test]
    fn an_overshot_enemy_connects_once_but_other_floors_do_not() {
        let swing = straight_blade(1.0, 2.0, 1.0);
        let behind = body(1, -0.90);
        let mut above = body(2, 0.0);
        above.pos.y = 3.0;
        let mut below = body(3, 0.0);
        below.pos.y = -3.0;
        let hits = resolve_swing(
            &swing,
            WowWeapon {
                min_damage: 10.0,
                max_damage: 10.0,
            },
            1.0,
            &[behind, behind, body(4, 0.0), above, below],
            |_| Outcome::default(),
        );
        assert_eq!(hits.iter().map(|hit| hit.guid).collect::<Vec<_>>(), [1, 4]);
    }

    #[test]
    fn reach_is_relative_to_the_player_and_preserves_blade_height() {
        let origin = Vec3::new(100.0, 20.0, -300.0);
        let a = origin + Vec3::new(0.0, 1.3, 1.0);
        let b = origin + Vec3::new(0.0, 1.3, 2.0);
        let swing = Swing::from_extracted(
            origin,
            0.15,
            [(a, b); crate::SWEEP_STEPS],
            2.0,
            crate::ErWeapon::Longsword.code(),
        );
        assert!((swing.sweep[0].1 - (origin + Vec3::new(0.0, 1.3, 2.80))).length() < 0.0001);
        assert_eq!(
            swing.sweep[0].0, a,
            "keep the near end of the blade in place"
        );
        let mut target = body(1, 0.0);
        target.pos = origin + Vec3::new(0.0, 0.0, 3.30);
        assert!(swing.touches(&target));
        target.pos = origin - Vec3::Z * 0.90;
        assert!(swing.touches(&target));
    }

    #[test]
    fn duplicate_or_zero_guid_candidates_are_not_damage_multipliers() {
        let hits = resolve_swing(
            &straight_blade(0.0, 2.0, 1.0),
            WowWeapon {
                min_damage: 10.0,
                max_damage: 10.0,
            },
            1.0,
            &[body(0, 0.5), body(1, 0.5), body(1, 0.5)],
            |_| Outcome::default(),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].guid, 1);
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
