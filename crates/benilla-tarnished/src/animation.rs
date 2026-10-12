//! Engine-free boundary around the pinned sandbox's animation selection and arm layers.

#![allow(dead_code)]

use glam::{Mat3, Quat, Vec3};

use crate::sim::{
    data::*,
    player::{Player, State},
};
use crate::SolvedJoint;

mod baked {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/anim.rs"));
}
use baked::{vec3, Clips};

#[derive(Default)]
pub(crate) struct Animator {
    clip: String,
    frame: f32,
    looped: bool,
    from: Vec<f32>,
    fade: f32,
    fade_len: f32,
    shown: Vec<f32>,
    phase: f32,
    guard: f32,
    carry: f32,
    swap: f32,
    rest: Option<(String, f32)>,
    ran: f32,
    ran_dir: u32,
    still: bool,
    crouched: bool,
}

include!(concat!(env!("OUT_DIR"), "/animation_mechanism.rs"));

pub(crate) struct Animation {
    clips: Clips,
    rig: Animator,
    elapsed: f32,
    bind: Vec<SolvedJoint>,
}

impl Animation {
    pub(crate) fn load(path: &std::path::Path, player: &Player) -> std::io::Result<Self> {
        let clips = Clips::load(path)?;
        let required = [
            "Pelvis",
            "Spine1",
            "Spine2",
            "Neck",
            "Head",
            "L_Clavicle",
            "L_UpperArm",
            "L_Forearm",
            "L_Hand",
            "R_Clavicle",
            "R_UpperArm",
            "R_Forearm",
            "R_Hand",
            "L_Thigh",
            "L_Calf",
            "L_Foot",
            "L_Toe0",
            "R_Thigh",
            "R_Calf",
            "R_Foot",
            "R_Toe0",
            "R_Weapon",
            "L_Weapon",
        ];
        if required
            .iter()
            .any(|name| !clips.joints.iter().any(|j| j == name))
            || ["Head", "R_Weapon", "L_Weapon"]
                .iter()
                .any(|name| !clips.oriented.iter().any(|j| j == name))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ER animation skeleton is incomplete",
            ));
        }
        let mut sources = vec![
            clip_name(WEAPONS[player.weapon].stance[0], IDLE),
            clip_name(WEAPONS[player.left].stance[0], GUARD),
            AIR_LOOP.to_owned(),
            FALL_START.to_owned(),
        ];
        for direction in 0..4 {
            sources.push(clip_name(0, WALK + direction));
            sources.push(clip_name(0, RUN + direction));
        }
        sources.push(clip_name(0, SPRINT));
        for direction in [Dir::Front, Dir::Back, Dir::Left, Dir::Right] {
            for id in [
                ActionId::Roll(player.load, direction),
                ActionId::Hurt(HurtLevel::Small, direction),
            ] {
                sources.push(id.def().source.to_owned());
            }
        }
        for id in [
            ActionId::Backstep,
            ActionId::SprintStop,
            ActionId::GuardHit,
            ActionId::GuardBreak,
            ActionId::LandLight,
            ActionId::LandRun,
            ActionId::LandSprint,
            ActionId::LandHeavy,
            ActionId::LandFall,
        ] {
            sources.push(id.def().source.to_owned());
        }
        let moveset = Moveset {
            weapon: player.weapon as u8,
            two_hand: false,
        };
        for kind in [
            AttackKind::Light1,
            AttackKind::Light2,
            AttackKind::Light3,
            AttackKind::Light4,
            AttackKind::Light5,
            AttackKind::Light6,
            AttackKind::RunLight,
            AttackKind::RunHeavy,
            AttackKind::RollAttack,
            AttackKind::BackstepAttack,
            AttackKind::Heavy1Charge,
            AttackKind::Heavy1,
            AttackKind::Heavy2Charge,
            AttackKind::Heavy2,
            AttackKind::GuardCounter,
            AttackKind::JumpLightLand,
            AttackKind::JumpLightLandShort,
            AttackKind::JumpHeavyLand,
            AttackKind::JumpHeavyLandShort,
        ] {
            if let Some(def) = moveset.attack(kind) {
                sources.push(def.source.to_owned());
            }
        }
        for heavy in [false, true] {
            if let Some(def) = moveset.air(heavy) {
                sources.push(def.source.to_owned());
            }
        }
        if let Some(missing) = sources.iter().find(|name| clips.get(name).is_none()) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
                format!("ER animation clip {missing} is missing; bake against the same pinned action table")));
        }
        let mut rig = Animator {
            ran: f32::MAX,
            still: true,
            ..Default::default()
        };
        let pose = sample_pose(player, &mut rig, &clips, 0.0, 0.0, 0.0);
        if pose.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ER sword stance clip is missing",
            ));
        }
        let bind = solve(&clips, &pose);
        Ok(Self {
            clips,
            rig,
            elapsed: 0.0,
            bind,
        })
    }

    pub(crate) fn bind(&self) -> Vec<SolvedJoint> {
        self.bind.clone()
    }

    pub(crate) fn step(&mut self, player: &Player) -> Vec<SolvedJoint> {
        let dt = 1.0 / 60.0;
        self.elapsed += dt;
        let pose = sample_pose(player, &mut self.rig, &self.clips, self.elapsed, dt, 0.0);
        if pose.is_empty() {
            return Vec::new();
        }
        solve(&self.clips, &pose)
    }
}

fn solve(clips: &Clips, pose: &[f32]) -> Vec<SolvedJoint> {
    clips
        .joints
        .iter()
        .map(|name| {
            let position = vec3(pose, clips.joint(name));
            let oriented = match name.as_str() {
                "L_Hand" => Some("L_Weapon"),
                "R_Hand" => Some("R_Weapon"),
                _ if clips.oriented.contains(name) => Some(name.as_str()),
                _ => None,
            };
            let rotation = oriented.map_or(Quat::IDENTITY, |joint| {
                let at = clips.axes(joint);
                let x = vec3(pose, at).normalize_or(Vec3::X);
                let z = x.cross(vec3(pose, at + 3)).normalize_or(Vec3::Z);
                Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z))
            });
            SolvedJoint {
                name: name.clone(),
                position: position.to_array(),
                rotation: rotation.to_array(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(frames: u32, value: f32) -> Vec<u8> {
        let mut data = b"ERAN".to_vec();
        for count in [1_u32, 0, 1] {
            data.extend(count.to_le_bytes());
        }
        for name in ["Pelvis", "test"] {
            data.push(name.len() as u8);
            data.extend(name.bytes());
        }
        // Joint names precede clip metadata; with one joint the second name is the clip.
        data.extend(0.0_f32.to_le_bytes());
        data.extend(frames.to_le_bytes());
        for i in 0..frames {
            for sample in [value + i as f32, 0.0, 0.0] {
                data.extend(sample.to_le_bytes());
            }
        }
        data
    }

    #[test]
    fn pinned_sampler_interpolates_baked_positions_and_clamps_actions() {
        let clips = Clips::from_bytes(&fixture(2, 2.0)).unwrap();
        let clip = clips.get("test").unwrap();
        let mut pose = Vec::new();
        clips.sample(clip, 0.5, false, &mut pose);
        assert_eq!(pose, [2.5, 0.0, 0.0]);
        clips.sample(clip, 10.0, false, &mut pose);
        assert_eq!(pose, [3.0, 0.0, 0.0]);
        clips.sample(clip, 1.5, true, &mut pose);
        assert_eq!(pose, [2.5, 0.0, 0.0]);
    }

    #[test]
    fn malformed_bakes_fail_before_sampling() {
        assert!(Clips::from_bytes(&fixture(0, 0.0)).is_err());
        assert!(Clips::from_bytes(&fixture(1, f32::NAN)).is_err());
        let mut bytes = fixture(2, 0.0);
        bytes.pop();
        assert!(Clips::from_bytes(&bytes).is_err());
        let mut bytes = fixture(1, 0.0);
        bytes.push(0);
        assert!(Clips::from_bytes(&bytes).is_err());
    }
}
