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

/// The equipped WoW armaments select the reference movesets.
pub struct ReferenceBackend {
    player: Player,
    level: Level,
    previous: BackendInput,
    action: Option<(data::ActionId, f32)>,
    epoch: u32,
    animation: Option<crate::animation::Animation>,
    pose: Vec<crate::SolvedJoint>,
    camera: crate::camera::Camera,
    view: Option<([f32; 3], [f32; 3])>,
    grounded: bool,
    feedback_window: Option<u64>,
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
            animation: None,
            pose: Vec::new(),
            camera: Default::default(),
            view: None,
            grounded: true,
            feedback_window: None,
        }
    }

    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let mut backend = Self::sword_and_shield();
        backend.animation = Some(crate::animation::Animation::load(path, &backend.player)?);
        backend.pose = backend.animation.as_ref().unwrap().bind();
        Ok(backend)
    }

    pub fn load_equipped(
        path: &std::path::Path,
        loadout: crate::weapon::Loadout,
    ) -> std::io::Result<Self> {
        let mut backend = Self::sword_and_shield();
        backend.equip(loadout).map_err(std::io::Error::other)?;
        backend.animation = Some(crate::animation::Animation::load(path, &backend.player)?);
        backend.pose = backend.animation.as_ref().unwrap().bind();
        Ok(backend)
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

    fn action(&self) -> Option<(data::ActionId, f32)> {
        if let Some(air) = self.player.air_attack {
            return Some((
                data::ActionId::Attack(
                    air.moveset,
                    if air.paired {
                        data::AttackKind::PairedJumpLand
                    } else if air.heavy {
                        data::AttackKind::JumpHeavyLand
                    } else {
                        data::AttackKind::JumpLightLand
                    },
                ),
                air.f,
            ));
        }
        match self.player.state {
            State::Act(action) => Some((action.id, action.f)),
            _ => None,
        }
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
    fn equip(&mut self, loadout: crate::weapon::Loadout) -> Result<(), String> {
        use crate::weapon::Offhand;
        // The pinned extractor's indices are the protocol's armament codes (weapon.rs).
        let mut player = Player::new(self.player.pos);
        player.yaw = self.player.yaw;
        player.hp = self.player.hp;
        player.stamina = self.player.stamina;
        player.weapon = usize::from(loadout.right.code());
        player.left = match loadout.offhand {
            Offhand::Empty => data::FIST,
            Offhand::Shield => data::SHIELD,
            Offhand::Torch => data::TORCH,
            Offhand::Weapon(weapon) => usize::from(weapon.code()),
        };
        player.grip = if loadout.two_handing {
            data::Grip::TwoHandRight
        } else {
            data::Grip::OneHand
        };
        if let Some(animation) = &mut self.animation {
            animation
                .equip(&player)
                .map_err(|error| error.to_string())?;
        }
        self.player = player;
        self.action = None;
        self.previous = BackendInput::default();
        self.feedback_window = None;
        self.epoch = self.epoch.wrapping_add(1);
        Ok(())
    }

    fn suspend_attacks(&mut self) {
        self.player.buffer = None;
        self.player.air_attack = None;
        self.player.guarding = false;
        self.player.hit_stop = 0.0;
        if matches!(self.player.state, State::Act(a) if matches!(a.id, data::ActionId::Attack(..)))
        {
            self.player.state = State::Ground;
        }
    }

    fn step(&mut self, input: BackendInput) {
        // One call is exactly one tick. The host accumulates real time; dt never changes ER windows.
        let camera_yaw = self.camera.step(&self.player, &input);
        let inp = Input {
            mv: Vec2::new(input.move_x, input.move_y),
            cam_yaw: camera_yaw,
            light: button(input.light_attack, self.previous.light_attack),
            heavy: button(input.heavy_attack, self.previous.heavy_attack),
            guard: button(input.guard, self.previous.guard),
            dodge: button(input.dodge, self.previous.dodge),
            ..Default::default()
        };
        // The host owns support/vertical travel; upstream still selects its own fall and landing.
        let floor = self.player.pos.y;
        if !self.grounded && !matches!(self.player.state, State::Air(_)) {
            self.player.state = State::Air(crate::sim::player::Air {
                vel: self.player.move_dir * self.player.speed,
                jumped: false,
                f: 0.0,
            });
        }
        self.player.pos.y = if self.grounded { 0.0 } else { 1.0 };
        let target = input.target.map(|p| Vec3::from_array(p) - Vec3::Y * floor);
        self.player.step(&inp, &self.level, target, true);
        self.player.pos.y = floor;
        let next = self.action();
        if next.is_some_and(|(id, frame)| {
            self.action
                .is_none_or(|(old, previous)| old != id || frame < previous)
        }) {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.action = next;
        self.previous = input;
        if let Some(animation) = &mut self.animation {
            self.pose = animation.step(&self.player);
        }
        self.view = Some(self.camera.follow(&self.player));
    }

    fn active_swing(&self) -> Option<Swing> {
        self.player.active_hit().map(|hit| Swing {
            radius: hit.radius,
            sweep: hit.sweep,
        })
    }

    fn attack_timing(&self) -> Option<crate::AttackTiming> {
        let (id, frame) = self.action()?;
        let def = id.def();
        let (from, to) = if let Some(air) = self.player.air_attack {
            (air.def.from, air.def.to)
        } else {
            let hit = def
                .hits
                .iter()
                .find(|hit| frame >= hit.from && frame < hit.to)?;
            (hit.from, hit.to)
        };
        Some(crate::AttackTiming {
            frame,
            from,
            to,
            total: def.total,
        })
    }

    fn active_motion_value(&self) -> Option<f32> {
        self.player.active_hit().map(|hit| hit.mv)
    }

    fn active_stamina_damage(&self) -> Option<f32> {
        // AtkParam's guard correction is extracted per hit (pinned extract.py::judge).
        self.player
            .active_hit()
            .map(|hit| hit.attack * hit.guard_damage / 4.0)
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
            weapon_class: self.player.moveset().weapon,
            contact_window: u64::from(self.epoch) << 32,
            ..Default::default()
        };
        if let Some((id, frame)) = self.action() {
            state.action = match id {
                data::ActionId::Attack(moveset, kind) => {
                    state.attack_kind = kind as u8;
                    state.weapon_class = moveset.weapon;
                    if let Some(index) = id
                        .def()
                        .hits
                        .iter()
                        .position(|h| frame >= h.from && frame < h.to)
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
        if matches!(outcome, O::Block { .. }) {
            // Fork policy: ordinary damage never interrupts an attack or movement. WoW owns HP;
            // only an actual block invokes the reference guard reaction and stamina/guard break.
            self.receive_hit(0.0, stamina_damage, Vec3::from_array(source));
        }
    }

    fn stock_stamina_damage(&self, damage_ratio: f32) -> f32 {
        // The pinned dummy's damage/stamina pairs share 4:1 (dummy.rs::DState::Strike).
        self.player.moveset().info().attack * damage_ratio.max(0.0) / 4.0
    }

    fn pose(&self) -> &[crate::SolvedJoint] {
        &self.pose
    }

    fn bind_pose(&self) -> Vec<crate::SolvedJoint> {
        self.animation
            .as_ref()
            .map_or_else(Vec::new, |animation| animation.bind())
    }

    fn camera(&self) -> Option<([f32; 3], [f32; 3])> {
        self.view
    }

    fn set_grounded(&mut self, grounded: bool) {
        self.grounded = grounded;
    }

    fn contact(&mut self) {
        let window = self.state().contact_window;
        if self.feedback_window != Some(window) {
            if let Some(hit) = self.player.active_hit() {
                // sim::World::player_attacks supplies this exact feedback. Its one-dummy mark_hit
                // is replaced by the host's per-target ledger so a sweep can reach several bodies.
                self.player.hit_stop = hit.stop * data::HIT_STOP_SCALE;
                self.feedback_window = Some(window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equipped_movesets_cover_melee_classes_and_both_hands() {
        use crate::weapon::{Hand, Loadout, Offhand};
        use crate::ErWeapon as W;
        for (weapon, hand) in [
            (W::Dagger, Hand::One),
            (W::Longsword, Hand::One),
            (W::Greatsword, Hand::Two),
            (W::BattleAxe, Hand::One),
            (W::Greataxe, Hand::Two),
            (W::Club, Hand::One),
            (W::GreatHammer, Hand::Two),
            (W::Halberd, Hand::Two),
            (W::Twinblade, Hand::Two),
            (W::Fist, Hand::One),
        ] {
            for heavy in [false, true] {
                let mut backend = ReferenceBackend::sword_and_shield();
                backend
                    .equip(Loadout::new(weapon, hand, Offhand::Empty))
                    .unwrap();
                backend.step(BackendInput {
                    light_attack: !heavy,
                    heavy_attack: heavy,
                    ..Default::default()
                });
                let mut contact = false;
                for _ in 0..600 {
                    backend.step(BackendInput::default());
                    if backend.active_swing().is_some() {
                        assert_eq!(backend.state().weapon_class, weapon.code());
                        contact = true;
                        break;
                    }
                }
                assert!(contact, "{weapon:?}, heavy={heavy}");
            }
        }
        for left in [W::Longsword, W::Dagger] {
            let mut backend = ReferenceBackend::sword_and_shield();
            backend
                .equip(Loadout::new(W::Longsword, Hand::One, Offhand::Weapon(left)))
                .unwrap();
            backend.step(BackendInput {
                guard: true,
                ..Default::default()
            });
            let state = backend.state();
            assert_eq!(state.weapon_class, left.code());
            assert!(if left == W::Longsword {
                state.attack_kind >= 26
            } else {
                (20..=25).contains(&state.attack_kind)
            });
            assert!(backend.guard(10.0, 1.0).is_none());
        }
    }

    #[test]
    fn equipment_changes_and_casts_cancel_old_swings_without_refilling_stamina() {
        let mut backend = ReferenceBackend::sword_and_shield();
        backend.player.stamina = 30.0;
        backend.player.hp = 50.0;
        backend.step(BackendInput {
            heavy_attack: true,
            ..Default::default()
        });
        let stamina = backend.stamina();
        backend.equip(crate::weapon::Loadout::unarmed()).unwrap();
        assert_eq!(backend.stamina(), stamina);
        assert_eq!(backend.player.hp, 50.0);
        assert!(backend.active_swing().is_none());
        backend.step(BackendInput {
            light_attack: true,
            ..Default::default()
        });
        backend.suspend_attacks();
        assert!(backend.action().is_none());
        for _ in 0..120 {
            backend.step(BackendInput::default());
            assert!(
                backend.active_swing().is_none(),
                "a cast must discard queued melee input"
            );
        }
    }

    #[test]
    fn host_support_drives_upstream_falling_without_integrating_height_twice() {
        let mut backend = ReferenceBackend::sword_and_shield();
        backend.set_transform(Vec3::new(0.0, 30.0, 0.0), 0.0);
        backend.set_grounded(false);
        backend.step(BackendInput::default());
        assert!(matches!(backend.player.state, State::Air(_)));
        assert_eq!(backend.position().y, 30.0);
        backend.step(BackendInput::default());
        assert!(matches!(backend.player.state, State::Air(air) if air.f > 0.0));
        assert_eq!(backend.position().y, 30.0);
    }

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
    fn ordinary_damage_does_not_interrupt_light_or_heavy_attacks() {
        use benilla_protocol::tarnished::DefenceOutcome;
        for heavy in [false, true] {
            let mut backend = ReferenceBackend::sword_and_shield();
            backend.step(BackendInput {
                light_attack: !heavy,
                heavy_attack: heavy,
                ..Default::default()
            });
            let action = backend.action().expect("attack started");
            assert!(matches!(action.0, data::ActionId::Attack(..)));
            let stamina = backend.stamina();
            backend.react(DefenceOutcome::Hit { taken: 50 }, 20.0, [0.0, 0.0, 1.0]);
            assert_eq!(
                backend.action(),
                Some(action),
                "an incoming hit must not replace the swing with Hurt"
            );
            assert_eq!(backend.stamina(), stamina);
            backend.step(BackendInput::default());
            assert!(
                matches!(backend.player.state, State::Act(a) if matches!(a.id, data::ActionId::Attack(..)))
            );
        }
    }

    #[test]
    fn single_button_press_exposes_contact_during_extracted_active_frames() {
        for heavy in [false, true] {
            let mut backend = ReferenceBackend::sword_and_shield();
            backend.step(BackendInput {
                light_attack: !heavy,
                heavy_attack: heavy,
                ..Default::default()
            });
            let mut contacted = false;
            for _ in 0..600 {
                // Release immediately: damage must never require holding through the recovery.
                backend.step(BackendInput::default());
                if backend.active_swing().is_some() {
                    let timing = backend.attack_timing().unwrap();
                    assert!(timing.frame >= timing.from && timing.frame < timing.to);
                    assert!(
                        timing.frame < timing.total,
                        "contact happens before animation completion"
                    );
                    assert!(
                        timing.frame - timing.from <= data::DF,
                        "first active simulation tick"
                    );
                    contacted = true;
                    break;
                }
            }
            assert!(contacted, "a tapped attack must reach its hit window");
        }
    }

    #[test]
    fn blocking_still_spends_stamina_and_breaks_an_exhausted_guard() {
        use benilla_protocol::tarnished::DefenceOutcome;
        for broken in [false, true] {
            let mut backend = ReferenceBackend::sword_and_shield();
            for _ in 0..60 {
                backend.step(BackendInput {
                    guard: true,
                    ..Default::default()
                });
            }
            backend.player.stamina = if broken { 1.0 } else { 100.0 };
            backend.react(
                DefenceOutcome::Block {
                    absorbed: 50,
                    stamina_cost: 11,
                    guard_broken: broken,
                    taken: 0,
                },
                20.0,
                [0.0, 0.0, 1.0],
            );
            assert_eq!(backend.stamina(), if broken { 0.0 } else { 89.0 });
            assert_eq!(
                backend.action().unwrap().0,
                if broken {
                    data::ActionId::GuardBreak
                } else {
                    data::ActionId::GuardHit
                }
            );
        }
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
