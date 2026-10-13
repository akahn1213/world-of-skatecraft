//! Match native damage requests to visible contacts, in either network arrival order.
//! No avoidance is sampled at windup. Unmatched work expires to the server's stock fallback.

use crate::defence::DefenceState;
use benilla_protocol::tarnished::Incoming;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contact {
    Melee(u64),
    Projectile { source: u64, spell: u32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub defence: DefenceState,
    pub source: [f32; 3],
    pub stamina_per_damage: f32,
}

/// Short bounded history handles a projectile arriving before the server's damage request.
#[derive(Default)]
pub struct Impacts {
    pending: VecDeque<(Contact, Incoming, f64)>,
    arrived: VecDeque<(Contact, Sample, f64)>,
}

impl Impacts {
    pub fn clear(&mut self) {
        self.pending.clear();
        self.arrived.clear();
    }

    pub fn expire(&mut self, now: f64) {
        self.pending.retain(|(_, _, at)| now - at < 2.0);
        self.arrived.retain(|(_, _, at)| now - at < 2.0);
    }

    pub fn request(&mut self, key: Contact, hit: Incoming, now: f64) -> Option<(Incoming, Sample)> {
        self.expire(now);
        if self
            .pending
            .iter()
            .any(|(_, old, _)| old.attack_id == hit.attack_id)
        {
            return None;
        }
        if let Some(i) = self.arrived.iter().position(|(k, _, _)| *k == key) {
            let (_, sample, _) = self.arrived.remove(i)?;
            return Some((hit, sample));
        }
        if self.pending.len() < 64 {
            self.pending.push_back((key, hit, now));
        }
        None
    }

    pub fn contact(
        &mut self,
        key: Contact,
        sample: Sample,
        now: f64,
    ) -> Option<(Incoming, Sample)> {
        self.expire(now);
        if let Some(i) = self.pending.iter().position(|(k, _, _)| *k == key) {
            let (_, hit, _) = self.pending.remove(i)?;
            return Some((hit, sample));
        }
        // Idle attack clips/extra melee keys must never grant a future swing an old iframe.
        if matches!(key, Contact::Projectile { .. }) {
            if self.arrived.len() == 64 {
                self.arrived.pop_front();
            }
            self.arrived.push_back((key, sample, now));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defence::{resolve, Absorption, Guard};
    use benilla_protocol::tarnished::DefenceOutcome;
    fn hit(id: u32, melee: bool) -> Incoming {
        Incoming {
            attack_id: id,
            source_guid: 9,
            school: if melee { 0 } else { 2 },
            amount: 100,
            melee,
            stamina_milli: None,
        }
    }
    fn sample(iframe: bool, blocking: bool) -> Sample {
        Sample {
            defence: DefenceState {
                iframe,
                guard: blocking.then_some(Guard {
                    stamina: 100.,
                    stamina_damage: 20.,
                    absorption: Absorption::default(),
                    source_facing_dot: 1.,
                }),
                ..Default::default()
            },
            source: [0.; 3],
            stamina_per_damage: 0.2,
        }
    }
    #[test]
    fn melee_checks_roll_and_guard_at_contact_not_windup() {
        for (iframe, blocking) in [(true, false), (false, true), (false, false)] {
            let mut q = Impacts::default();
            assert!(q.request(Contact::Melee(9), hit(1, true), 0.).is_none());
            let (hit, s) = q
                .contact(Contact::Melee(9), sample(iframe, blocking), 0.6)
                .unwrap();
            let result = resolve(&hit, &s.defence, 0.9);
            assert_eq!(matches!(result, DefenceOutcome::Iframe), iframe);
            assert_eq!(matches!(result, DefenceOutcome::Block { .. }), blocking);
            assert!(q
                .contact(Contact::Melee(9), sample(true, false), 0.61)
                .is_none());
            assert!(q.request(Contact::Melee(9), hit, 0.8).is_none());
        }
    }
    #[test]
    fn projectile_uses_arrival_state_in_either_packet_order() {
        for late in [false, true] {
            for blocking in [false, true] {
                let mut q = Impacts::default();
                let key = Contact::Projectile {
                    source: 9,
                    spell: 133,
                };
                let contact = sample(!blocking, blocking);
                let (hit, s) = if late {
                    assert!(q.contact(key, contact, 0.5).is_none());
                    q.request(key, hit(1, false), 0.7).unwrap()
                } else {
                    assert!(q.request(key, hit(1, false), 0.3).is_none());
                    q.contact(key, contact, 0.5).unwrap()
                };
                assert_eq!(
                    matches!(resolve(&hit, &s.defence, 0.9), DefenceOutcome::Block { .. }),
                    blocking
                );
                assert_eq!(
                    matches!(resolve(&hit, &s.defence, 0.9), DefenceOutcome::Iframe),
                    !blocking
                );
            }
        }
    }
    #[test]
    fn matching_is_ordered_scoped_and_bounded() {
        let mut q = Impacts::default();
        let key = Contact::Projectile {
            source: 9,
            spell: 133,
        };
        q.contact(key, sample(true, false), 0.);
        assert!(q
            .request(
                Contact::Projectile {
                    source: 10,
                    spell: 133
                },
                hit(2, false),
                0.1
            )
            .is_none());
        assert!(q
            .request(
                Contact::Projectile {
                    source: 9,
                    spell: 116
                },
                hit(3, false),
                0.1
            )
            .is_none());
        assert!(q.request(key, hit(4, false), 3.).is_none()); // expired contact
        q.clear();
        for id in 0..100 {
            q.request(key, hit(id, false), 4.);
        }
        assert_eq!(q.pending.len(), 64);
        assert_eq!(
            q.contact(key, sample(false, false), 4.1)
                .unwrap()
                .0
                .attack_id,
            0
        );
        q.expire(7.);
        assert!(q.pending.is_empty());
    }
}
