//! Fork gameplay: swept skating contacts in metres, with one hit per pass.
use glam::{Quat, Vec3};
use std::collections::{HashMap, HashSet};

pub const MIN_SPEED: f32 = 2.0;
pub const HIT_COOLDOWN: f64 = 1.0;

#[derive(Clone, Copy)]
pub struct Motion {
    pub position: Vec3,
    pub speed: f32,
    pub time: f64,
    pub radius: f32,
    pub height: f32,
    pub flipping: bool,
}
#[derive(Clone, Copy)]
pub struct Body {
    pub guid: u64,
    pub position: Vec3,
    pub radius: f32,
    pub height: f32,
}
#[derive(Default)]
pub struct Contacts {
    previous: Option<Motion>,
    touching: HashSet<u64>,
    hits: HashMap<u64, f64>,
}

fn segment_distance(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> f32 {
    let (u, v, w) = (b - a, d - c, a - c);
    let (uu, uv, vv, uw, vw) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
    let denom = uu * vv - uv * uv;
    let mut s = if denom > 1e-8 {
        ((uv * vw - vv * uw) / denom).clamp(0., 1.)
    } else {
        0.
    };
    let t = if vv > 1e-8 {
        ((uv * s + vw) / vv).clamp(0., 1.)
    } else {
        0.
    };
    if uu > 1e-8 {
        s = ((uv * t - uw) / uu).clamp(0., 1.);
    }
    (w + u * s - v * t).length()
}
fn touches(from: Vec3, motion: Motion, body: Body, padding: f32) -> bool {
    // Minkowski difference of two vertical bodies: sweep the feet against the extended axis.
    let bottom = body.position - Vec3::Y * motion.height;
    let top = body.position + Vec3::Y * body.height;
    segment_distance(from, motion.position, bottom, top) <= motion.radius + body.radius + padding
}
impl Contacts {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn sample(&mut self, motion: Motion, bodies: &[Body]) -> Vec<u64> {
        if !motion.position.is_finite() || !motion.speed.is_finite() || !motion.time.is_finite() {
            self.clear();
            return Vec::new();
        }
        let previous = self.previous.replace(motion);
        let valid = previous.is_some_and(|old| {
            let dt = motion.time - old.time;
            dt > 0.
                && dt <= 0.25
                && old.position.distance(motion.position)
                    <= (motion.speed.max(old.speed) * dt as f32 * 1.5 + 0.5).min(8.)
        });
        self.hits.retain(|_, at| motion.time - *at < HIT_COOLDOWN);
        let mut next = HashSet::new();
        let mut hits = Vec::new();
        for &body in bodies {
            if !body.position.is_finite() {
                continue;
            }
            // A little separation hysteresis stops radius-edge jitter repeatedly re-arming a hit.
            if touches(motion.position, motion, body, 0.15) {
                next.insert(body.guid);
            }
            if valid
                && motion.speed >= MIN_SPEED
                && !self.touching.contains(&body.guid)
                && !self.hits.contains_key(&body.guid)
                && touches(previous.unwrap().position, motion, body, 0.)
            {
                self.hits.insert(body.guid, motion.time);
                hits.push(body.guid);
            }
        }
        self.touching = next;
        hits
    }
}

/// The pinned engine bridge exposes the solved board/hips, but no live flip-state flag.
/// Measure rotation relative to the hips: roll and shove-it yaw count, ollie pitch and
/// whole-body spins do not. A stale trick label is deliberately never used.
#[derive(Default)]
pub struct Flip {
    previous: Option<(Quat, f64)>,
}
impl Flip {
    pub fn sample(
        &mut self,
        airborne: bool,
        board_relative_to_hips: Option<Quat>,
        time: f64,
    ) -> bool {
        let Some(q) = board_relative_to_hips
            .filter(|q| airborne && q.is_finite() && q.length_squared() > 0.5)
        else {
            self.previous = None;
            return false;
        };
        let q = q.normalize();
        let Some((old, at)) = self.previous.replace((q, time)) else {
            return false;
        };
        let dt = (time - at) as f32;
        if !(0.001..=0.1).contains(&dt) {
            return false;
        }
        let mut delta = q * old.inverse();
        if delta.w < 0. {
            delta = -delta;
        }
        let rotation = delta.to_scaled_axis() / dt;
        let roll = rotation.dot(old * Vec3::Z);
        let shove = rotation.dot(old * Vec3::Y);
        roll.hypot(shove) >= 6.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn motion(x: f32, t: f64) -> Motion {
        Motion {
            position: Vec3::new(x, 0., 0.),
            speed: 12.,
            time: t,
            radius: 0.6,
            height: 1.8,
            flipping: false,
        }
    }
    fn body() -> Body {
        Body {
            guid: 1,
            position: Vec3::ZERO,
            radius: 0.4,
            height: 1.8,
        }
    }
    #[test]
    fn crossing_hits_once_and_requires_leaving() {
        let mut c = Contacts::default();
        assert!(c.sample(motion(-2., 0.), &[body()]).is_empty());
        assert_eq!(c.sample(motion(0., 0.2), &[body()]), [1]);
        assert!(c.sample(motion(0.1, 0.3), &[body()]).is_empty());
        assert!(c.sample(motion(0.2, 1.5), &[body()]).is_empty());
        c.sample(motion(2., 1.7), &[body()]);
        assert_eq!(c.sample(motion(0., 1.9), &[body()]), [1]);
    }
    #[test]
    fn fast_passes_hit_between_frames_but_teleports_do_not() {
        let mut c = Contacts::default();
        c.sample(motion(-2., 0.), &[body()]);
        assert_eq!(c.sample(motion(2., 0.24), &[body()]), [1]);
        c.clear();
        c.sample(motion(-40., 0.), &[body()]);
        assert!(c.sample(motion(40., 0.1), &[body()]).is_empty());
    }
    #[test]
    fn slow_spawn_and_vertical_near_misses_do_no_damage() {
        let mut c = Contacts::default();
        assert!(c.sample(motion(0., 0.), &[body()]).is_empty());
        c.clear();
        c.sample(motion(-2., 0.), &[body()]);
        let mut slow = motion(-0.9, 0.1);
        slow.speed = 1.;
        assert!(c.sample(slow, &[body()]).is_empty());
        c.clear();
        let high = Body {
            position: Vec3::Y * 8.,
            ..body()
        };
        c.sample(motion(-2., 0.), &[high]);
        assert!(c.sample(motion(0., 0.2), &[high]).is_empty());
    }
    #[test]
    fn actual_flip_rotation_counts_but_ollie_ground_and_caught_board_do_not() {
        for axis in [Vec3::Y, Vec3::Z] {
            let mut f = Flip::default();
            assert!(!f.sample(true, Some(Quat::IDENTITY), 0.));
            assert!(f.sample(true, Some(Quat::from_axis_angle(axis, 0.2)), 0.02));
            assert!(!f.sample(true, Some(Quat::from_axis_angle(axis, 0.2)), 0.04));
            assert!(!f.sample(false, Some(Quat::from_axis_angle(axis, 0.4)), 0.06));
        }
        let mut f = Flip::default();
        f.sample(true, Some(Quat::IDENTITY), 0.);
        assert!(!f.sample(true, Some(Quat::from_rotation_x(0.3)), 0.02));
    }
}
