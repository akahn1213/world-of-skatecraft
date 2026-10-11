//! Negotiated, freshest-only full visual poses; sender clocks are mapped by capture deltas.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use benilla_assets::LockRecover;
use benilla_protocol::skate::{self, Joint, Relay, Snapshot};
use benilla_protocol::{SessionEvent, SessionEventKind};
use bevy::prelude::*;

use super::{rig, SkatePose};
use crate::net::{ClientCommand, GuidIndex, NetCommands, NetHandlerApp, SelfGuid};

const TIMEOUT: f64 = 1.0;
const DELAY: f64 = 0.1;
const BUFFER: usize = 8;

/// One outstanding wake token and one replaceable payload, independent of the ordinary queue.
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
    acknowledged: bool,
    attempts: u8,
    next_hello: f64,
    last_send: f64,
    active: bool,
    ride: u32,
    sequence: u32,
    mailbox: Arc<Mutex<Mailbox>>,
}

impl Link {
    fn publish(&self, body: Vec<u8>, commands: &NetCommands) {
        let mut slot = self.mailbox.lock_recover();
        slot.payload = Some(body);
        if !slot.queued {
            slot.queued = true;
            if commands
                .0
                .send(ClientCommand::Skate(self.mailbox.clone()))
                .is_err()
            {
                slot.take();
            }
        }
    }
}

#[derive(Clone)]
struct Sample {
    local: f64,
    snapshot: Snapshot,
}

#[derive(Default)]
struct Stream {
    samples: VecDeque<Sample>,
    last_arrival: f64,
    receipt_ms: u32,
}

fn newer(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) > 0
}

impl Stream {
    fn push(&mut self, snapshot: Snapshot, receipt_ms: u32, now: f64) -> bool {
        let mut local = now;
        if let Some(last) = self.samples.back() {
            let old = &last.snapshot;
            if snapshot.ride == old.ride {
                if !newer(snapshot.sequence, old.sequence) {
                    return false;
                }
                let delta = snapshot.sample_ms.wrapping_sub(old.sample_ms) as i32;
                let server_delta = receipt_ms.wrapping_sub(self.receipt_ms) as i32;
                // Re-anchor a clock discontinuity or a long silence; never synchronize clocks.
                if delta > 0
                    && delta <= 1000
                    && server_delta >= 0
                    && now - self.last_arrival <= TIMEOUT
                {
                    local = (last.local + f64::from(delta) / 1000.0).clamp(now - 0.25, now + 0.1);
                } else {
                    self.samples.clear();
                }
            } else {
                if !newer(snapshot.ride, old.ride) {
                    return false;
                }
                self.samples.clear();
            }
        }
        self.last_arrival = now;
        self.receipt_ms = receipt_ms;
        if !snapshot.active {
            self.samples.clear();
        }
        self.samples.push_back(Sample { local, snapshot });
        while self.samples.len() > BUFFER {
            self.samples.pop_front();
        }
        true
    }

    fn active(&self, now: f64) -> bool {
        now - self.last_arrival <= TIMEOUT && self.samples.back().is_some_and(|s| s.snapshot.active)
    }

    fn sample(&mut self, now: f64) -> Option<Snapshot> {
        if !self.active(now) {
            return None;
        }
        let target = now - DELAY;
        while self.samples.len() > 2 && self.samples[1].local <= target {
            self.samples.pop_front();
        }
        let a = self.samples.front()?;
        let Some(b) = self.samples.get(1) else {
            return Some(a.snapshot.clone());
        };
        let span = b.local - a.local;
        let t = if span > 0.0 {
            ((target - a.local) / span).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        Some(interpolate(&a.snapshot, &b.snapshot, t))
    }
}

fn interpolate(a: &Snapshot, b: &Snapshot, t: f32) -> Snapshot {
    let lerp =
        |a: [f32; 3], b: [f32; 3]| Vec3::from_array(a).lerp(Vec3::from_array(b), t).to_array();
    let turn = |a: [f32; 4], b: [f32; 4]| {
        Quat::from_array(a)
            .normalize()
            .slerp(Quat::from_array(b).normalize(), t)
            .to_array()
    };
    let mut out = b.clone();
    out.position = lerp(a.position, b.position);
    out.rotation = turn(a.rotation, b.rotation);
    for joint in &mut out.joints {
        if let Some(old) = a.joints.iter().find(|j| j.name == joint.name) {
            joint.translation = lerp(old.translation, joint.translation);
            joint.rotation = turn(old.rotation, joint.rotation);
            joint.scale = lerp(old.scale, joint.scale);
        }
    }
    out
}

#[derive(Component, Default)]
pub(crate) struct RemoteSkate {
    stream: Stream,
    pub(super) pose: SkatePose,
    pub(super) plan: rig::Plan,
    pub(super) held: rig::Held,
}

/// Presence is refreshed in Net before ordinary motion and removed on off/timeout.
#[derive(Component)]
pub(crate) struct ReplicatedSkating;

pub(super) fn register(app: &mut App) {
    use SessionEventKind as K;
    app.init_resource::<Link>()
        .net_handler(K::Skate, on_packet)
        .net_handler(K::Connected, on_edge)
        .net_handler(K::Disconnected, on_edge)
        .net_handler(K::Worldport, on_edge);
}

fn on_edge(In(ev): In<SessionEvent>, mut link: ResMut<Link>, time: Res<Time>) {
    match ev {
        SessionEvent::Connected { .. } | SessionEvent::Worldport { .. } => {
            link.mailbox.lock_recover().payload = None;
            link.entered = true;
            link.acknowledged = false;
            link.attempts = 0;
            link.next_hello = time.elapsed_secs_f64();
            link.active = false;
            link.ride = link.ride.wrapping_add(1);
        }
        SessionEvent::Disconnected { .. } => {
            link.mailbox.lock_recover().payload = None;
            link.entered = false;
            link.acknowledged = false;
        }
        _ => {}
    }
}

fn on_packet(
    In(ev): In<SessionEvent>,
    mut link: ResMut<Link>,
    time: Res<Time>,
    index: Res<GuidIndex>,
    self_guid: Res<SelfGuid>,
    mut remotes: Query<&mut RemoteSkate>,
    mut commands: Commands,
) {
    let SessionEvent::Skate(packet) = ev else {
        return;
    };
    match packet {
        Relay::Hello if link.entered => {
            link.acknowledged = true;
            link.last_send = f64::NEG_INFINITY;
        }
        Relay::Snapshot {
            guid,
            receipt_ms,
            snapshot,
        } if link.acknowledged => {
            if self_guid.0 == Some(guid) {
                return;
            }
            let Some(&entity) = index.0.get(&guid) else {
                return;
            };
            if let Ok(mut remote) = remotes.get_mut(entity) {
                remote
                    .stream
                    .push(snapshot, receipt_ms, time.elapsed_secs_f64());
            } else {
                let mut remote = RemoteSkate::default();
                remote
                    .stream
                    .push(snapshot, receipt_ms, time.elapsed_secs_f64());
                commands.entity(entity).insert(remote);
            }
        }
        _ => {}
    }
}

fn rate() -> f64 {
    static RATE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *RATE.get_or_init(|| {
        std::env::var("WOW_SKATE_NET_HZ")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(20.0)
            .clamp(1.0, 20.0)
    })
}

fn capture(pose: &SkatePose, ride: u32, sequence: u32) -> Option<Snapshot> {
    let (_, rotation, translation) = pose.root.to_scale_rotation_translation();
    let mut joints = Vec::new();
    if pose.active {
        // Send the solved rig dynamically, including the board model's actual joint targets.
        for (name, bone) in pose.names.iter().zip(&pose.bones) {
            let (scale, rotation, translation) = bone.to_scale_rotation_translation();
            joints.push(Joint {
                name: name.clone(),
                translation: translation.to_array(),
                rotation: rotation.normalize().to_array(),
                scale: scale.to_array(),
            });
        }
        if joints.len() > skate::MAX_JOINTS {
            // Retarget and board joints take priority over fingers and accessory chains.
            let required = super::board::required_joints();
            joints.retain(|j| required.contains(&j.name.as_str()) || rig::required_joint(&j.name));
        }
    }
    let snapshot = Snapshot {
        ride,
        sequence,
        sample_ms: pose.sample_ms,
        active: pose.active,
        position: super::from_skate(translation, pose.origin).to_array(),
        rotation: rotation.normalize().to_array(),
        joints,
    };
    snapshot.validate().ok()?;
    Some(snapshot)
}

pub(super) fn send_local(
    pose: Res<SkatePose>,
    time: Res<Time>,
    mut link: ResMut<Link>,
    commands: Res<NetCommands>,
) {
    let now = time.elapsed_secs_f64();
    if !link.entered {
        return;
    }
    if !link.acknowledged {
        if link.attempts < 3 && now >= link.next_hello {
            link.publish(skate::HELLO.to_vec(), &commands);
            link.attempts += 1;
            link.next_hello = now + 2.0;
        }
        return;
    }
    let changed = pose.active != link.active;
    if !changed && now - link.last_send < if pose.active { 1.0 / rate() } else { 1.0 } {
        return;
    }
    if changed && pose.active {
        link.ride = link.ride.wrapping_add(1);
    }
    link.sequence = link.sequence.wrapping_add(1);
    if let Some(snapshot) = capture(&pose, link.ride, link.sequence) {
        if let Ok(body) = snapshot.encode() {
            link.publish(body, &commands);
            link.last_send = now;
            link.active = pose.active;
        }
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_remote(
    time: Res<Time>,
    mut commands: Commands,
    mut units: Query<(
        Entity,
        &mut RemoteSkate,
        &mut Transform,
        Option<&mut crate::net::RemoteMotion>,
        Has<ReplicatedSkating>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    for (entity, mut remote, mut transform, motion, was_active) in &mut units {
        let Some(snapshot) = remote.stream.sample(now) else {
            remote.pose.active = false;
            if was_active {
                commands.entity(entity).remove::<ReplicatedSkating>();
                remote.held = rig::Held::default();
            }
            continue;
        };
        let pos = Vec3::from_array(snapshot.position);
        let rotation = Quat::from_array(snapshot.rotation).normalize();
        transform.translation = pos;
        transform.rotation = rotation;
        // Rebase to the world root: root translation zero, joints still in engine meters.
        remote.pose.active = true;
        remote.pose.origin = pos;
        remote.pose.root = Mat4::from_quat(rotation);
        remote.pose.names = snapshot.joints.iter().map(|j| j.name.clone()).collect();
        remote.pose.bones = snapshot
            .joints
            .iter()
            .map(|j| {
                Mat4::from_scale_rotation_translation(
                    Vec3::from_array(j.scale),
                    Quat::from_array(j.rotation).normalize(),
                    Vec3::from_array(j.translation),
                )
            })
            .collect();
        if let Some(mut motion) = motion {
            motion.wow_pos = benilla_assets::coords::bevy_to_wow(pos);
            let forward = rotation * Vec3::Z;
            motion.orientation = (-forward.z).atan2(-forward.x);
            motion.pending.clear();
        }
        if !was_active {
            commands.entity(entity).insert(ReplicatedSkating);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(seq: u32, ms: u32, x: f32) -> Snapshot {
        Snapshot {
            ride: 1,
            sequence: seq,
            sample_ms: ms,
            active: true,
            position: [x, 0.0, 0.0],
            rotation: Quat::IDENTITY.to_array(),
            joints: vec![Joint {
                name: "HIPS".into(),
                translation: [x, 0.0, 0.0],
                rotation: Quat::IDENTITY.to_array(),
                scale: [1.0; 3],
            }],
        }
    }

    #[test]
    fn unrelated_clocks_capture_deltas_and_bounded_hold() {
        let mut stream = Stream::default();
        stream.push(snapshot(1, 900_000, 0.0), 123, 5.0);
        stream.push(snapshot(2, 900_100, 10.0), 223, 5.13);
        let s = stream.sample(5.15).unwrap();
        assert!((s.position[0] - 5.0).abs() < 0.001);
        assert!((s.joints[0].translation[0] - 5.0).abs() < 0.001);
        assert_eq!(stream.sample(5.8).unwrap().position[0], 10.0);
        assert!(stream.sample(6.14).is_none());
    }

    #[test]
    fn lifecycle_off_restart_stale_and_wrapping_sequences() {
        let mut stream = Stream::default();
        stream.push(snapshot(u32::MAX, u32::MAX - 49, 0.0), u32::MAX - 49, 1.0);
        assert!(stream.push(snapshot(0, 50, 10.0), 50, 1.1));
        assert!(!stream.push(snapshot(u32::MAX, 0, 50.0), 0, 1.2));
        let mut off = snapshot(1, 100, 10.0);
        off.active = false;
        off.joints.clear();
        assert!(stream.push(off, 100, 1.2));
        assert!(stream.sample(1.2).is_none());
        let mut restart = snapshot(0, 0, 20.0);
        restart.ride = 2;
        assert!(stream.push(restart, 200, 1.3));
        assert_eq!(stream.sample(1.3).unwrap().position[0], 20.0);
        assert!(!stream.push(snapshot(100, 200, 30.0), 300, 1.4));
    }

    #[test]
    fn trs_rotation_and_names_are_interpolated_without_index_assumption() {
        let a = snapshot(1, 1, 0.0);
        let mut b = snapshot(2, 2, 2.0);
        b.rotation = Quat::from_rotation_z(1.0).to_array();
        b.joints[0].rotation = Quat::from_rotation_x(1.0).to_array();
        b.joints[0].scale = [3.0; 3];
        b.joints.insert(
            0,
            Joint {
                name: "NEW".into(),
                ..b.joints[0].clone()
            },
        );
        let s = interpolate(&a, &b, 0.5);
        assert!(Quat::from_array(s.rotation).angle_between(Quat::from_rotation_z(0.5)) < 0.001);
        assert_eq!(s.joints[1].scale, [2.0; 3]);
        assert!(
            Quat::from_array(s.joints[1].rotation).angle_between(Quat::from_rotation_x(0.5))
                < 0.001
        );
    }

    #[test]
    fn buffer_clock_discontinuity_and_rebase_roundtrip() {
        let mut stream = Stream::default();
        for i in 0..30 {
            stream.push(snapshot(i, i * 50, i as f32), i * 50, i as f64 * 0.05);
        }
        assert!(stream.samples.len() <= BUFFER);
        stream.push(snapshot(31, 1, 99.0), 1600, 1.6);
        assert_eq!(stream.samples.len(), 1);
        let pose = SkatePose {
            active: true,
            origin: Vec3::new(1000.0, 2000.0, 3000.0),
            root: Mat4::from_rotation_translation(
                Quat::from_rotation_x(0.3),
                Vec3::new(1.0, 2.0, 3.0),
            ),
            names: vec!["HIPS".into()],
            bones: vec![Mat4::IDENTITY],
            sample_ms: 42,
        };
        let s = capture(&pose, 1, 2).unwrap();
        let pos = Vec3::from_array(s.position);
        assert!(
            (pos - pose.origin - Vec3::new(1.0, 2.0, 3.0) / super::super::METERS_PER_YARD).length()
                < 0.001
        );
        assert_eq!(s.sample_ms, 42);
    }

    #[test]
    fn mailbox_keeps_only_freshest_and_rearms() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let commands = NetCommands(tx);
        let link = Link::default();
        for i in 0..100 {
            link.publish(vec![i], &commands);
        }
        assert_eq!(rx.len(), 1);
        assert_eq!(link.mailbox.lock_recover().take(), Some(vec![99]));
        rx.try_recv().unwrap();
        link.publish(vec![123], &commands);
        assert_eq!(rx.len(), 1);
    }
}
