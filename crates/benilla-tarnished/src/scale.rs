//! Scaling an ELDEN RING attack's motion value onto the attacker's real WoW weapon.
//!
//! The rewrite computes an attack's damage as a weapon's base attack times the attack's motion
//! value (`third_party/elden-ring-combat-rewrite`, `src/sim/mod.rs`: `hit.attack * hit.mv`).
//! Tarnished combat keeps the motion value — the thing that makes a charged heavy hit far harder
//! than a jab — but swaps the rewrite's own base attack for the WoW weapon's damage, so numbers
//! stay in the band a 1.12 server, its health pools and its combat log expect. Crit and glancing
//! then apply the stock WoW multipliers.

/// WoW physical crit is a doubling (`+0x...`, the 1.12 melee crit bonus is ×2).
pub const CRIT_MULTIPLIER: f32 = 2.0;
/// A glancing blow against a higher-level target loses roughly a third in 1.12.
pub const GLANCING_MULTIPLIER: f32 = 0.65;

/// The WoW weapon the swing is scaled onto: its damage range, as the item carries it.
#[derive(Clone, Copy, Debug)]
pub struct WowWeapon {
    pub min_damage: f32,
    pub max_damage: f32,
}

impl WowWeapon {
    /// The average weapon swing damage, the base the motion value multiplies.
    pub fn base(self) -> f32 {
        0.5 * (self.min_damage + self.max_damage)
    }
}

/// How a single contact landed, for the crit / glancing multipliers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub crit: bool,
    pub glancing: bool,
}

/// Final pre-armour physical damage of one contact: the WoW weapon's base swing times the ELDEN
/// RING motion value, then the stock crit / glancing multipliers. Never negative; rounds to a
/// whole point the combat log can show.
pub fn scale_damage(weapon: WowWeapon, motion_value: f32, outcome: Outcome) -> u32 {
    let mut dmg = weapon.base().max(0.0) * motion_value.max(0.0);
    if outcome.crit {
        dmg *= CRIT_MULTIPLIER;
    }
    if outcome.glancing {
        dmg *= GLANCING_MULTIPLIER;
    }
    dmg.round().max(0.0) as u32
}

/// Pack a contact's [`Outcome`] into the wire flags of a
/// [`benilla_protocol::tarnished::AttackTarget`].
pub fn target_flags(outcome: Outcome) -> u8 {
    let mut flags = 0;
    if outcome.crit {
        flags |= 0x01;
    }
    if outcome.glancing {
        flags |= 0x02;
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWORD: WowWeapon = WowWeapon {
        min_damage: 80.0,
        max_damage: 120.0,
    };

    #[test]
    fn base_is_the_average_swing() {
        assert_eq!(SWORD.base(), 100.0);
    }

    #[test]
    fn motion_value_scales_and_rounds() {
        // A light jab at motion value 1.0 is the weapon's base swing.
        assert_eq!(scale_damage(SWORD, 1.0, Outcome::default()), 100);
        // A charged heavy at 2.6 hits far harder.
        assert_eq!(scale_damage(SWORD, 2.6, Outcome::default()), 260);
    }

    #[test]
    fn crit_doubles_and_glancing_cuts() {
        let crit = scale_damage(
            SWORD,
            1.0,
            Outcome {
                crit: true,
                glancing: false,
            },
        );
        assert_eq!(crit, 200);
        let glancing = scale_damage(
            SWORD,
            1.0,
            Outcome {
                crit: false,
                glancing: true,
            },
        );
        assert_eq!(glancing, 65);
    }

    #[test]
    fn never_negative() {
        assert_eq!(scale_damage(SWORD, -1.0, Outcome::default()), 0);
    }

    #[test]
    fn flags_round_trip_with_the_protocol_bits() {
        let flags = target_flags(Outcome {
            crit: true,
            glancing: true,
        });
        assert_eq!(flags, 0x03);
    }
}
