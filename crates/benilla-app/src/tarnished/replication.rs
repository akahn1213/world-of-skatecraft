//! Stock-safe discovery; combat stays ordered, visual poses have one replaceable wake token.

use std::sync::{Arc, Mutex};

use benilla_assets::LockRecover;
use benilla_protocol::{
    tarnished::{Packet, Pose, Relay},
    SessionEvent, SessionEventKind,
};
use benilla_world::schedule::WorldStage;
use bevy::{prelude::*, time::Real};

use crate::net::{ClientCommand, GuidIndex, NetCommands, NetHandlerApp, SelfGuid};

use super::{Drive, Host};

#[derive(Default, Debug)]
pub(crate) struct Mailbox {
    payload: Option<Vec<u8>>,
    queued: bool,
}

impl Mailbox {
    pub(crate) fn take(&mut self) -> Option<Vec<u8>> {
        self.queued = false;
        self.payload.take()
    }
}

#[derive(Resource, Default)]
pub(super) struct Link {
    entered: bool,
    supported: bool,
    pub(super) acknowledged: bool,
    pub(super) requested: bool,
    pub(super) request_time: f64,
    attempts: u8,
    next_probe: f64,
    last_send: f64,
    bout: u32,
    sequence: u32,
    mailbox: Arc<Mutex<Mailbox>>,
}

impl Link {
    pub(super) fn discover(&mut self, now: f64, net: &NetCommands) {
        if !self.entered || self.acknowledged || self.attempts >= 3 || now < self.next_probe {
            return;
        }
        if self.supported {
            super::send(Packet::Hello, net);
        } else {
            let _ = net.0.send(ClientCommand::AddonMessage {
                distribution: benilla_ui::script::AddonDistribution::Party,
                text: format!("TARNISHED\tV\t{}", benilla_protocol::tarnished::VERSION),
            });
        }
        self.attempts += 1;
        self.next_probe = now + 2.0;
    }

    pub(super) fn begin(&mut self, now: f64) {
        self.bout = self.bout.wrapping_add(1);
        self.last_send = f64::NEG_INFINITY;
        self.requested = true;
        self.request_time = now;
    }

    fn pose(&mut self, drive: &Drive, now: f64) -> Pose {
        self.sequence = self.sequence.wrapping_add(1);
        Pose {
            bout: self.bout,
            sequence: self.sequence,
            sample_ms: (now * 1000.0) as u32,
            active: drive.active,
            guarding: drive.guarding && drive.active,
            action: if drive.active { drive.action } else { 0 },
            position: drive.position.to_array(),
            rotation: Quat::from_rotation_y(drive.yaw).to_array(),
            joints: if drive.active {
                drive.joints.clone()
            } else {
                Vec::new()
            },
        }
    }

    pub(super) fn publish_off(&mut self, drive: &Drive, net: &NetCommands) {
        self.mailbox.lock_recover().payload = None;
        let pose = self.pose(drive, self.last_send.max(0.0));
        super::send(Packet::Stance(pose), net);
    }

    pub(super) fn publish(&mut self, drive: &Drive, now: f64, net: &NetCommands, ordered: bool) {
        if !self.acknowledged || now - self.last_send < 0.05 {
            return;
        }
        let pose = self.pose(drive, now);
        let packet = if ordered {
            Packet::Stance(pose)
        } else {
            Packet::Pose(pose)
        };
        self.last_send = now;
        if ordered {
            super::send(packet, net);
            return;
        }
        if let Ok(payload) = packet.encode() {
            let mut slot = self.mailbox.lock_recover();
            slot.payload = Some(payload);
            if !slot.queued {
                slot.queued = true;
                if net
                    .0
                    .send(ClientCommand::TarnishedPose(self.mailbox.clone()))
                    .is_err()
                {
                    slot.take();
                }
            }
        }
    }
}

#[derive(Component)]
pub(crate) struct ReplicatedTarnished;

#[derive(Component)]
pub(super) struct Remote {
    pose: Pose,
    previous: Pose,
    arrived: f64,
    previous_arrival: f64,
    pub(super) displayed: Vec<benilla_protocol::tarnished::Joint>,
    pub(super) displayed_active: bool,
}

fn newer(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) > 0
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<Link>()
        .net_handler(SessionEventKind::Chat, capabilities)
        .net_handler(SessionEventKind::Tarnished, packet)
        .net_handler(SessionEventKind::Connected, edge)
        .net_handler(SessionEventKind::Disconnected, edge)
        .net_handler(SessionEventKind::LoggedOut, edge)
        .net_handler(SessionEventKind::Worldport, edge)
        .net_handler(SessionEventKind::Teleport, suspend)
        .net_handler(SessionEventKind::LogoutResponse, suspend)
        .add_systems(
            Update,
            remote
                .in_set(WorldStage::Input)
                .before(super::TarnishedInputSet),
        );
}

fn capabilities(In(event): In<SessionEvent>, mut link: ResMut<Link>) {
    if let SessionEvent::Chat(message) = event {
        if link.entered
            && message.language == benilla_protocol::messages::LANGUAGE_ADDON
            && message.sender_guid == 0
            && message.text == format!("TARNISHED\tCAPS\t{}", benilla_protocol::tarnished::VERSION)
        {
            link.supported = true;
            link.attempts = 0;
            link.next_probe = f64::NEG_INFINITY;
        }
    }
}

fn suspend(
    In(event): In<SessionEvent>,
    mut host: ResMut<Host>,
    mut drive: ResMut<Drive>,
    mut link: ResMut<Link>,
    self_guid: Res<SelfGuid>,
    net: Res<NetCommands>,
) {
    let stops = match event {
        SessionEvent::Teleport { guid, .. } => self_guid.0 == Some(guid),
        SessionEvent::LogoutResponse { reason, .. } => reason == 0,
        _ => false,
    };
    if stops {
        super::stop(&mut host, &mut drive, &mut link, &net);
    }
}

fn edge(
    In(event): In<SessionEvent>,
    mut link: ResMut<Link>,
    mut host: ResMut<Host>,
    mut drive: ResMut<Drive>,
    time: Res<Time<Real>>,
    net: Res<NetCommands>,
    remotes: Query<Entity, With<Remote>>,
    mut commands: Commands,
) {
    super::stop(&mut host, &mut drive, &mut link, &net);
    link.mailbox.lock_recover().payload = None;
    link.entered = !matches!(
        event,
        SessionEvent::Disconnected { .. } | SessionEvent::LoggedOut
    );
    link.acknowledged = false;
    link.requested = false;
    link.supported = false;
    link.attempts = 0;
    link.next_probe = time.elapsed_secs_f64();
    if matches!(event, SessionEvent::Connected { .. }) {
        host.combat = Default::default();
    }
    for entity in &remotes {
        commands
            .entity(entity)
            .remove::<(Remote, ReplicatedTarnished)>();
    }
}

fn packet(
    In(event): In<SessionEvent>,
    mut link: ResMut<Link>,
    mut host: ResMut<Host>,
    mut drive: ResMut<Drive>,
    time: Res<Time<Real>>,
    index: Res<GuidIndex>,
    self_guid: Res<SelfGuid>,
    mut remotes: Query<&mut Remote>,
    mut commands: Commands,
    net: Res<NetCommands>,
) {
    let SessionEvent::Tarnished(packet) = event else {
        return;
    };
    match packet {
        Relay::Hello if link.entered && link.supported => link.acknowledged = true,
        Relay::Incoming(hit) if link.acknowledged && drive.active => {
            // The server bounds pending work at 64; never let the host grow an unbounded queue.
            if host.incoming.len() < 64 {
                host.incoming.push_back(hit);
            }
        }
        Relay::Pose { guid, pose, .. }
            if link.acknowledged && self_guid.0 == Some(guid) && pose.bout == link.bout =>
        {
            if pose.active && link.requested {
                link.requested = false;
                drive.pending = false;
                drive.active = true;
                let _ = net.0.send(ClientCommand::TarnishedMode(true));
                info!("tarnished: server approved equipped stance");
            } else if !pose.active {
                super::stop(&mut host, &mut drive, &mut link, &net);
            }
        }
        Relay::Pose { guid, pose, .. } if link.acknowledged && self_guid.0 != Some(guid) => {
            let Some(&entity) = index.0.get(&guid) else {
                return;
            };
            let now = time.elapsed_secs_f64();
            if let Ok(mut remote) = remotes.get_mut(entity) {
                if (pose.bout == remote.pose.bout && !newer(pose.sequence, remote.pose.sequence))
                    || (pose.bout != remote.pose.bout && !newer(pose.bout, remote.pose.bout))
                {
                    return;
                }
                remote.previous = if pose.bout == remote.pose.bout {
                    remote.pose.clone()
                } else {
                    pose.clone()
                };
                remote.previous_arrival = remote.arrived;
                remote.pose = pose;
                remote.arrived = now;
            } else {
                commands.entity(entity).insert(Remote {
                    previous: pose.clone(),
                    pose,
                    arrived: now,
                    previous_arrival: now,
                    displayed: Vec::new(),
                    displayed_active: false,
                });
            }
        }
        _ => {}
    }
}

fn remote(
    time: Res<Time<Real>>,
    mut query: Query<(
        Entity,
        &mut Remote,
        &mut Transform,
        Has<ReplicatedTarnished>,
    )>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    for (entity, mut remote, mut transform, held) in &mut query {
        if !remote.pose.active || now - remote.arrived > 2.0 {
            remote.displayed_active = false;
            if held {
                commands.entity(entity).remove::<ReplicatedTarnished>();
            }
            continue;
        }
        remote.displayed_active = true;
        if !held {
            commands.entity(entity).insert(ReplicatedTarnished);
        }
        let span = remote.arrived - remote.previous_arrival;
        let t = if span > 0.0 {
            ((now - 0.1 - remote.previous_arrival) / span).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        transform.translation = Vec3::from_array(remote.previous.position)
            .lerp(Vec3::from_array(remote.pose.position), t);
        transform.rotation = Quat::from_array(remote.previous.rotation)
            .slerp(Quat::from_array(remote.pose.rotation), t);
        remote.displayed = remote
            .pose
            .joints
            .iter()
            .map(|joint| {
                let mut out = joint.clone();
                if let Some(previous) = remote.previous.joints.iter().find(|p| p.name == joint.name)
                {
                    out.translation = Vec3::from_array(previous.translation)
                        .lerp(Vec3::from_array(joint.translation), t)
                        .to_array();
                    out.rotation = Quat::from_array(previous.rotation)
                        .slerp(Quat::from_array(joint.rotation), t)
                        .to_array();
                }
                out
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn remote_baked_joints_are_displayed_only_until_off_or_timeout() {
        let mut world = World::new();
        world.init_resource::<Time<Real>>();
        let pose = Pose {
            bout: 1,
            sequence: 1,
            sample_ms: 0,
            active: true,
            guarding: false,
            action: 100,
            position: [0.0; 3],
            rotation: Quat::IDENTITY.to_array(),
            joints: vec![benilla_protocol::tarnished::Joint {
                name: "Pelvis".into(),
                translation: [0.0, 1.0, 0.0],
                rotation: Quat::IDENTITY.to_array(),
                scale: [1.0; 3],
            }],
        };
        let entity = world
            .spawn((
                Transform::default(),
                Remote {
                    previous: pose.clone(),
                    pose,
                    arrived: 0.0,
                    previous_arrival: 0.0,
                    displayed: Vec::new(),
                    displayed_active: false,
                },
            ))
            .id();
        world.run_system_once(remote).unwrap();
        assert!(world.get::<Remote>(entity).unwrap().displayed_active);
        assert_eq!(
            world.get::<Remote>(entity).unwrap().displayed[0].translation,
            [0.0, 1.0, 0.0]
        );
        assert!(world.get::<ReplicatedTarnished>(entity).is_some());
        world.get_mut::<Remote>(entity).unwrap().pose.active = false;
        world.run_system_once(remote).unwrap();
        assert!(!world.get::<Remote>(entity).unwrap().displayed_active);
        assert!(world.get::<ReplicatedTarnished>(entity).is_none());
        world.get_mut::<Remote>(entity).unwrap().pose.active = true;
        world
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(3));
        world.run_system_once(remote).unwrap();
        assert!(!world.get::<Remote>(entity).unwrap().displayed_active);
    }

    #[test]
    fn custom_hello_waits_for_server_authored_capabilities() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let net = NetCommands(tx);
        let mut world = World::new();
        world.insert_resource(Link {
            entered: true,
            ..default()
        });
        world.resource_mut::<Link>().discover(0.0, &net);
        assert!(matches!(rx.try_recv().unwrap(),
            ClientCommand::AddonMessage { text, .. } if text == "TARNISHED\tV\t3"));
        let mut message = benilla_protocol::messages::ChatMessage {
            chat_type: benilla_protocol::messages::CHAT_MSG_PARTY,
            language: benilla_protocol::messages::LANGUAGE_ADDON,
            sender_guid: 5,
            target_guid: 0,
            sender_name: None,
            channel: None,
            text: "TARNISHED\tCAPS\t3".into(),
            chat_tag: 0,
        };
        world
            .run_system_once_with(capabilities, SessionEvent::Chat(message.clone()))
            .unwrap();
        assert!(!world.resource::<Link>().supported);
        message.sender_guid = 0;
        world
            .run_system_once_with(capabilities, SessionEvent::Chat(message))
            .unwrap();
        world.resource_mut::<Link>().discover(0.0, &net);
        assert!(matches!(rx.try_recv().unwrap(),
            ClientCommand::Tarnished(body) if body == benilla_protocol::tarnished::HELLO));
    }

    #[test]
    fn only_matching_server_self_echo_grants_the_requested_stance() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut world = World::new();
        world.insert_resource(NetCommands(tx));
        world.insert_resource(Link {
            acknowledged: true,
            requested: true,
            bout: 7,
            ..default()
        });
        world.init_resource::<Host>();
        world.init_resource::<Drive>();
        world.init_resource::<Time<Real>>();
        world.init_resource::<GuidIndex>();
        world.insert_resource(SelfGuid(Some(3)));
        let pose = Pose {
            bout: 6,
            sequence: 1,
            sample_ms: 0,
            active: true,
            guarding: false,
            action: 0,
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            joints: vec![],
        };
        let event = |pose| {
            SessionEvent::Tarnished(Relay::Pose {
                guid: 3,
                receipt_ms: 0,
                pose,
            })
        };
        world
            .run_system_once_with(packet, event(pose.clone()))
            .unwrap();
        assert!(!world.resource::<Drive>().active);
        assert!(rx.is_empty());
        let mut approved = pose;
        approved.bout = 7;
        world
            .run_system_once_with(packet, event(approved.clone()))
            .unwrap();
        assert!(world.resource::<Drive>().active);
        assert!(!world.resource::<Link>().requested);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::TarnishedMode(true)
        ));
        // Subsequent visual echoes cannot grant the stance again.
        world.run_system_once_with(packet, event(approved)).unwrap();
        assert!(rx.is_empty());
    }

    #[test]
    fn visual_mailbox_is_latest_wins_with_one_wake() {
        let mut slot = Mailbox {
            payload: Some(vec![1]),
            queued: true,
        };
        slot.payload = Some(vec![2]);
        assert_eq!(slot.take(), Some(vec![2]));
        assert!(!slot.queued);
        assert!(slot.take().is_none());
    }

    #[test]
    fn visual_sequence_wrap_is_ordered() {
        assert!(newer(0, u32::MAX));
        assert!(!newer(u32::MAX, 0));
        assert!(!newer(17, 17));
    }
}
