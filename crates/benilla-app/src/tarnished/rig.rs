//! Retarget baked ER model-space joints onto each race's tagged M2 skeleton.

use benilla_protocol::tarnished::Joint;
use benilla_tarnished::SolvedJoint;
use benilla_world::rig_anim::RigPose;
use bevy::prelude::*;

fn source(name: &str) -> &'static str {
    match name {
        "HIPS" => "Pelvis",
        "SPINE" => "Spine1",
        "SPINE2" => "Spine2",
        "NECK" => "Neck",
        "HEAD" => "Head",
        "LEFTSHOULDER" => "L_Clavicle",
        "LEFTARM" => "L_UpperArm",
        "LEFTFOREARM" => "L_Forearm",
        "LEFTHAND" => "L_Hand",
        "RIGHTSHOULDER" => "R_Clavicle",
        "RIGHTARM" => "R_UpperArm",
        "RIGHTFOREARM" => "R_Forearm",
        "RIGHTHAND" => "R_Hand",
        "LEFTUPLEG" => "L_Thigh",
        "LEFTLEG" => "L_Calf",
        "LEFTFOOT" => "L_Foot",
        "LEFTTOEBASE" => "L_Toe0",
        "RIGHTUPLEG" => "R_Thigh",
        "RIGHTLEG" => "R_Calf",
        "RIGHTFOOT" => "R_Foot",
        "RIGHTTOEBASE" => "R_Toe0",
        _ => "",
    }
}

#[derive(Component, Default)]
pub(super) struct Plan {
    parents: Vec<i16>,
    keys: Vec<i16>,
    pivots: Vec<Vec3>,
    binds: Vec<Vec3>,
    mappings: Vec<crate::skate::rig::Mapping>,
}

#[allow(clippy::type_complexity)]
pub(super) fn retarget(
    drive: Res<super::Drive>,
    mut units: Query<(
        Entity,
        &mut RigPose,
        Has<crate::net::Embodied>,
        Option<&super::replication::Remote>,
        Option<&mut Plan>,
    )>,
    mut commands: Commands,
) {
    for (entity, mut rig, local, remote, plan) in &mut units {
        let joints = if local && drive.active {
            drive.joints.as_slice()
        } else if let Some(remote) = remote.filter(|remote| remote.displayed_active) {
            remote.displayed.as_slice()
        } else {
            continue;
        };
        if joints.is_empty() {
            continue;
        }
        let Some(mut plan) = plan else {
            commands.entity(entity).insert(Plan::default());
            continue;
        };
        let remote_bind: Vec<SolvedJoint> = if local {
            Vec::new()
        } else {
            joints
                .iter()
                .filter_map(|joint| {
                    Some(SolvedJoint {
                        name: joint.name.strip_prefix("Bind_")?.to_owned(),
                        position: joint.translation,
                        rotation: joint.rotation,
                    })
                })
                .collect()
        };
        let bind = if local { &drive.bind } else { &remote_bind };
        if !bind.is_empty() {
            apply(&mut plan, &mut rig, bind, joints);
        }
    }
}

fn apply(plan: &mut Plan, rig: &mut RigPose, bind: &[SolvedJoint], joints: &[Joint]) {
    if plan.parents != rig.parents
        || plan.keys != rig.key_bones
        || plan.binds != rig.bind_translations()
    {
        plan.parents.clone_from(&rig.parents);
        plan.keys.clone_from(&rig.key_bones);
        let translations = rig.bind_translations();
        plan.binds = translations.to_vec();
        plan.pivots = vec![Vec3::ZERO; rig.parents.len()];
        for (i, translation) in translations.iter().enumerate() {
            let parent = usize::try_from(rig.parents[i]).ok().filter(|&p| p < i);
            plan.pivots[i] = *translation + parent.map_or(Vec3::ZERO, |p| plan.pivots[p]);
        }
        plan.mappings = crate::skate::rig::map_skeleton(&plan.parents, &plan.pivots, &plan.keys);
    }
    if rig.locals.len() != plan.pivots.len() {
        return;
    }
    let bind_at = |name| bind.iter().find(|joint| joint.name == source(name));
    let posed = |name| joints.iter().find(|joint| joint.name == source(name));
    let facing = Quat::from_rotation_y(std::f32::consts::PI);
    let height = plan
        .mappings
        .iter()
        .find(|mapping| mapping.skate == "HEAD")
        .map_or(1.0, |mapping| plan.pivots[mapping.bone].y);
    let reference_height = bind_at("HEAD")
        .map_or(height * 0.9144, |joint| joint.position[1])
        .max(0.01);
    let scale = height / reference_height;
    let mut skins = vec![None; plan.pivots.len()];
    for mapping in &plan.mappings {
        let Some(joint) = posed(mapping.skate) else {
            continue;
        };
        let to = Vec3::from_array(joint.translation);
        let rotation = if matches!(mapping.skate, "HEAD" | "LEFTHAND" | "RIGHTHAND") {
            let Some(reference) = bind_at(mapping.skate) else {
                continue;
            };
            let delta =
                Quat::from_array(joint.rotation) * Quat::from_array(reference.rotation).inverse();
            let fit = plan
                .mappings
                .iter()
                .find(|parent| parent.child.is_some_and(|(bone, _)| bone == mapping.bone))
                .and_then(|parent| {
                    let old =
                        (plan.pivots[mapping.bone] - plan.pivots[parent.bone]).try_normalize()?;
                    let new = facing
                        * (Vec3::from_array(reference.position)
                            - Vec3::from_array(bind_at(parent.skate)?.position));
                    Some(Quat::from_rotation_arc(old, new.try_normalize()?))
                })
                .unwrap_or(Quat::IDENTITY);
            delta * facing.inverse() * fit * facing
        } else {
            let segment = mapping
                .child
                .map(|(_, child)| (mapping.skate, child))
                .or_else(|| {
                    plan.mappings.iter().find_map(|parent| {
                        parent
                            .child
                            .filter(|(child, _)| *child == mapping.bone)
                            .map(|_| (parent.skate, mapping.skate))
                    })
                });
            segment
                .and_then(|(a, b)| {
                    let a_bone = plan.mappings.iter().find(|m| m.skate == a)?.bone;
                    let b_bone = plan.mappings.iter().find(|m| m.skate == b)?.bone;
                    let old = facing * (plan.pivots[b_bone] - plan.pivots[a_bone]);
                    let new = Vec3::from_array(posed(b)?.translation)
                        - Vec3::from_array(posed(a)?.translation);
                    Some(Quat::from_rotation_arc(
                        old.try_normalize()?,
                        new.try_normalize()?,
                    ))
                })
                .unwrap_or(Quat::IDENTITY)
        };
        let torso = match mapping.skate {
            "HIPS" => Some(("LEFTUPLEG", "RIGHTUPLEG")),
            "SPINE" | "SPINE2" => Some(("LEFTARM", "RIGHTARM")),
            _ => None,
        };
        let torso_turn = torso.and_then(|(left, right)| {
            let l = plan.mappings.iter().find(|m| m.skate == left)?.bone;
            let r = plan.mappings.iter().find(|m| m.skate == right)?.bone;
            let (child, child_name) = mapping.child?;
            let old_up = facing * (plan.pivots[child] - plan.pivots[mapping.bone]);
            let new_up = Vec3::from_array(posed(child_name)?.translation) - to;
            let old_across = facing * (plan.pivots[l] - plan.pivots[r]);
            let new_across = Vec3::from_array(posed(left)?.translation)
                - Vec3::from_array(posed(right)?.translation);
            let basis = |across: Vec3, up: Vec3| {
                let y = up.try_normalize()?;
                let z = across.cross(y).try_normalize()?;
                Some(Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)))
            };
            Some(basis(new_across, new_up)? * basis(old_across, old_up)?.inverse())
        });
        // A segment alone cannot carry axial hip/chest twist; paired limbs supply that axis.
        let rotation = facing * torso_turn.unwrap_or(rotation) * facing.inverse();
        skins[mapping.bone] = Some(Mat4::from_rotation_translation(
            rotation,
            facing * to * scale,
        ));
    }
    let mut models = vec![Mat4::IDENTITY; plan.pivots.len()];
    for i in 0..models.len() {
        let parent = usize::try_from(plan.parents[i]).ok().filter(|&p| p < i);
        models[i] = skins[i].unwrap_or_else(|| {
            parent.map_or(rig.locals[i].to_matrix(), |p| {
                models[p] * rig.locals[i].to_matrix()
            })
        });
        if skins[i].is_some() {
            let local = parent.map_or(models[i], |p| models[p].inverse() * models[i]);
            rig.locals[i] = Transform::from_matrix(local);
        }
    }
    rig.pose_dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_pose_reaches_mapped_bones_without_rescaling_crouches_or_replacing_gear_sockets() {
        let skeleton = benilla_assets::ModelSkeleton {
            joints: [
                (-1, Vec3::Y),
                (0, Vec3::Y * 0.4),
                (1, Vec3::Y * 0.4),
                (2, Vec3::Y * 0.2),
                (3, Vec3::X * 0.1),
            ]
            .into_iter()
            .map(|(parent, local_translation)| benilla_assets::ModelJoint {
                parent,
                local_translation,
                billboard: None,
                parent_arm: None,
            })
            .collect(),
            key_bones: vec![5, 4, -1, 6, -1],
            ..Default::default()
        };
        let mut rig = RigPose::new(Entity::PLACEHOLDER, &skeleton);
        let bind = vec![SolvedJoint {
            name: "Head".into(),
            position: [0.0, 2.0, 0.0],
            rotation: Quat::IDENTITY.to_array(),
        }];
        let joints: Vec<Joint> = [
            ("Pelvis", 0.3),
            ("Spine1", 0.5),
            ("Neck", 0.8),
            ("Head", 1.0),
        ]
        .into_iter()
        .map(|(name, y)| Joint {
            name: name.into(),
            translation: [0.0, y, 0.0],
            rotation: Quat::IDENTITY.to_array(),
            scale: [1.0; 3],
        })
        .collect();
        apply(&mut Plan::default(), &mut rig, &bind, &joints);
        let mut composed = Mat4::IDENTITY;
        for local in &rig.locals[..4] {
            composed *= local.to_matrix();
        }
        assert!(composed
            .transform_point3(Vec3::ZERO)
            .abs_diff_eq(Vec3::Y, 0.001));
        assert_eq!(rig.locals[4].translation, Vec3::X * 0.1);
        assert_eq!(rig.locals[4].rotation, Quat::IDENTITY);
        assert!(rig.pose_dirty);
    }

    #[test]
    fn m2_mapping_names_resolve_to_the_baked_skeleton() {
        for name in [
            "HIPS",
            "SPINE",
            "SPINE2",
            "NECK",
            "HEAD",
            "LEFTSHOULDER",
            "LEFTARM",
            "LEFTFOREARM",
            "LEFTHAND",
            "RIGHTSHOULDER",
            "RIGHTARM",
            "RIGHTFOREARM",
            "RIGHTHAND",
            "LEFTUPLEG",
            "LEFTLEG",
            "LEFTFOOT",
            "LEFTTOEBASE",
            "RIGHTUPLEG",
            "RIGHTLEG",
            "RIGHTFOOT",
            "RIGHTTOEBASE",
        ] {
            assert!(!source(name).is_empty(), "{name}");
        }
    }
}
