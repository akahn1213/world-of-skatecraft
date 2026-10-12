//! Mapping WoW 1.12 armaments onto ELDEN RING weapon classes.
//!
//! A Tarnished fights with the weapon the character actually has equipped: the client reads the
//! item's class, subclass (`ItemSubClass.dbc`) and how it is held, and chooses the nearest ELDEN
//! RING moveset. The subclass numbers are the 1.12 client's own (`ItemClass.dbc` class 2 =
//! Weapon, class 4 subclass 6 = Shield); the ELDEN RING classes and which can pair are from the
//! upstream rewrite's README and weapon table
//! (`third_party/elden-ring-combat-rewrite`, `src/sim/data.rs::WEAPONS`).

/// WoW `ItemClass.dbc` classes used here.
pub mod wow_class {
    pub const WEAPON: u32 = 2;
    pub const ARMOR: u32 = 4;
}

/// WoW weapon subclasses (`ItemClass.dbc` class 2) and the shield (class 4 subclass 6).
pub mod wow_subclass {
    pub const AXE_1H: u32 = 0;
    pub const AXE_2H: u32 = 1;
    pub const BOW: u32 = 2;
    pub const GUN: u32 = 3;
    pub const MACE_1H: u32 = 4;
    pub const MACE_2H: u32 = 5;
    pub const POLEARM: u32 = 6;
    pub const SWORD_1H: u32 = 7;
    pub const SWORD_2H: u32 = 8;
    pub const STAFF: u32 = 10;
    pub const FIST: u32 = 13;
    pub const DAGGER: u32 = 15;
    pub const THROWN: u32 = 16;
    pub const CROSSBOW: u32 = 18;
    pub const WAND: u32 = 19;
    pub const FISHING_POLE: u32 = 20;
    /// `ItemClass.dbc` class 4 (Armor).
    pub const SHIELD: u32 = 6;
}

/// An ELDEN RING weapon class. The 24 armament classes of the rewrite, in its README order, plus
/// the shield. The numeric value is the on-wire `weapon_class` of a
/// [`benilla_protocol::tarnished::AttackVerdict`]; the adapter that drives the vendored
/// simulation translates it to that build's own generated weapon index.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum ErWeapon {
    Dagger = 0,
    Longsword = 1,
    Claymore = 2,
    Greatsword = 3,
    Rapier = 4,
    Uchigatana = 5,
    Club = 6,
    BattleAxe = 7,
    ShortSpear = 8,
    Halberd = 9,
    HeavyThrustingSword = 10,
    CurvedSword = 11,
    CurvedGreatsword = 12,
    Twinblade = 13,
    GreatHammer = 14,
    Flail = 15,
    Greataxe = 16,
    GreatSpear = 17,
    Reaper = 18,
    Whip = 19,
    Fist = 20,
    Claw = 21,
    ColossalWeapon = 22,
    Torch = 23,
    Shield = 24,
}

impl ErWeapon {
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Whether this class can "power stance" — the same class in each hand giving a paired
    /// moveset. The rewrite gives 22 of 24 a paired moveset; the shield and the torch do not have
    /// one.
    pub fn pairs(self) -> bool {
        !matches!(self, ErWeapon::Shield | ErWeapon::Torch)
    }

    /// A guard (the left-hand button held) can block with this in hand only when it is a shield or
    /// a weapon held in both hands. Everything else in the left hand attacks instead.
    pub fn guards_when_two_handed(self) -> bool {
        !matches!(self, ErWeapon::Shield)
    }
}

/// How an armament is held, read from the WoW inventory type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hand {
    One,
    Two,
}

/// What the left hand holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Offhand {
    Empty,
    Shield,
    Torch,
    Weapon(ErWeapon),
}

/// Map a WoW `(class, subclass)` and how it is held to an ELDEN RING weapon class. Ranged
/// armaments and anything not a melee weapon fall back to the fists, since Tarnished combat is
/// melee only.
pub fn map_weapon(class: u32, subclass: u32, hand: Hand) -> ErWeapon {
    use wow_subclass as s;
    use ErWeapon as E;
    if class != wow_class::WEAPON {
        return E::Fist;
    }
    match subclass {
        s::DAGGER => E::Dagger,
        s::SWORD_1H => E::Longsword,
        s::SWORD_2H => E::Greatsword,
        s::AXE_1H => E::BattleAxe,
        s::AXE_2H => E::Greataxe,
        s::MACE_1H => E::Club,
        s::MACE_2H => E::GreatHammer,
        s::POLEARM => E::Halberd,
        // A WoW staff is a two-handed pole/blunt caster arm; the nearest heavy two-hander.
        s::STAFF => E::ColossalWeapon,
        s::FIST => E::Fist,
        // Bows, guns, crossbows, wands, thrown, fishing poles: no melee moveset — bare fists.
        s::BOW | s::GUN | s::CROSSBOW | s::WAND | s::THROWN | s::FISHING_POLE => E::Fist,
        _ => {
            // Unknown subclass: honour the hand so a two-handed oddity still reads as a big weapon.
            match hand {
                Hand::One => E::Longsword,
                Hand::Two => E::Greatsword,
            }
        }
    }
}

/// Map the WoW off-hand slot to a Tarnished off-hand. A shield (`ARMOR`/`SHIELD`) becomes the
/// guard shield; a held weapon becomes a left-hand armament; an empty off-hand is the bare hand.
pub fn map_offhand(class: Option<u32>, subclass: Option<u32>, hand: Hand) -> Offhand {
    match (class, subclass) {
        (None, _) => Offhand::Empty,
        (Some(wow_class::ARMOR), Some(wow_subclass::SHIELD)) => Offhand::Shield,
        (Some(c), Some(sub)) => Offhand::Weapon(map_weapon(c, sub, hand)),
        _ => Offhand::Empty,
    }
}

/// The full Tarnished loadout: the right-hand weapon, how it is held, and the off-hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Loadout {
    pub right: ErWeapon,
    pub right_hand: Hand,
    pub offhand: Offhand,
    /// The right weapon is being gripped in both hands (the off-hand is put away).
    pub two_handing: bool,
}

impl Loadout {
    pub fn new(right: ErWeapon, right_hand: Hand, offhand: Offhand) -> Self {
        Self {
            right,
            right_hand,
            offhand,
            two_handing: right_hand == Hand::Two,
        }
    }

    /// A paired ("power stance") moveset: the same pairable class in each hand, and not two-handing.
    pub fn paired(self) -> bool {
        if self.two_handing {
            return false;
        }
        matches!(self.offhand, Offhand::Weapon(left) if left == self.right && left.pairs())
    }

    /// Whether the left-hand button raises a guard rather than attacking: a shield, or a weapon
    /// (right or left) held in both hands.
    pub fn can_guard(self) -> bool {
        match self.offhand {
            Offhand::Shield => true,
            _ => self.two_handing && self.right.guards_when_two_handed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subclasses_map_to_the_expected_classes() {
        use wow_subclass as s;
        assert_eq!(map_weapon(2, s::DAGGER, Hand::One), ErWeapon::Dagger);
        assert_eq!(map_weapon(2, s::SWORD_1H, Hand::One), ErWeapon::Longsword);
        assert_eq!(map_weapon(2, s::SWORD_2H, Hand::Two), ErWeapon::Greatsword);
        assert_eq!(map_weapon(2, s::AXE_2H, Hand::Two), ErWeapon::Greataxe);
        assert_eq!(map_weapon(2, s::MACE_2H, Hand::Two), ErWeapon::GreatHammer);
        assert_eq!(map_weapon(2, s::POLEARM, Hand::Two), ErWeapon::Halberd);
        assert_eq!(map_weapon(2, s::FIST, Hand::One), ErWeapon::Fist);
    }

    #[test]
    fn ranged_and_non_weapons_fall_back_to_fists() {
        for sub in [
            wow_subclass::BOW,
            wow_subclass::GUN,
            wow_subclass::WAND,
            wow_subclass::THROWN,
        ] {
            assert_eq!(map_weapon(2, sub, Hand::One), ErWeapon::Fist);
        }
        // A non-weapon class is never a melee moveset.
        assert_eq!(map_weapon(4, 6, Hand::One), ErWeapon::Fist);
    }

    #[test]
    fn offhand_shield_and_weapon() {
        assert_eq!(map_offhand(None, None, Hand::One), Offhand::Empty);
        assert_eq!(map_offhand(Some(4), Some(6), Hand::One), Offhand::Shield);
        assert_eq!(
            map_offhand(Some(2), Some(wow_subclass::DAGGER), Hand::One),
            Offhand::Weapon(ErWeapon::Dagger)
        );
    }

    #[test]
    fn paired_needs_the_same_pairable_class_in_each_hand() {
        let paired = Loadout::new(
            ErWeapon::Longsword,
            Hand::One,
            Offhand::Weapon(ErWeapon::Longsword),
        );
        assert!(paired.paired());
        let mixed = Loadout::new(
            ErWeapon::Longsword,
            Hand::One,
            Offhand::Weapon(ErWeapon::Dagger),
        );
        assert!(!mixed.paired());
        // Two-handing cannot pair.
        let two = Loadout::new(ErWeapon::Greatsword, Hand::Two, Offhand::Empty);
        assert!(!two.paired());
    }

    #[test]
    fn guard_only_with_a_shield_or_a_two_handed_weapon() {
        assert!(Loadout::new(ErWeapon::Longsword, Hand::One, Offhand::Shield).can_guard());
        assert!(Loadout::new(ErWeapon::Greatsword, Hand::Two, Offhand::Empty).can_guard());
        // One-handed with no shield cannot raise a guard: the left button attacks.
        assert!(!Loadout::new(
            ErWeapon::Longsword,
            Hand::One,
            Offhand::Weapon(ErWeapon::Dagger)
        )
        .can_guard());
    }
}
