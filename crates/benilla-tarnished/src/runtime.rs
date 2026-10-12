//! Fixed-rate host clock and once-per-target contact accounting, independent of engine and assets.

use std::collections::{HashSet, VecDeque};

use benilla_protocol::tarnished::{AttackTarget, AttackVerdict, DefenceVerdict, Incoming};

use crate::{Backend, BackendInput};

pub const TICK: f64 = 1.0 / 60.0;

#[derive(Default)]
pub struct InputBuffer {
    latest: BackendInput,
    sampled: u8,
    held: u8,
    edges: VecDeque<u8>,
    mouse: [f32; 2],
}

impl InputBuffer {
    pub fn sample(&mut self, input: BackendInput) {
        self.latest = input;
        self.mouse[0] += input.mouse_x;
        self.mouse[1] += input.mouse_y;
        let buttons = u8::from(input.light_attack)
            | (u8::from(input.heavy_attack) << 1)
            | (u8::from(input.guard) << 2)
            | (u8::from(input.dodge) << 3);
        if buttons != self.sampled {
            // Preserve control transitions through render frames that contain no simulation tick.
            if self.edges.len() < 64 {
                self.edges.push_back(buttons);
            }
            self.sampled = buttons;
        }
    }

    pub fn tick(&mut self) -> BackendInput {
        if let Some(buttons) = self.edges.pop_front() {
            self.held = buttons;
        } else {
            self.held = self.sampled;
        }
        let mouse = std::mem::take(&mut self.mouse);
        BackendInput {
            light_attack: self.held & 1 != 0,
            heavy_attack: self.held & 2 != 0,
            guard: self.held & 4 != 0,
            dodge: self.held & 8 != 0,
            dt: TICK as f32,
            mouse_x: mouse[0],
            mouse_y: mouse[1],
            ..self.latest
        }
    }
}

#[derive(Default)]
pub struct Clock {
    remaining: f64,
}

impl Clock {
    /// Retain fractional time, but never replay a multi-second stall as a burst of combat.
    pub fn advance(&mut self, delta: f64) -> usize {
        if !delta.is_finite() || delta < 0.0 {
            return 0;
        }
        self.remaining += delta.min(0.25);
        let ticks = (self.remaining / TICK).floor() as usize;
        self.remaining -= ticks as f64 * TICK;
        ticks
    }
}

#[derive(Default)]
pub struct Combat {
    pub clock: Clock,
    sequence: u32,
    attack_id: u32,
    window: Option<u64>,
    contacts: HashSet<u64>,
    answered: HashSet<u32>,
    answer_order: VecDeque<u32>,
}

impl Combat {
    pub fn begin_bout(&mut self) {
        self.window = None;
        self.contacts.clear();
    }

    pub fn step(&mut self, backend: &mut dyn Backend, input: BackendInput) {
        backend.step(input);
        let state = backend.state();
        if self.window != Some(state.contact_window) {
            self.window = Some(state.contact_window);
            self.contacts.clear();
            self.attack_id = self.attack_id.wrapping_add(1);
        }
    }

    fn next_sequence(&mut self) -> u32 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    pub fn attack(
        &mut self,
        backend: &dyn Backend,
        sample_ms: u32,
        targets: Vec<AttackTarget>,
    ) -> Option<AttackVerdict> {
        let mut fresh = Vec::new();
        for target in targets {
            if self.contacts.len() >= benilla_protocol::tarnished::MAX_TARGETS {
                break;
            }
            if target.guid != 0 && self.contacts.insert(target.guid) {
                fresh.push(target);
            }
        }
        if fresh.is_empty() {
            return None;
        }
        let state = backend.state();
        Some(AttackVerdict {
            sequence: self.next_sequence(),
            attack_id: self.attack_id,
            swing_ms: sample_ms,
            weapon_class: state.weapon_class,
            attack_kind: state.attack_kind,
            targets: fresh,
            stamina_milli: backend
                .active_stamina_damage()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| (value * 1000.0).round() as u32),
        })
    }

    pub fn defence(
        &mut self,
        incoming: Incoming,
        outcome: benilla_protocol::tarnished::DefenceOutcome,
    ) -> Option<DefenceVerdict> {
        let verdict = DefenceVerdict {
            sequence: self.next_sequence(),
            attack_id: incoming.attack_id,
            source_guid: incoming.source_guid,
            outcome,
        };
        if verdict.validate_for(&incoming).is_err() || !self.answered.insert(incoming.attack_id) {
            return None;
        }
        self.answer_order.push_back(incoming.attack_id);
        // Tokens are opaque (the server scrambles its counter), not numerically ordered.
        if self.answer_order.len() > 1024 {
            if let Some(oldest) = self.answer_order.pop_front() {
                self.answered.remove(&oldest);
            }
        }
        Some(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_motion_accumulates_without_ticks_and_is_not_repeated_by_catchup() {
        let mut input = InputBuffer::default();
        input.sample(BackendInput {
            mouse_x: 2.0,
            mouse_y: 1.0,
            ..Default::default()
        });
        input.sample(BackendInput {
            mouse_x: 3.0,
            mouse_y: -1.0,
            ..Default::default()
        });
        let tick = input.tick();
        assert_eq!([tick.mouse_x, tick.mouse_y], [5.0, 0.0]);
        let tick = input.tick();
        assert_eq!([tick.mouse_x, tick.mouse_y], [0.0, 0.0]);
    }
    use benilla_protocol::tarnished::DefenceOutcome;

    #[derive(Default)]
    struct TestBackend {
        window: u64,
    }

    impl Backend for TestBackend {
        fn step(&mut self, _: BackendInput) {}
        fn active_swing(&self) -> Option<crate::attack::Swing> {
            None
        }
        fn active_motion_value(&self) -> Option<f32> {
            None
        }
        fn active_stamina_damage(&self) -> Option<f32> {
            None
        }
        fn guard(&self, _: f32, _: f32) -> Option<crate::defence::Guard> {
            None
        }
        fn invincible(&self) -> bool {
            false
        }
        fn set_transform(&mut self, _: [f32; 3], _: f32) {}
        fn state(&self) -> crate::BackendState {
            crate::BackendState {
                contact_window: self.window,
                weapon_class: 1,
                ..Default::default()
            }
        }
        fn set_health(&mut self, _: f32) {}
        fn react(&mut self, _: DefenceOutcome, _: f32, _: [f32; 3]) {}
        fn stock_stamina_damage(&self, _: f32) -> f32 {
            0.0
        }
    }

    fn target(guid: u64) -> AttackTarget {
        AttackTarget {
            guid,
            damage: 40,
            flags: 0,
        }
    }

    #[test]
    fn each_target_is_hit_once_across_all_ticks_of_one_window() {
        let mut combat = Combat::default();
        let mut backend = TestBackend { window: 1 };
        combat.step(&mut backend, BackendInput::default());
        let first = combat.attack(&backend, 1, vec![target(7)]).unwrap();
        combat.step(&mut backend, BackendInput::default());
        let later = combat
            .attack(&backend, 2, vec![target(7), target(8)])
            .unwrap();
        assert_eq!(later.targets, vec![target(8)]);
        assert_eq!(first.attack_id, later.attack_id);
        assert!(later.sequence > first.sequence);
        assert!(combat
            .attack(&backend, 3, vec![target(7), target(8)])
            .is_none());
        backend.window = 2;
        combat.step(&mut backend, BackendInput::default());
        let next = combat.attack(&backend, 4, vec![target(7)]).unwrap();
        assert_ne!(next.attack_id, first.attack_id);
        combat.begin_bout();
        backend.window = 1;
        combat.step(&mut backend, BackendInput::default());
        let reentered = combat.attack(&backend, 5, vec![target(7)]).unwrap();
        assert!(reentered.sequence > next.sequence);
        assert!(reentered.attack_id > next.attack_id);
    }

    #[test]
    fn opaque_tokens_have_bounded_replay_retention() {
        let mut combat = Combat::default();
        for n in 0..2048 {
            let token = n ^ 0x8abc_ff31;
            let incoming = Incoming {
                attack_id: token,
                source_guid: 3,
                school: 0,
                amount: 0,
                melee: true,
                stamina_milli: None,
            };
            assert!(combat.defence(incoming, DefenceOutcome::Iframe).is_some());
            assert!(combat.defence(incoming, DefenceOutcome::Iframe).is_none());
        }
        assert_eq!(combat.answered.len(), 1024);
        assert_eq!(combat.answer_order.len(), 1024);
    }

    #[test]
    fn fractional_frames_are_not_lost_and_stalls_are_bounded() {
        let mut clock = Clock::default();
        assert_eq!(clock.advance(TICK / 2.0), 0);
        assert_eq!(clock.advance(TICK / 2.0), 1);
        assert_eq!(clock.advance(10.0), 15);
        assert_eq!(clock.advance(f64::NAN), 0);
    }

    #[test]
    fn button_edges_survive_render_frames_without_a_fixed_tick() {
        let mut buffer = InputBuffer::default();
        buffer.sample(BackendInput {
            light_attack: true,
            ..Default::default()
        });
        buffer.sample(BackendInput::default());
        assert!(buffer.tick().light_attack);
        assert!(!buffer.tick().light_attack);
        assert!(!buffer.tick().light_attack);
        buffer.sample(BackendInput {
            dodge: true,
            move_y: 1.0,
            ..Default::default()
        });
        for _ in 0..30 {
            let input = buffer.tick();
            assert!(input.dodge);
            assert_eq!(input.move_y, 1.0);
            assert_eq!(input.dt, TICK as f32);
        }
    }

    #[test]
    fn correlated_defence_is_applied_only_once() {
        let mut combat = Combat::default();
        let incoming = Incoming {
            attack_id: 19,
            source_guid: 7,
            school: 0,
            amount: 40,
            melee: true,
            stamina_milli: None,
        };
        assert!(combat
            .defence(incoming, DefenceOutcome::Hit { taken: 39 })
            .is_none());
        assert!(combat
            .defence(incoming, DefenceOutcome::Hit { taken: 40 })
            .is_some());
        assert!(combat.defence(incoming, DefenceOutcome::Iframe).is_none());
    }
}
