//! Fork exception: incoming defence belongs to the visible contact, never the windup.
use super::{Drive, Host};
use crate::{
    creature_anim::AnimSoundEvent,
    net::{Guid, GuidIndex, NetCommands, ObjectStore, SelfGuid},
};
use benilla_protocol::tarnished::{Incoming, Packet};
use benilla_tarnished::{
    defence::{self, DefenceState},
    impact::{Contact, Impacts, Sample},
    Backend,
};
use bevy::prelude::*;

#[derive(Message)]
pub(crate) struct ProjectileContact {
    pub source: Entity,
    pub target: Entity,
    pub spell_id: u32,
}

#[derive(Resource, Default)]
struct Contacts(Impacts);

pub(super) fn register(app: &mut App) {
    app.init_resource::<Contacts>()
        .add_message::<ProjectileContact>()
        .add_systems(
            Update,
            resolve
                .after(super::TarnishedInputSet)
                .after(super::ContactEventsSet),
        );
}

fn sample(
    backend: &dyn Backend,
    drive: &Drive,
    local: &ObjectStore,
    source: Option<(&Transform, &ObjectStore)>,
    can_parry: bool,
) -> Sample {
    let position = source.map_or(drive.position, |(t, _)| t.translation);
    let toward = (position - drive.position).with_y(0.).normalize_or_zero();
    let dot = toward
        .dot(Quat::from_rotation_y(drive.yaw) * Vec3::NEG_Z)
        .clamp(-1., 1.);
    let mean = |store: &ObjectStore| {
        (store.0.unit_min_damage().unwrap_or(0.) + store.0.unit_max_damage().unwrap_or(0.)) * 0.5
    };
    let source_damage = source.map_or(0., |(_, s)| mean(s));
    let baseline = if source_damage > 0. {
        source_damage
    } else {
        mean(local)
    };
    Sample {
        defence: DefenceState {
            iframe: backend.invincible(),
            passive_eligible: source.is_some()
                && dot >= 0.
                && local.0.unit_flags() & 0x0004_0000 == 0,
            dodge_chance: local.0.player_dodge_percentage().unwrap_or(0.) / 100.,
            parry_chance: local.0.player_parry_percentage().unwrap_or(0.) / 100.,
            can_parry,
            guard: backend.guard(0., dot),
        },
        source: super::reference_position(position),
        stamina_per_damage: if baseline > 0. {
            backend.stock_stamina_damage(1.) / baseline
        } else {
            0.
        },
    }
}

fn answer(host: &mut Host, hit: Incoming, mut sample: Sample, net: &NetCommands) {
    let stamina = hit
        .stamina_milli
        .map_or(hit.amount as f32 * sample.stamina_per_damage, |v| {
            v as f32 / 1000.
        });
    if let Some(guard) = &mut sample.defence.guard {
        guard.stamina_damage = stamina;
    }
    let outcome = defence::resolve(&hit, &sample.defence, rand::random::<f32>());
    if let Some(verdict) = host.combat.defence(hit, outcome) {
        if let Some(backend) = &mut host.backend {
            backend.react(outcome, stamina, sample.source);
        }
        super::send(Packet::Defence(verdict), net);
    }
}

fn resolve(
    mut host: ResMut<Host>,
    drive: Res<Drive>,
    mut contacts: ResMut<Contacts>,
    mut melee: MessageReader<AnimSoundEvent>,
    mut missiles: MessageReader<ProjectileContact>,
    objects: Query<(&Guid, &Transform, &ObjectStore)>,
    index: Res<GuidIndex>,
    self_guid: Res<SelfGuid>,
    time: Res<Time>,
    net: Res<NetCommands>,
) {
    let Some((_, _, local)) = self_guid
        .0
        .and_then(|g| index.0.get(&g))
        .and_then(|e| objects.get(*e).ok())
    else {
        contacts.0.clear();
        melee.clear();
        missiles.clear();
        return;
    };
    if !drive.active || host.backend.is_none() {
        contacts.0.clear();
        melee.clear();
        missiles.clear();
        return;
    }
    let now = time.elapsed_secs_f64();
    contacts.0.expire(now);
    let can_parry = host
        .loadout
        .is_some_and(|gear| gear.right != benilla_tarnished::ErWeapon::Fist);
    while let Some((key, hit)) = host.visual_incoming.pop_front() {
        if let Some((hit, sample)) = contacts.0.request(key, hit, now) {
            answer(&mut host, hit, sample, &net);
        }
    }
    // ER contacts and non-travelling spells arrive at contact already.
    while let Some(hit) = host.incoming.pop_front() {
        let source = index
            .0
            .get(&hit.source_guid)
            .and_then(|e| objects.get(*e).ok())
            .map(|(_, t, s)| (t, s));
        let state = sample(
            &**host.backend.as_ref().unwrap(),
            &drive,
            local,
            source,
            can_parry,
        );
        answer(&mut host, hit, state, &net);
    }
    for event in melee.read() {
        if crate::creature_anim::impact_tag(&event.ident).is_none() {
            continue;
        }
        let Ok((guid, t, s)) = objects.get(event.entity) else {
            continue;
        };
        let state = sample(
            &**host.backend.as_ref().unwrap(),
            &drive,
            local,
            Some((t, s)),
            can_parry,
        );
        if let Some((hit, state)) = contacts.0.contact(Contact::Melee(guid.0), state, now) {
            answer(&mut host, hit, state, &net);
        }
    }
    for event in missiles.read() {
        if index.0.get(&self_guid.0.unwrap()) != Some(&event.target) {
            continue;
        }
        let Ok((guid, t, s)) = objects.get(event.source) else {
            continue;
        };
        let state = sample(
            &**host.backend.as_ref().unwrap(),
            &drive,
            local,
            Some((t, s)),
            can_parry,
        );
        if let Some((hit, state)) = contacts.0.contact(
            Contact::Projectile {
                source: guid.0,
                spell: event.spell_id,
            },
            state,
            now,
        ) {
            answer(&mut host, hit, state, &net);
        }
    }
}
