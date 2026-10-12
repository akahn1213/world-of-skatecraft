//! Adapter for the pinned rewrite. WoW world collision remains the host's responsibility.

use glam::{Vec2, Vec3};

use crate::sim::{
    data,
    level::Level,
    player::{Button, Input, Player},
};
use crate::{
    attack::Swing,
    defence::{Absorption, Guard},
    Backend, BackendInput,
};

/// Sword/shield only until the two-client loop is verified.
pub struct ReferenceBackend {
    player: Player,
    level: Level,
    previous: BackendInput,
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
            cam_yaw: self.player.yaw,
            light: button(input.light_attack, self.previous.light_attack),
            heavy: button(input.heavy_attack, self.previous.heavy_attack),
            guard: button(input.guard, self.previous.guard),
            dodge: button(input.dodge, self.previous.dodge),
            ..Default::default()
        };
        self.player.step(&inp, &self.level, None, true);
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
