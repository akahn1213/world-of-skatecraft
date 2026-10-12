//! The pinned orbit/follow mechanism, with world obstruction owned by the host.

#![allow(dead_code)]

use crate::sim::{angle_diff, dir_of, player::Player, yaw_of};
use glam::{Vec2, Vec3};

include!(concat!(env!("OUT_DIR"), "/camera_mechanism.rs"));

#[derive(Default)]
pub(crate) struct Camera {
    rig: CamRig,
    initialized: bool,
}

impl Camera {
    pub(crate) fn step(&mut self, player: &Player, input: &crate::BackendInput) -> f32 {
        if !self.initialized {
            self.rig.yaw = input.camera_yaw;
            self.initialized = true;
        }

        let dt = 1.0 / 60.0;
        let stick = Vec2::new(input.look_x, -input.look_y);
        let delta = if stick.length() > 0.15 {
            stick * STICK_SPEED * dt
        } else {
            Vec2::ZERO
        } + Vec2::new(input.mouse_x, input.mouse_y) * MOUSE_SENSITIVITY;
        look(
            &mut self.rig,
            player,
            input.target.map(Vec3::from_array),
            delta,
            dt,
        );
        self.rig.yaw
    }

    pub(crate) fn follow(&mut self, player: &Player) -> ([f32; 3], [f32; 3]) {
        follow(&mut self.rig, player, 1.0 / 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upstream_lock_camera_converges_and_keeps_authored_boom_length() {
        let player = Player::new(Vec3::ZERO);
        let mut camera = Camera::default();
        let input = crate::BackendInput {
            target: Some([4.0, 0.0, 0.0]),
            ..Default::default()
        };
        for _ in 0..120 {
            camera.step(&player, &input);
        }
        assert!((camera.rig.yaw - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        let (eye, pivot) = camera.follow(&player);
        assert!((Vec3::from_array(eye).distance(Vec3::from_array(pivot)) - DISTANCE).abs() < 0.001);
        assert!((camera.rig.pitch - LOCKED_PITCH).abs() < 0.001);
    }
}
