//! Fork-only contact combat; the client reports collisions, never a chosen damage amount.
use super::{replication::Link, SkateDrive, METERS_PER_YARD};
use crate::net::{Guid, NetCommands, ObjectStore, SelfPlayer};
use benilla_skate::{Body, Contacts};
use bevy::prelude::*;

#[derive(Resource, Default)]
struct CollisionCombat(Contacts);
pub(super) fn register(app: &mut App) {
    app.init_resource::<CollisionCombat>().add_systems(
        Update,
        contacts
            .after(crate::player::PlayerControlSet)
            .in_set(crate::char_select::InWorldGated),
    );
}
fn contacts(
    mut drive: ResMut<SkateDrive>,
    mut combat: ResMut<CollisionCombat>,
    mut link: ResMut<Link>,
    net: Res<NetCommands>,
    reactions: crate::target::ReactionInputs,
    local: Query<
        (
            &Guid,
            &ObjectStore,
            Option<&crate::entities::CollisionHeight>,
        ),
        With<SelfPlayer>,
    >,
    units: Query<(
        &Guid,
        &Transform,
        &ObjectStore,
        Option<&crate::entities::CollisionHeight>,
    )>,
) {
    let Ok((self_id, store, height)) = local.single() else {
        combat.0.clear();
        drive.contacts.clear();
        return;
    };
    if std::mem::take(&mut drive.reset_contacts) {
        combat.0.clear();
    }
    if !drive.active || drive.contacts.is_empty() {
        // An engine pause/wipeout cannot sweep a stale segment into a new victim.
        if !drive.active {
            combat.0.clear();
        }
        return;
    }
    let bodies: Vec<_> = units
        .iter()
        .filter_map(|(guid, t, s, height)| {
            if guid.0 == self_id.0
                || s.0.unit_is_dead()
                || !crate::target::can_attack(
                    Some(s),
                    reactions.factions.as_deref(),
                    &reactions.reputations,
                    Some(store),
                )
            {
                return None;
            }
            Some(Body {
                guid: guid.0,
                position: t.translation * METERS_PER_YARD,
                radius: s.0.unit_bounding_radius().max(0.2) * METERS_PER_YARD,
                height: height.map_or(crate::player::CAPSULE_HEIGHT, |h| h.0) * METERS_PER_YARD,
            })
        })
        .collect();
    for mut motion in drive.contacts.drain(..) {
        motion.height = height.map_or(crate::player::CAPSULE_HEIGHT, |h| h.0) * METERS_PER_YARD;
        for target in combat.0.sample(motion, &bodies) {
            link.impact(target, motion.speed, motion.flipping, &net);
        }
    }
}
