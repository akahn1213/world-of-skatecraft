//! Fork lock-on controls: one deliberate right-stick flick selects one angular neighbour.

use glam::{Vec2, Vec3};

#[derive(Default)]
pub struct SwitchStick {
    latched: bool,
}

impl SwitchStick {
    pub fn sample(&mut self, stick: [f32; 2]) -> Option<i8> {
        let [x, y] = stick;
        if x.abs() < 0.3 && y.abs() < 0.3 {
            self.latched = false;
        }
        if !self.latched && x.abs() >= 0.65 && x.abs() > y.abs() {
            self.latched = true;
            Some(if x > 0.0 { 1 } else { -1 })
        } else {
            None
        }
    }
}

/// Candidates have already passed the host's attackability, range and visibility checks.
/// Right is clockwise around the player viewed from above; choose the smallest angular step.
/// No candidate on that side leaves the current lock alone.
pub fn neighbour(
    origin: Vec3,
    current: Vec3,
    direction: i8,
    candidates: impl IntoIterator<Item = (u64, Vec3)>,
) -> Option<u64> {
    let relative = |p: Vec3| Vec2::new(p.x - origin.x, p.z - origin.z);
    let from = relative(current).try_normalize()?;
    candidates
        .into_iter()
        .filter_map(|(guid, position)| {
            let offset = relative(position);
            let to = offset.try_normalize()?;
            let angle = from.perp_dot(to).atan2(from.dot(to)) * f32::from(direction);
            (angle.is_finite() && angle > 0.01).then_some((guid, angle, offset.length_squared()))
        })
        .min_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then(a.2.total_cmp(&b.2))
                .then(a.0.cmp(&b.0))
        })
        .map(|(guid, ..)| guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flick_requires_recentering_and_ignores_vertical_camera_motion() {
        let mut stick = SwitchStick::default();
        assert_eq!(stick.sample([0.2, 0.0]), None);
        assert_eq!(stick.sample([0.7, 0.9]), None);
        assert_eq!(stick.sample([0.8, 0.1]), Some(1));
        assert_eq!(stick.sample([1.0, 0.0]), None);
        assert_eq!(stick.sample([-1.0, 0.0]), None);
        assert_eq!(stick.sample([0.0, 0.0]), None);
        assert_eq!(stick.sample([-0.8, 0.0]), Some(-1));
    }

    #[test]
    fn direction_uses_target_bearings_not_distance_or_world_axes() {
        for yaw in [0.0, 1.4, -2.7] {
            let rotation = glam::Quat::from_rotation_y(yaw);
            let origin = Vec3::new(10.0, 20.0, 30.0);
            let at = |x, z| origin + rotation * Vec3::new(x, 0.0, z);
            let candidates = [
                (1, at(-2.0, -5.0)),
                (2, at(2.0, -5.0)),
                (3, at(2.0, -1.0)),
                (4, origin),
            ];
            assert_eq!(neighbour(origin, at(0.0, -5.0), -1, candidates), Some(1));
            assert_eq!(neighbour(origin, at(0.0, -5.0), 1, candidates), Some(2));
            assert_eq!(
                neighbour(origin, at(0.0, -5.0), 1, [(1, at(-1.0, -5.0))]),
                None
            );
        }
    }
}
