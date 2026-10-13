//! Fork interaction button. Feed a resolved nearby pick into the ordinary right-click router.

use super::*;
use benilla_world::collision::WorldCollision;
use bevy::window::PrimaryWindow;
use std::sync::Mutex;

#[derive(Resource, Default)]
pub(super) struct Interaction(pub Option<PressPick>);

#[allow(clippy::too_many_arguments)]
pub(super) fn poll(
    mut transport: Local<Mutex<benilla_controller::ControllerTransport>>,
    mut input: Local<benilla_controller::CombatInput>,
    mut request: ResMut<Interaction>,
    window: Query<&Window, With<PrimaryWindow>>,
    capture: Res<crate::ui_script::UiKeyboardCapture>,
    loading: Res<crate::loading_screen::LoadingScreen>,
    ground: Res<crate::spell::SpellTargeting>,
    payload: Res<crate::ui_script::CursorPayloadHeld>,
    skate: Res<crate::skate::SkateDrive>,
    objects: Query<(Entity, &Guid, &Transform, &ObjectStore)>,
    me: Query<Entity, With<SelfPlayer>>,
    reactions: ReactionInputs,
    quest: Res<crate::ui_quest::QuestGiver>,
    collision: WorldCollision,
) {
    let frame = input.sample(transport.get_mut().unwrap().poll());
    request.0 = None;
    if frame.pressed & 0x8000 == 0
        || !window.single().is_ok_and(|w| w.focused)
        || capture.typing
        || loading.covering()
        || ground.active()
        || payload.0
        || skate.active
    {
        return;
    }
    let Some((self_entity, _, self_tf, own)) = me.single().ok().and_then(|e| objects.get(e).ok())
    else {
        return;
    };
    if own.0.unit_is_dead()
        || own.0.unit_flags() & (crate::player::UNIT_FLAG_STUNNED | 0x0010_0000) != 0
    {
        return;
    }
    let mounted = own.0.unit_mount_display_id() != 0;
    let pick = objects
        .iter()
        .filter_map(|(entity, guid, transform, store)| {
            if entity == self_entity {
                return None;
            }
            let unit = store.is_unit();
            let loot = !mounted
                && if unit {
                    store.0.unit_is_dead()
                        && !store.0.unit_dynflag_dead()
                        && store.0.unit_lootable()
                } else {
                    store.0.object_type() == Some(benilla_protocol::messages::ObjectType::Corpse)
                        && store.0.corpse_lootable()
                };
            let kind = if loot {
                cursor_mode::CursorKind::LootAll
            } else if unit
                && !store.0.unit_is_dead()
                && can_interact(
                    Some(store),
                    reactions.factions.as_deref(),
                    &reactions.reputations,
                    Some(own),
                )
            {
                cursor_mode::service_cursor(store.0.unit_npc_flags(), quest.status(guid.0))?
            } else {
                return None;
            };
            let distance = transform.translation.distance_squared(self_tf.translation);
            let range = if loot && unit {
                (store.0.unit_combat_reach() + own.0.unit_combat_reach() + 1.333_33)
                    .max(5.0)
                    .powi(2)
            } else if loot {
                25.0
            } else {
                cursor_mode::SERVICE_RANGE_SQ
            };
            if !distance.is_finite() || distance > range {
                return None;
            }
            let delta = transform.translation - self_tf.translation;
            if let Ok(direction) = Dir3::new(delta) {
                if collision
                    .ray_body(self_tf.translation + Vec3::Y, direction, delta.length())
                    .is_some()
                {
                    return None;
                }
            }
            Some((
                !loot,
                distance,
                guid.0,
                PressPick {
                    hovered: if unit {
                        Hovered {
                            target: Some(entity),
                            guid: Some(guid.0),
                            ..Default::default()
                        }
                    } else {
                        Hovered {
                            corpse: Some(entity),
                            corpse_guid: Some(guid.0),
                            ..Default::default()
                        }
                    },
                    cursor: cursor_mode::WorldCursor {
                        kind,
                        unable: false,
                    },
                    ..Default::default()
                },
            ))
        })
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    request.0 = pick.map(|(_, _, _, pick)| pick);
}
