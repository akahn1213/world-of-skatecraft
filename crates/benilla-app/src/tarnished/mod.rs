//! Opt-in fork combat: the pinned 60 Hz reference drives intent, WoW owns terrain and health.

pub(crate) mod replication;
mod rig;

use std::collections::VecDeque;
use std::sync::Mutex;

use benilla_protocol::tarnished::{Incoming, Packet};
use benilla_tarnished::{
    attack::{resolve_swing, Body},
    defence::{self, DefenceState},
    runtime::{Combat, InputBuffer, TICK},
    scale::{Outcome, WowWeapon},
    Backend, BackendInput,
};
use benilla_world::{collision::WorldCollision, schedule::WorldStage};
use bevy::{prelude::*, window::PrimaryWindow};

use crate::net::{ClientCommand, Guid, GuidIndex, NetCommands, ObjectStore, SelfGuid};

#[derive(Resource, Default)]
pub(crate) struct Drive {
    pub(crate) active: bool,
    pub(crate) pending: bool,
    pub(crate) position: Vec3,
    pub(crate) yaw: f32,
    pub(crate) speed: f32,
    pub(crate) velocity: Vec3,
    pub(crate) action: u16,
    pub(crate) guarding: bool,
    pub(crate) stamina: f32,
    pub(crate) lock: Option<u64>,
    pub(crate) falling: bool,
    pub(crate) camera: Option<Transform>,
    pub(crate) joints: Vec<benilla_protocol::tarnished::Joint>,
    pub(crate) bind: Vec<benilla_tarnished::SolvedJoint>,
}

impl Drive {
    pub(crate) fn owns_key(&self, key: KeyCode) -> bool {
        (self.active || self.pending)
            && matches!(
                key,
                KeyCode::KeyW
                    | KeyCode::KeyA
                    | KeyCode::KeyS
                    | KeyCode::KeyD
                    | KeyCode::KeyU
                    | KeyCode::KeyI
                    | KeyCode::KeyO
                    | KeyCode::Space
                    | KeyCode::F8
                    | KeyCode::F9
            )
    }
}

#[derive(Resource, Default)]
struct Host {
    backend: Option<Box<dyn Backend>>,
    combat: Combat,
    incoming: VecDeque<Incoming>,
    vertical_speed: f32,
    grounded: bool,
    steep_support: bool,
    input: InputBuffer,
    transport: Mutex<benilla_controller::ControllerTransport>,
    controller: benilla_controller::CombatInput,
}

fn backend() -> Option<Box<dyn Backend>> {
    #[cfg(feature = "er-data")]
    {
        let path = std::env::var_os("ER_ANIMATIONS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(".setup/tarnished/assets/player_anims.bin")
            });
        match benilla_tarnished::reference::ReferenceBackend::load(&path) {
            Ok(backend) => Some(Box::new(backend)),
            Err(error) => {
                warn!("tarnished: cannot load {}: {error}; run python3 setup/tarnished.py (both actions and animations are required)", path.display());
                None
            }
        }
    }
    #[cfg(not(feature = "er-data"))]
    {
        None
    }
}

pub(crate) struct TarnishedPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TarnishedInputSet;

impl Plugin for TarnishedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drive>().init_resource::<Host>();
        replication::register(app);
        app.add_systems(
            Update,
            tick.in_set(WorldStage::Input)
                .in_set(TarnishedInputSet)
                .before(crate::player::PlayerControlSet),
        );
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, draw_hud.after(crate::player::PlayerControlSet));
        app.add_systems(Startup, prepare);
        app.add_systems(
            PostUpdate,
            rig::retarget
                .in_set(benilla_world::rig_anim::PosePost)
                .after(crate::creature_anim::apply_body_twist)
                .after(benilla_world::rig_anim::apply_global_sequences)
                .after(crate::entities::conform_units),
        );
    }
}

fn prepare(mut host: ResMut<Host>, mut drive: ResMut<Drive>) {
    host.backend = backend();
    if let Some(backend) = &host.backend {
        drive.bind = backend.bind_pose();
    }
}

fn send(packet: Packet, commands: &NetCommands) {
    if let Ok(body) = packet.encode() {
        let _ = commands.0.send(ClientCommand::Tarnished(body));
    }
}

fn stop(host: &mut Host, drive: &mut Drive, link: &mut replication::Link, net: &NetCommands) {
    if drive.active || link.requested {
        drive.active = false;
        drive.pending = false;
        link.requested = false;
        drive.lock = None;
        drive.joints.clear();
        drive.camera = None;
        host.backend = None;
        host.incoming.clear();
        host.vertical_speed = 0.0;
        host.input = Default::default();
        // Off is ordered, never replaceable by a later pose in the visual mailbox.
        link.publish_off(drive, net);
        let _ = net.0.send(ClientCommand::TarnishedMode(false));
    }
}

fn reference_position(position: Vec3) -> [f32; 3] {
    (position * 0.9144).to_array()
}

fn reference_yaw(yaw: f32) -> f32 {
    // Reference dir_of(yaw) is (+sin, 0, +cos), WoW/Bevy faces (-sin, 0, -cos).
    yaw + std::f32::consts::PI
}

fn equipped(store: &ObjectStore, items: &crate::items::Items, net: &NetCommands) -> bool {
    let template = |slot| {
        let entry = store.0.player_visible_item_entry(slot)?;
        items.template(entry, store.0.player_inv_slot(slot).unwrap_or(0), net)
    };
    // The first vertical loop matches the server's approved warrior/sword/shield slice.
    store.0.unit_class() == Some(1)
        && template(15).is_some_and(|item| item.class == 2 && item.subclass == 7)
        && template(16).is_some_and(|item| item.class == 4 && item.subclass == 6)
        && !crate::items::is_disarmed(store)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn tick(
    time: Res<Time>,
    input_sources: (
        Res<ButtonInput<KeyCode>>,
        Res<ButtonInput<MouseButton>>,
        Res<bevy::input::mouse::AccumulatedMouseMotion>,
    ),
    window: Query<&Window, With<PrimaryWindow>>,
    capture: Res<crate::ui_script::UiKeyboardCapture>,
    player: Res<crate::player::Player>,
    skate: Res<crate::skate::SkateDrive>,
    capsule: Res<crate::player::PlayerCapsule>,
    collision: WorldCollision,
    camera: Query<&crate::player::camera::FlyCam>,
    objects: Query<(
        &Guid,
        &Transform,
        &ObjectStore,
        Option<&crate::entities::CollisionHeight>,
    )>,
    context: (
        Res<SelfGuid>,
        Res<GuidIndex>,
        Res<crate::items::Items>,
        crate::target::ReactionInputs,
        Res<crate::target::Selection>,
        Res<crate::loading_screen::LoadingScreen>,
    ),
    mut state: (ResMut<Host>, ResMut<Drive>, ResMut<replication::Link>),
    net: Res<NetCommands>,
) {
    let (keys, mouse, motion) = input_sources;
    let (host, drive, link) = (&mut *state.0, &mut *state.1, &mut *state.2);
    link.discover(time.elapsed_secs_f64(), &net);
    let (self_guid, index, items, reactions, selection, loading) = context;
    let self_object = self_guid
        .0
        .and_then(|guid| index.0.get(&guid))
        .and_then(|entity| objects.get(*entity).ok());
    let focused = window.single().is_ok_and(|w| w.focused);
    let allowed = player.may_tarnish()
        && !skate.active
        && !player.swimming
        && focused
        && !capture.typing
        && !loading.covering()
        && self_object.is_some_and(|(_, _, s, _)| {
            !s.0.unit_is_dead()
                && s.0.unit_mount_display_id() == 0
                && s.0.unit_flags() & (crate::player::UNIT_FLAG_STUNNED | 0x0010_0000) == 0
                && equipped(s, &items, &net)
        });
    let raw = host.transport.lock().unwrap().poll();
    let pad = host.controller.sample(raw);
    let buttons = pad.buttons;
    let pressed = pad.pressed;
    let toggle = keys.just_pressed(KeyCode::F8) || pressed & 0x0010 != 0;
    if (drive.active || link.requested)
        && (!allowed
            || !link.acknowledged
            || toggle
            || (link.requested && time.elapsed_secs_f64() - link.request_time > 2.0))
    {
        stop(host, drive, link, &net);
    } else if toggle {
        if !benilla_tarnished::er_data_compiled() {
            warn!("{}", benilla_tarnished::unavailable());
        } else if !allowed || !link.acknowledged {
            warn!("tarnished: requires a negotiated server and a living, unmounted warrior with sword/shield");
        } else {
            host.backend = backend();
            if host.backend.is_none() {
                return;
            }
            drive.bind = host.backend.as_ref().unwrap().bind_pose();
            drive.position = player.pos;
            drive.yaw = player.face_yaw();
            host.combat.clock = Default::default();
            host.combat.begin_bout();
            host.grounded = player.move_flags() & crate::creature_anim::move_flags::FALLING == 0;
            link.begin(time.elapsed_secs_f64());
            drive.pending = true;
            // The self pose echoed by the server grants the stance before either controller changes.
            drive.active = host.backend.is_some();
            link.publish(drive, time.elapsed_secs_f64(), &net, true);
            drive.active = false;
            info!("tarnished: requesting sword/shield stance");
        }
    }
    if !drive.active {
        host.incoming.clear();
        return;
    }
    let Some((self_id, _, self_store, _)) = self_object else {
        return;
    };
    let lock_pressed = keys.just_pressed(KeyCode::F9) || pressed & 0x0080 != 0;
    if lock_pressed {
        drive.lock = if drive.lock.is_some() {
            None
        } else {
            selection.guid
        };
    }
    let locked = drive
        .lock
        .and_then(|guid| {
            index
                .0
                .get(&guid)
                .and_then(|entity| objects.get(*entity).ok())
        })
        .filter(|(guid, transform, store, _)| {
            guid.0 != self_id.0
                && !store.0.unit_is_dead()
                && drive.position.distance(transform.translation) < 25.0 / 0.9144
                && crate::target::can_attack(
                    Some(store),
                    reactions.factions.as_deref(),
                    &reactions.reputations,
                    Some(self_store),
                )
                && collision
                    .ray_body(
                        drive.position + Vec3::Y,
                        Dir3::new(transform.translation - drive.position).unwrap_or(Dir3::NEG_Z),
                        drive.position.distance(transform.translation),
                    )
                    .is_none()
        });
    if locked.is_none() {
        drive.lock = None;
    }
    let cam_yaw = camera.single().map_or(drive.yaw, |cam| cam.heading());
    let combat_key = |key| {
        keys.pressed(key)
            && !capture.consumed.contains(&key)
            && ![
                KeyCode::AltLeft,
                KeyCode::AltRight,
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
                KeyCode::SuperLeft,
                KeyCode::SuperRight,
            ]
            .iter()
            .any(|key| keys.pressed(*key))
    };
    let keyboard_axis = |positive, negative| {
        u8::from(combat_key(positive)) as f32 - u8::from(combat_key(negative)) as f32
    };
    let input = BackendInput {
        move_x: if pad.movement == [0.0; 2] {
            keyboard_axis(KeyCode::KeyD, KeyCode::KeyA)
        } else {
            pad.movement[0]
        },
        move_y: if pad.movement == [0.0; 2] {
            keyboard_axis(KeyCode::KeyW, KeyCode::KeyS)
        } else {
            pad.movement[1]
        },
        look_x: pad.look[0],
        look_y: pad.look[1],
        mouse_x: if mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Left) {
            motion.delta.x
        } else {
            0.0
        },
        mouse_y: if mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Left) {
            motion.delta.y
        } else {
            0.0
        },
        light_attack: buttons & 0x0200 != 0 || combat_key(KeyCode::KeyU),
        heavy_attack: pad.heavy_attack || combat_key(KeyCode::KeyI),
        guard: buttons & 0x0100 != 0 || combat_key(KeyCode::KeyO),
        dodge: buttons & 0x2000 != 0 || combat_key(KeyCode::Space),
        camera_yaw: reference_yaw(cam_yaw),
        target: locked.map(|(_, t, _, _)| reference_position(t.translation)),
        dt: TICK as f32,
    };
    let ticks = host.combat.clock.advance(time.delta_secs_f64());
    host.input.sample(input);
    let Host {
        backend,
        combat,
        incoming,
        vertical_speed,
        grounded,
        steep_support,
        input,
        transport: _,
        controller: _,
    } = host;
    let Some(backend) = backend.as_mut() else {
        return;
    };
    backend.set_health(
        self_store.0.unit_health().unwrap_or(1) as f32
            / self_store.0.unit_max_health().unwrap_or(1).max(1) as f32,
    );
    for _ in 0..ticks {
        backend.set_transform(reference_position(drive.position), reference_yaw(drive.yaw));
        backend.set_grounded(*grounded);
        let before = drive.position;
        combat.step(&mut **backend, input.tick());
        let solved = backend.state();
        let proposed = Vec3::from_array(solved.position) / 0.9144;
        let mut velocity = (proposed - before) / TICK as f32;
        velocity.y = 0.0;
        let center = before + Vec3::Y * (crate::player::CAPSULE_HEIGHT * 0.5);
        let duration = std::time::Duration::from_secs_f64(TICK);
        let resolved = if *grounded {
            let step = crate::player::mover::grounded_step(
                &collision,
                &capsule.0,
                center,
                velocity,
                duration,
                crate::player::mover::Support {
                    steep: *steep_support,
                    ..Default::default()
                },
            );
            *steep_support = step.steep_support;
            *grounded = step.unsupported.is_none();
            step.center - Vec3::Y * step.unsupported.unwrap_or(0.0)
        } else {
            let (end, mean) = crate::player::mover::fall_step(
                *vertical_speed,
                TICK as f32,
                crate::player::TERMINAL_VELOCITY,
            );
            *vertical_speed = end;
            velocity.y = mean;
            let mut center = crate::player::mover::airborne_step(
                &collision, &capsule.0, center, velocity, duration,
            );
            if let Some(hit) = collision.cast_mover(&capsule.0, center, Vec3::NEG_Y * 0.05, 0.02) {
                if hit.normal1.y >= 0.642_788 {
                    center.y -= hit.distance;
                    *grounded = true;
                    *vertical_speed = 0.0;
                }
            }
            center
        };
        drive.position = resolved - Vec3::Y * (crate::player::CAPSULE_HEIGHT * 0.5);
        drive.falling = !*grounded;
        drive.yaw = solved.yaw - std::f32::consts::PI;
        drive.speed = Vec2::new(drive.position.x - before.x, drive.position.z - before.z).length()
            / TICK as f32;
        drive.velocity = (drive.position - before) / TICK as f32;
        drive.action = solved.action;
        drive.stamina = solved.stamina;
        backend.set_transform(reference_position(drive.position), reference_yaw(drive.yaw));
        drive.guarding = backend.guard(0.0, 1.0).is_some();
        drive.joints = backend
            .pose()
            .iter()
            .map(|joint| benilla_protocol::tarnished::Joint {
                name: joint.name.clone(),
                translation: joint.position,
                rotation: joint.rotation,
                scale: [1.0; 3],
            })
            .collect();
        // Observers need no ER install: carry the reference frames needed to fit their M2.
        drive.joints.extend(
            drive
                .bind
                .iter()
                .filter(|joint| {
                    matches!(
                        joint.name.as_str(),
                        "Head" | "L_Hand" | "R_Hand" | "L_Forearm" | "R_Forearm"
                    )
                })
                .map(|joint| benilla_protocol::tarnished::Joint {
                    name: format!("Bind_{}", joint.name),
                    translation: joint.position,
                    rotation: joint.rotation,
                    scale: [1.0; 3],
                }),
        );
        drive.camera = backend.camera().map(|(eye, pivot)| {
            let eye = Vec3::from_array(eye) / 0.9144;
            let pivot = Vec3::from_array(pivot) / 0.9144;
            let boom = eye - pivot;
            let eye = Dir3::new(boom)
                .ok()
                .and_then(|direction| collision.ray_body(pivot, direction, boom.length()))
                .map_or(eye, |hit| {
                    pivot + boom.normalize_or_zero() * (hit.distance - 0.2).max(0.5)
                });
            Transform::from_translation(eye).looking_at(pivot, Vec3::Y)
        });
        while let Some(hit) = incoming.pop_front() {
            let source = index
                .0
                .get(&hit.source_guid)
                .and_then(|e| objects.get(*e).ok());
            let source_position = source.map_or(drive.position, |(_, t, _, _)| t.translation);
            let toward = (source_position - drive.position)
                .with_y(0.0)
                .normalize_or_zero();
            let forward = Quat::from_rotation_y(drive.yaw) * Vec3::NEG_Z;
            let dot = toward.dot(forward).clamp(-1.0, 1.0);
            let source_damage = source.map_or(0.0, |(_, _, s, _)| {
                (s.0.unit_min_damage().unwrap_or(0.0) + s.0.unit_max_damage().unwrap_or(0.0)) * 0.5
            });
            let local_damage = (self_store.0.unit_min_damage().unwrap_or(0.0)
                + self_store.0.unit_max_damage().unwrap_or(0.0))
                * 0.5;
            let baseline = if source_damage > 0.0 {
                source_damage
            } else {
                local_damage
            };
            let stamina = hit.stamina_milli.map_or_else(
                || {
                    backend.stock_stamina_damage(if baseline > 0.0 {
                        hit.amount as f32 / baseline
                    } else {
                        0.0
                    })
                },
                |value| value as f32 / 1000.0,
            );
            let outcome = defence::resolve(
                &hit,
                &DefenceState {
                    iframe: backend.invincible(),
                    passive_eligible: source.is_some()
                        && dot >= 0.0
                        && self_store.0.unit_flags() & 0x0004_0000 == 0,
                    dodge_chance: self_store.0.player_dodge_percentage().unwrap_or(0.0) / 100.0,
                    parry_chance: self_store.0.player_parry_percentage().unwrap_or(0.0) / 100.0,
                    can_parry: equipped(self_store, &items, &net),
                    guard: backend.guard(stamina, dot),
                },
                rand::random::<f32>(),
            );
            if let Some(verdict) = combat.defence(hit, outcome) {
                backend.react(outcome, stamina, reference_position(source_position));
                send(Packet::Defence(verdict), &net);
            }
        }
        if let Some(swing) = backend.active_swing() {
            let candidates: Vec<Body> = objects
                .iter()
                .filter_map(|(guid, transform, store, height)| {
                    if guid.0 == self_id.0
                        || store.0.unit_is_dead()
                        || !crate::target::can_attack(
                            Some(store),
                            reactions.factions.as_deref(),
                            &reactions.reputations,
                            Some(self_store),
                        )
                    {
                        return None;
                    }
                    let h = height.map_or(crate::player::CAPSULE_HEIGHT, |h| h.0) * 0.9144;
                    let radius = store
                        .0
                        .unit_bounding_radius()
                        .max(crate::player::CAPSULE_RADIUS)
                        * 0.9144;
                    Some(Body {
                        guid: guid.0,
                        pos: reference_position(transform.translation).into(),
                        radius,
                        height: h,
                        head_height: h - radius * 0.5,
                        head_radius: radius,
                    })
                })
                .collect();
            let targets = resolve_swing(
                &swing,
                WowWeapon {
                    min_damage: self_store.0.unit_min_damage().unwrap_or(0.0),
                    max_damage: self_store.0.unit_max_damage().unwrap_or(0.0),
                },
                backend.active_motion_value().unwrap_or(0.0),
                &candidates,
                |_| Outcome {
                    crit: rand::random::<f32>()
                        < self_store.0.player_crit_percentage().unwrap_or(0.0) / 100.0,
                    glancing: false,
                },
            );
            if let Some(verdict) =
                combat.attack(&**backend, time.elapsed().as_millis() as u32, targets)
            {
                backend.contact();
                send(Packet::Attack(verdict), &net);
            }
        }
    }
    link.publish(drive, time.elapsed_secs_f64(), &net, false);
}

#[derive(Component)]
struct StaminaHud;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        StaminaHud,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Percent(14.0),
            left: Val::Percent(4.0),
            ..default()
        },
        Text::new(""),
        TextFont::default(),
        TextColor(Color::srgb(0.65, 0.95, 0.5)),
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

fn draw_hud(
    drive: Res<Drive>,
    assets: Res<AssetServer>,
    mut hud: Query<(&mut Text, &mut TextFont, &mut Visibility), With<StaminaHud>>,
    mut font: Local<Option<Handle<Font>>>,
) {
    let Ok((mut text, mut style, mut visibility)) = hud.single_mut() else {
        return;
    };
    *visibility = if drive.active {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !drive.active {
        return;
    }
    style.font = font
        .get_or_insert_with(|| crate::char_select::wow_font(&assets))
        .clone();
    style.font_size = 20.0;
    text.0 = format!(
        "Tarnished  Stamina {:.0}{}{}",
        drive.stamina,
        if drive.guarding { "  GUARD" } else { "" },
        if drive.lock.is_some() { "  LOCK" } else { "" }
    );
}
