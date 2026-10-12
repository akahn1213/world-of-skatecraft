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
                text: "TARNISHED\tV\t2".into(),
            });
        }
        self.attempts += 1;
        self.next_probe = now + 2.0;
    }

    pub(super) fn begin(&mut self) {
        self.bout = self.bout.wrapping_add(1);
        self.last_send = f64::NEG_INFINITY;
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
            // The action table contains root/blade paths, not an ER skeletal asset.
            joints: Vec::new(),
        }
    }

    pub(super) fn publish_off(&mut self, drive: &Drive, net: &NetCommands) {
        self.mailbox.lock_recover().payload = None;
        let pose = self.pose(drive, self.last_send.max(0.0));
        super::send(Packet::Pose(pose), net);
    }

    pub(super) fn publish(&mut self, drive: &Drive, now: f64, net: &NetCommands, ordered: bool) {
        if !self.acknowledged || now - self.last_send < 0.05 {
            return;
        }
        let packet = Packet::Pose(self.pose(drive, now));
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
struct Remote {
    pose: Pose,
    previous: Pose,
    arrived: f64,
    previous_arrival: f64,
    displayed_action: u16,
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
            && message.text == "TARNISHED\tCAPS\t2"
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
    drive: Res<Drive>,
    time: Res<Time<Real>>,
    index: Res<GuidIndex>,
    self_guid: Res<SelfGuid>,
    mut remotes: Query<&mut Remote>,
    mut commands: Commands,
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
                    displayed_action: u16::MAX,
                });
            }
        }
        _ => {}
    }
}

pub(super) fn native_animation(action: u16, guarding: bool) -> Option<u16> {
    match action {
        100..=140 => Some(17), // WoW Attack1H; never a source of ER timing.
        2 => Some(30),         // Dodge presentation, not a fabricated ER skeletal animation.
        3 => Some(24),
        4 => Some(14),
        5 => Some(7),
        _ if guarding => Some(24),
        _ => None,
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
    mut visuals: MessageWriter<crate::creature_anim::EmoteAnim>,
    mut sequence: ResMut<crate::creature_anim::PlaySeq>,
) {
    let now = time.elapsed_secs_f64();
    for (entity, mut remote, mut transform, held) in &mut query {
        if !remote.pose.active || now - remote.arrived > 2.0 {
            if held {
                commands.entity(entity).remove::<ReplicatedTarnished>();
            }
            continue;
        }
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
        if !held || remote.pose.action != remote.displayed_action {
            remote.displayed_action = remote.pose.action;
            if let Some(anim_id) = native_animation(remote.pose.action, remote.pose.guarding) {
                visuals.write(crate::creature_anim::EmoteAnim {
                    entity,
                    anim_id,
                    seq: sequence.next(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_mailbox_is_latest_wins_with_one_wake() {
        let mut slot = Mailbox::default();
        slot.payload = Some(vec![1]);
        slot.queued = true;
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
