//! Adapter for the pinned rewrite. WoW world collision remains the host's responsibility.

use glam::{Vec2, Vec3};

use crate::sim::{
    data,
    level::Level,
    player::{Button, Input, Player, State},
};
use crate::{
    attack::Swing,
    defence::{Absorption, Guard},
    Backend, BackendInput, BackendState,
};

/// Sword/shield only until the two-client loop is verified.
pub struct ReferenceBackend {
    player: Player,
    level: Level,
    previous: BackendInput,
    action: Option<(data::ActionId, f32)>,
    epoch: u32,
}

impl ReferenceBackend {
    pub fn sword_and_shield() -> Self {
        let mut player = Player::new(Vec3::ZERO);
        player.weapon = data::DEFAULT_WEAPON;
        player.left = data::SHIELD;
        player.grip = data::Grip::OneHand;
        Self {
            player,
            level: Level::flat(),
            previous: BackendInput::default(),
            action: None,
            epoch: 0,
        }
    }

    pub fn position(&self) -> Vec3 {
        self.player.pos
    }
    pub fn yaw(&self) -> f32 {
        self.player.yaw
    }
    pub fn stamina(&self) -> f32 {
        self.player.stamina
    }

    /// Synchronize with the host's collision-resolved transform before stepping.
    pub fn set_transform(&mut self, position: Vec3, yaw: f32) {
        self.player.pos = position;
        self.player.yaw = yaw;
    }

    /// Apply the reference guard reaction once, after passive avoidance has failed.
    /// `amount` is post-armour WoW damage, `stamina_damage` is the incoming reference action's.
    pub fn receive_hit(&mut self, amount: f32, stamina_damage: f32, source: Vec3) {
        self.player.receive_hit(
            &crate::sim::player::Incoming {
                damage: amount,
                stamina: stamina_damage,
                from: source,
                low: false,
                level: data::HurtLevel::Small,
            },
            &self.level,
        );
    }
}

fn button(held: bool, previous: bool) -> Button {
    Button {
        held,
        pressed: held && !previous,
        released: !held && previous,
    }
}

impl Backend for ReferenceBackend {
    fn step(&mut self, input: BackendInput) {
        // One call is exactly one tick. The host accumulates real time; dt never changes ER windows.
        let inp = Input {
            mv: Vec2::new(input.move_x, input.move_y),
            cam_yaw: input.camera_yaw,
            light: button(input.light_attack, self.previous.light_attack),
            heavy: button(input.heavy_attack, self.previous.heavy_attack),
            guard: button(input.guard, self.previous.guard),
            dodge: button(input.dodge, self.previous.dodge),
            ..Default::default()
        };
        // The host owns terrain height; flatten the reference locally at the resolved feet.
        let floor = self.player.pos.y;
        self.player.pos.y = 0.0;
        let target = input.target.map(|p| Vec3::from_array(p) - Vec3::Y * floor);
        self.player.step(&inp, &self.level, target, true);
        self.player.pos.y += floor;
        let next = match self.player.state {
            State::Act(a) => Some((a.id, a.f)),
            _ => None,
        };
        if next.is_some_and(|(id, frame)| {
            self.action
                .is_none_or(|(old, previous)| old != id || frame < previous)
        }) {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.action = next;
        self.previous = input;
    }

    fn active_swing(&self) -> Option<Swing> {
        self.player.active_hit().map(|hit| Swing {
            radius: hit.radius,
            sweep: hit.sweep,
        })
    }

    fn active_motion_value(&self) -> Option<f32> {
        self.player.active_hit().map(|hit| hit.mv)
    }

    fn guard(&self, stamina_damage: f32, source_facing_dot: f32) -> Option<Guard> {
        self.player.guard_up().then_some(Guard {
            stamina: self.player.stamina,
            stamina_damage,
            source_facing_dot,
            // receive_hit fully negates a raised guard's hit, including guard break.
            absorption: Absorption {
                physical: 1.0,
                magic: 1.0,
            },
        })
    }

    fn invincible(&self) -> bool {
        self.player.invincible()
    }

    fn set_transform(&mut self, position: [f32; 3], yaw: f32) {
        self.player.pos = Vec3::from_array(position);
        self.player.yaw = yaw;
    }

    fn state(&self) -> BackendState {
        let mut state = BackendState {
            position: self.player.pos.to_array(),
            yaw: self.player.yaw,
            stamina: self.player.stamina,
            weapon_class: crate::ErWeapon::Longsword.code(),
            contact_window: u64::from(self.epoch) << 32,
            ..Default::default()
        };
        if let State::Act(a) = self.player.state {
            state.action = match a.id {
                data::ActionId::Attack(moveset, kind) => {
                    state.attack_kind = kind as u8;
                    if usize::from(moveset.weapon) == data::SHIELD {
                        state.weapon_class = crate::ErWeapon::Shield.code();
                    }
                    if let Some(index) =
                        a.id.def()
                            .hits
                            .iter()
                            .position(|h| a.f >= h.from && a.f < h.to)
                    {
                        state.contact_window |= index as u64;
                    }
                    100 + kind as u16
                }
                data::ActionId::Roll(..)
                | data::ActionId::CrouchRoll(..)
                | data::ActionId::Backstep => 2,
                data::ActionId::GuardHit => 3,
                data::ActionId::GuardBreak => 4,
                data::ActionId::Hurt(..) => 5,
                _ => 0,
            };
        }
        state
    }

    fn set_health(&mut self, fraction: f32) {
        // Player::new supplies the reference HP scale; only WoW's health decides death.
        self.player.hp = data::MAX_HP * fraction.clamp(0.001, 1.0);
    }

    fn react(
        &mut self,
        outcome: benilla_protocol::tarnished::DefenceOutcome,
        stamina_damage: f32,
        source: [f32; 3],
    ) {
        use benilla_protocol::tarnished::DefenceOutcome as O;
        if matches!(outcome, O::Hit { .. } | O::Block { .. }) {
            // WoW owns HP. A zero-HP-damage incoming still produces the reference hurt/guard action.
            self.receive_hit(0.0, stamina_damage, Vec3::from_array(source));
        }
    }

    fn stock_stamina_damage(&self, damage_ratio: f32) -> f32 {
        // The pinned dummy's damage/stamina pairs share 4:1 (dummy.rs::DState::Strike).
        data::WEAPONS[data::DEFAULT_WEAPON].attack * damage_ratio.max(0.0) / 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shield_requires_held_guard_and_reference_raise_time() {
        let mut backend = ReferenceBackend::sword_and_shield();
        assert!(backend.guard(20.0, 1.0).is_none());
        backend.step(BackendInput {
            guard: true,
            ..Default::default()
        });
        assert!(backend.guard(20.0, 1.0).is_none());
        let ticks = (data::GUARD_RAISE_FRAMES / data::DF).ceil() as usize;
        for _ in 0..ticks {
            backend.step(BackendInput {
                guard: true,
                ..Default::default()
            });
        }
        assert!(backend.guard(20.0, 1.0).is_some());
        backend.step(BackendInput::default());
        assert!(backend.guard(20.0, 1.0).is_none());
    }

    #[test]
    fn buttons_have_press_hold_and_release_edges() {
        let press = button(true, false);
        assert!(press.held && press.pressed && !press.released);
        let hold = button(true, true);
        assert!(hold.held && !hold.pressed && !hold.released);
        let release = button(false, true);
        assert!(!release.held && !release.pressed && release.released);
    }
}
