//! Opt-in Tarnished combat extension: a negotiated, versioned transport for client-authoritative
//! ELDEN RING-style melee between capable clients, layered on top of the ordinary combat protocol
//! and never replacing it.
//!
//! Two authorities ride this channel. The attacker's client owns the *offence*: it computes swing
//! timing, the swept contacts (one swing can touch several enemies), and the per-target damage
//! scaled from the attack's motion value onto the attacker's real WoW weapon, then sends an
//! [`AttackVerdict`]. The defender's client owns the *defence*: the server forwards every incoming
//! melee or spell aimed at a Tarnished defender as an ordered [`Incoming`] event carrying a
//! server-issued correlation token, and the client answers with a [`DefenceVerdict`] (i-frame,
//! dodge, parry, shield guard or a plain hit). The server trusts the designated outcomes but
//! validates the protocol — ownership, the correlation token, duplicates, replay and whether the
//! attacker may hit the target at all — and falls back to stock resolution when a defender does
//! not answer in time, so a Tarnished player is never infinitely immune.

use std::collections::HashSet;
use std::io::{self, Read};

pub const CMSG_TARNISHED: u16 = 0x03f2;
pub const SMSG_TARNISHED: u16 = 0x03f3;
pub const MAX_PAYLOAD: usize = 8192;
pub const MAX_JOINTS: usize = 96;
/// Most enemies one swept swing may name. A swing that touches more than this is clamped by the
/// sender; the extra contacts are the sender's to drop, never the wire's to overflow.
pub const MAX_TARGETS: usize = 16;
/// The two-byte frame that acknowledges presence, in either direction.
pub const HELLO: [u8; 2] = [1, 0];

/// The capability version this build speaks. The discovery handshake is independent of skating's:
/// a server that relays skating need not relay Tarnished combat, and an older peer that knows
/// neither is undisturbed.
pub const VERSION: u8 = 3;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid tarnished payload")
}

fn take<const N: usize>(r: &mut &[u8]) -> io::Result<[u8; N]> {
    let mut value = [0; N];
    r.read_exact(&mut value)?;
    Ok(value)
}

fn u8_(r: &mut &[u8]) -> io::Result<u8> {
    Ok(take::<1>(r)?[0])
}

fn u16_(r: &mut &[u8]) -> io::Result<u16> {
    Ok(u16::from_le_bytes(take(r)?))
}

fn u32_(r: &mut &[u8]) -> io::Result<u32> {
    Ok(u32::from_le_bytes(take(r)?))
}

fn u64_(r: &mut &[u8]) -> io::Result<u64> {
    Ok(u64::from_le_bytes(take(r)?))
}

fn floats<const N: usize>(r: &mut &[u8]) -> io::Result<[f32; N]> {
    let mut value = [0.0; N];
    for v in &mut value {
        *v = f32::from_le_bytes(take(r)?);
        if !v.is_finite() {
            return Err(invalid());
        }
    }
    Ok(value)
}

fn quaternion(q: [f32; 4]) -> bool {
    q.iter().all(|v| v.is_finite()) && (0.9..=1.1).contains(&q.iter().map(|v| v * v).sum::<f32>())
}

fn name_valid(name: &str) -> bool {
    (1..=48).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// One enemy a swept swing reached, with the damage the attacker's client already scaled from the
/// attack's motion value onto its real weapon. The server keeps armour and resists itself; this is
/// the pre-mitigation physical amount the designated outcome claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackTarget {
    pub guid: u64,
    /// Pre-armour physical damage this contact deals.
    pub damage: u32,
    /// Bit 0 crit, bit 1 glancing; the rest reserved and must be zero.
    pub flags: u8,
}

const TARGET_FLAG_CRIT: u8 = 0x01;
const TARGET_FLAG_GLANCING: u8 = 0x02;
const TARGET_FLAGS_KNOWN: u8 = TARGET_FLAG_CRIT | TARGET_FLAG_GLANCING;

impl AttackTarget {
    pub fn crit(self) -> bool {
        self.flags & TARGET_FLAG_CRIT != 0
    }

    pub fn glancing(self) -> bool {
        self.flags & TARGET_FLAG_GLANCING != 0
    }

    fn validate(&self) -> io::Result<()> {
        if self.guid == 0 || self.flags & !TARGET_FLAGS_KNOWN != 0 {
            return Err(invalid());
        }
        Ok(())
    }
}

/// A client-authoritative attack: the attacker names its swing and every enemy it swept. The
/// `attack_id` correlates a swing across its targets and the server's reply; `sequence` orders and
/// de-duplicates the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackVerdict {
    /// Monotonic within one login, so the server can reject a replayed or stale swing.
    pub sequence: u32,
    /// Correlates the swing with its [`Relay::Attack`] echo; unique within a login.
    pub attack_id: u32,
    /// The attacker's clock at contact, for relay interpolation only — never trusted for timing.
    pub swing_ms: u32,
    /// ER weapon class driving the moveset (0..=24; 24 is the shield bash).
    pub weapon_class: u8,
    /// The attack kind within the moveset (a [`data::AttackKind`]-style index).
    pub attack_kind: u8,
    pub targets: Vec<AttackTarget>,
    /// Reference stamina damage before the guard multiplier, in thousandths of a point.
    /// Optional tail preserves decoding of the original v1 offence envelope.
    pub stamina_milli: Option<u32>,
}

impl AttackVerdict {
    pub fn validate(&self) -> io::Result<()> {
        if self.weapon_class > 24
            || self.targets.is_empty()
            || self.targets.len() > MAX_TARGETS
            || self.stamina_milli.is_some_and(|v| v > 1_000_000_000)
        {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        for t in &self.targets {
            t.validate()?;
            if !seen.insert(t.guid) {
                // One swing cannot name the same enemy twice: a duplicate is a forged multiplier.
                return Err(invalid());
            }
        }
        Ok(())
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> io::Result<()> {
        self.validate()?;
        out.extend(self.sequence.to_le_bytes());
        out.extend(self.attack_id.to_le_bytes());
        out.extend(self.swing_ms.to_le_bytes());
        out.push(self.weapon_class);
        out.push(self.attack_kind);
        out.push(self.targets.len() as u8);
        for t in &self.targets {
            out.extend(t.guid.to_le_bytes());
            out.extend(t.damage.to_le_bytes());
            out.push(t.flags);
        }
        if let Some(stamina) = self.stamina_milli {
            out.extend(stamina.to_le_bytes());
        }
        Ok(())
    }

    fn decode_from(r: &mut &[u8]) -> io::Result<Self> {
        let sequence = u32_(r)?;
        let attack_id = u32_(r)?;
        let swing_ms = u32_(r)?;
        let weapon_class = u8_(r)?;
        let attack_kind = u8_(r)?;
        let count = u8_(r)? as usize;
        if count == 0 || count > MAX_TARGETS {
            return Err(invalid());
        }
        let mut targets = Vec::with_capacity(count);
        for _ in 0..count {
            targets.push(AttackTarget {
                guid: u64_(r)?,
                damage: u32_(r)?,
                flags: u8_(r)?,
            });
        }
        let verdict = AttackVerdict {
            sequence,
            attack_id,
            swing_ms,
            weapon_class,
            attack_kind,
            targets,
            stamina_milli: if r.is_empty() { None } else { Some(u32_(r)?) },
        };
        verdict.validate()?;
        Ok(verdict)
    }
}

/// How a Tarnished defender's client resolved one incoming hit, in the fixed order the client
/// evaluates: an active i-frame, then an eligible passive WoW dodge or parry, then an active shield
/// guard, then the hit itself. The server applies exactly this; it never re-rolls avoidance on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefenceOutcome {
    /// An active dodge-roll i-frame covered the blow: no damage, no stamina here.
    Iframe,
    /// A stock WoW dodge connected (passive %, melee only).
    Dodge,
    /// A stock WoW parry connected (passive %, melee only).
    Parry,
    /// A held guard took the blow.
    Block {
        /// Damage the guard absorbed; the remainder in `taken` still lands.
        absorbed: u32,
        /// Stamina the guard spent, in whole points.
        stamina_cost: u16,
        /// The guard broke: the defender is open and staggered.
        guard_broken: bool,
        /// Damage that got through the guard.
        taken: u32,
    },
    /// Nothing stopped it: `taken` lands in full (armour and resists already applied upstream).
    Hit { taken: u32 },
}

/// A defender's answer to one [`Incoming`] event. The `attack_id` must echo the server's token, or
/// the server rejects it as uncorrelated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefenceVerdict {
    pub sequence: u32,
    /// Echoes the [`Incoming::attack_id`] this answers.
    pub attack_id: u32,
    /// Echoes the attacker so the server can match without trusting it.
    pub source_guid: u64,
    pub outcome: DefenceOutcome,
}

mod defence_tag {
    pub const IFRAME: u8 = 0;
    pub const DODGE: u8 = 1;
    pub const PARRY: u8 = 2;
    pub const BLOCK: u8 = 3;
    pub const HIT: u8 = 4;
}

impl DefenceVerdict {
    /// Validate the correlated envelope, without re-rolling the designated client's defence.
    /// The server must additionally bind the authenticated session to the pending defender and
    /// enforce its replay/deadline policy before applying ordinary damage consequences.
    pub fn validate_for(&self, incoming: &Incoming) -> io::Result<()> {
        if self.attack_id != incoming.attack_id || self.source_guid != incoming.source_guid {
            return Err(invalid());
        }
        match self.outcome {
            DefenceOutcome::Dodge | DefenceOutcome::Parry if !incoming.melee => Err(invalid()),
            DefenceOutcome::Block {
                absorbed, taken, ..
            } if absorbed.checked_add(taken) != Some(incoming.amount) => Err(invalid()),
            DefenceOutcome::Hit { taken } if taken != incoming.amount => Err(invalid()),
            _ => Ok(()),
        }
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend(self.sequence.to_le_bytes());
        out.extend(self.attack_id.to_le_bytes());
        out.extend(self.source_guid.to_le_bytes());
        match self.outcome {
            DefenceOutcome::Iframe => out.push(defence_tag::IFRAME),
            DefenceOutcome::Dodge => out.push(defence_tag::DODGE),
            DefenceOutcome::Parry => out.push(defence_tag::PARRY),
            DefenceOutcome::Block {
                absorbed,
                stamina_cost,
                guard_broken,
                taken,
            } => {
                out.push(defence_tag::BLOCK);
                out.extend(absorbed.to_le_bytes());
                out.extend(stamina_cost.to_le_bytes());
                out.push(u8::from(guard_broken));
                out.extend(taken.to_le_bytes());
            }
            DefenceOutcome::Hit { taken } => {
                out.push(defence_tag::HIT);
                out.extend(taken.to_le_bytes());
            }
        }
    }

    fn decode_from(r: &mut &[u8]) -> io::Result<Self> {
        let sequence = u32_(r)?;
        let attack_id = u32_(r)?;
        let source_guid = u64_(r)?;
        let outcome = match u8_(r)? {
            defence_tag::IFRAME => DefenceOutcome::Iframe,
            defence_tag::DODGE => DefenceOutcome::Dodge,
            defence_tag::PARRY => DefenceOutcome::Parry,
            defence_tag::BLOCK => DefenceOutcome::Block {
                absorbed: u32_(r)?,
                stamina_cost: u16_(r)?,
                guard_broken: match u8_(r)? {
                    0 => false,
                    1 => true,
                    _ => return Err(invalid()),
                },
                taken: u32_(r)?,
            },
            defence_tag::HIT => DefenceOutcome::Hit { taken: u32_(r)? },
            _ => return Err(invalid()),
        };
        Ok(DefenceVerdict {
            sequence,
            attack_id,
            source_guid,
            outcome,
        })
    }
}

/// One joint of a relayed combat pose, as skating carries them: root-relative, engine metres.
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub name: String,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// A freshest-only visual snapshot of a Tarnished character: stance, guard and the solved pose.
/// Unlike the ordered gameplay verdicts, these are latest-wins; a dropped one costs nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    /// Bumped whenever the character re-enters combat mode, so a stale stream is discarded.
    pub bout: u32,
    pub sequence: u32,
    pub sample_ms: u32,
    /// In the Tarnished stance at all.
    pub active: bool,
    /// Holding the guard up this frame.
    pub guarding: bool,
    /// Current action index, for a remote HUD and sound; opaque to the wire.
    pub action: u16,
    /// Bevy axes, world yards.
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub joints: Vec<Joint>,
}

impl Pose {
    pub fn validate(&self) -> io::Result<()> {
        if !self.position.iter().all(|v| v.is_finite())
            || !quaternion(self.rotation)
            || self.joints.len() > MAX_JOINTS
            || (!self.active && !self.joints.is_empty())
        {
            return Err(invalid());
        }
        let mut names = HashSet::new();
        for j in &self.joints {
            if !name_valid(&j.name)
                || !names.insert(&j.name)
                || !j.translation.iter().all(|v| v.is_finite())
                || !quaternion(j.rotation)
                || !j
                    .scale
                    .iter()
                    .all(|v| v.is_finite() && (0.001..=100.0).contains(v))
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> io::Result<()> {
        self.validate()?;
        for n in [self.bout, self.sequence, self.sample_ms] {
            out.extend(n.to_le_bytes());
        }
        out.push(u8::from(self.active));
        out.push(u8::from(self.guarding));
        out.extend(self.action.to_le_bytes());
        let append = |out: &mut Vec<u8>, values: &[f32]| {
            for v in values {
                out.extend(v.to_le_bytes());
            }
        };
        append(out, &self.position);
        append(out, &self.rotation);
        out.push(self.joints.len() as u8);
        for j in &self.joints {
            out.push(j.name.len() as u8);
            out.extend(j.name.as_bytes());
            append(out, &j.translation);
            append(out, &j.rotation);
            append(out, &j.scale);
        }
        Ok(())
    }

    fn decode_from(r: &mut &[u8]) -> io::Result<Self> {
        let bout = u32_(r)?;
        let sequence = u32_(r)?;
        let sample_ms = u32_(r)?;
        let active = bool_(r)?;
        let guarding = bool_(r)?;
        let action = u16_(r)?;
        let position = floats(r)?;
        let rotation = floats(r)?;
        let count = u8_(r)? as usize;
        if count > MAX_JOINTS {
            return Err(invalid());
        }
        let mut joints = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u8_(r)? as usize;
            if !(1..=48).contains(&len) || r.len() < len {
                return Err(invalid());
            }
            let name = std::str::from_utf8(&r[..len])
                .map_err(|_| invalid())?
                .to_owned();
            *r = &r[len..];
            joints.push(Joint {
                name,
                translation: floats(r)?,
                rotation: floats(r)?,
                scale: floats(r)?,
            });
        }
        let pose = Pose {
            bout,
            sequence,
            sample_ms,
            active,
            guarding,
            action,
            position,
            rotation,
            joints,
        };
        pose.validate()?;
        Ok(pose)
    }
}

fn bool_(r: &mut &[u8]) -> io::Result<bool> {
    match u8_(r)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid()),
    }
}

/// What a Tarnished client sends up (`CMSG_TARNISHED`).
#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    Hello,
    /// Ordered stance request/retirement; visual poses cannot change gameplay ownership.
    Stance(Pose),
    Attack(AttackVerdict),
    Defence(DefenceVerdict),
    Pose(Pose),
}

mod kind {
    /// Second byte of the two-byte frame tag; the first is always 1 (so `[1, 0]` stays HELLO).
    pub const ATTACK: u8 = 2;
    pub const STANCE: u8 = 1;
    pub const DEFENCE: u8 = 3;
    pub const POSE: u8 = 4;
}

impl Packet {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Packet::Hello => out.extend(HELLO),
            Packet::Stance(p) => {
                out.extend([1, kind::STANCE]);
                p.encode_into(&mut out)?;
            }
            Packet::Attack(v) => {
                out.extend([1, kind::ATTACK]);
                v.encode_into(&mut out)?;
            }
            Packet::Defence(v) => {
                out.extend([1, kind::DEFENCE]);
                v.encode_into(&mut out);
            }
            Packet::Pose(p) => {
                out.extend([1, kind::POSE]);
                p.encode_into(&mut out)?;
            }
        }
        if out.len() > MAX_PAYLOAD {
            return Err(invalid());
        }
        Ok(out)
    }
}

pub fn decode(body: &[u8]) -> io::Result<Packet> {
    if body.len() > MAX_PAYLOAD {
        return Err(invalid());
    }
    if body == HELLO {
        return Ok(Packet::Hello);
    }
    let mut r = body;
    let tag = take::<2>(&mut r)?;
    if tag[0] != 1 {
        return Err(invalid());
    }
    let packet = match tag[1] {
        kind::STANCE => Packet::Stance(Pose::decode_from(&mut r)?),
        kind::ATTACK => Packet::Attack(AttackVerdict::decode_from(&mut r)?),
        kind::DEFENCE => Packet::Defence(DefenceVerdict::decode_from(&mut r)?),
        kind::POSE => Packet::Pose(Pose::decode_from(&mut r)?),
        _ => return Err(invalid()),
    };
    if !r.is_empty() {
        return Err(invalid());
    }
    Ok(packet)
}

/// The server-issued forward of one incoming melee or spell aimed at a Tarnished defender. The
/// defender's client resolves it and answers with a [`DefenceVerdict`] echoing `attack_id`; if no
/// answer arrives before the server's own deadline the server resolves it with the stock rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Incoming {
    /// The server's correlation token: random enough that a client cannot forge or replay one.
    pub attack_id: u32,
    pub source_guid: u64,
    /// Spell school (0 physical); carried so a client guard only absorbs what it should.
    pub school: u8,
    /// Damage after armour and resists, before the client's avoidance and absorption.
    pub amount: u32,
    /// A melee swing (eligible for dodge/parry) rather than a spell.
    pub melee: bool,
    /// Attacker-owned reference stamina damage; stock attacks have no such action metadata.
    pub stamina_milli: Option<u32>,
}

mod relay_tag {
    pub const POSE: u8 = 0x11;
    pub const ATTACK: u8 = 0x12;
    pub const INCOMING: u8 = 0x13;
}

/// What a Tarnished client receives (`SMSG_TARNISHED`): the server's relays of other clients'
/// frames, plus its own [`Incoming`] forwards.
#[derive(Clone, Debug, PartialEq)]
pub enum Relay {
    Hello,
    /// A freshest visual pose; the server's self echo grants or retires a requested stance.
    Pose {
        guid: u64,
        receipt_ms: u32,
        pose: Pose,
    },
    /// Another client's attack, relayed for a remote strike's visuals and sound.
    Attack {
        attacker_guid: u64,
        receipt_ms: u32,
        verdict: AttackVerdict,
    },
    /// A hit aimed at us to resolve.
    Incoming(Incoming),
}

pub fn decode_relay(body: &[u8]) -> io::Result<Relay> {
    if body.len() > MAX_PAYLOAD {
        return Err(invalid());
    }
    if body == HELLO {
        return Ok(Relay::Hello);
    }
    let mut r = body;
    let relay = match u8_(&mut r)? {
        relay_tag::POSE => {
            let guid = u64_(&mut r)?;
            let receipt_ms = u32_(&mut r)?;
            match decode(r)? {
                Packet::Pose(pose) => Relay::Pose {
                    guid,
                    receipt_ms,
                    pose,
                },
                _ => return Err(invalid()),
            }
        }
        relay_tag::ATTACK => {
            let attacker_guid = u64_(&mut r)?;
            let receipt_ms = u32_(&mut r)?;
            match decode(r)? {
                Packet::Attack(verdict) => Relay::Attack {
                    attacker_guid,
                    receipt_ms,
                    verdict,
                },
                _ => return Err(invalid()),
            }
        }
        relay_tag::INCOMING => {
            let incoming = Incoming {
                attack_id: u32_(&mut r)?,
                source_guid: u64_(&mut r)?,
                school: u8_(&mut r)?,
                amount: u32_(&mut r)?,
                melee: bool_(&mut r)?,
                stamina_milli: if r.is_empty() {
                    None
                } else {
                    Some(u32_(&mut r)?)
                },
            };
            if !r.is_empty() || incoming.stamina_milli.is_some_and(|v| v > 1_000_000_000) {
                return Err(invalid());
            }
            Relay::Incoming(incoming)
        }
        _ => return Err(invalid()),
    };
    Ok(relay)
}

/// Build the body a server writes for a [`Relay::Incoming`]; the relays of client frames are the
/// client's `encode` prefixed with the guid envelope, mirrored by [`decode_relay`].
pub fn encode_incoming(incoming: &Incoming) -> Vec<u8> {
    let mut out = vec![relay_tag::INCOMING];
    out.extend(incoming.attack_id.to_le_bytes());
    out.extend(incoming.source_guid.to_le_bytes());
    out.push(incoming.school);
    out.extend(incoming.amount.to_le_bytes());
    out.push(u8::from(incoming.melee));
    if let Some(stamina) = incoming.stamina_milli {
        out.extend(stamina.to_le_bytes());
    }
    out
}

/// Wrap a client's encoded [`Packet::Pose`] or [`Packet::Attack`] in the guid/receipt envelope a
/// server relays to nearby capable clients. Hello and the local-only verdicts are never relayed.
pub fn encode_relay(guid: u64, receipt_ms: u32, inner: &[u8]) -> io::Result<Vec<u8>> {
    let tag = match decode(inner)? {
        Packet::Pose(_) => relay_tag::POSE,
        Packet::Attack(_) => relay_tag::ATTACK,
        Packet::Hello | Packet::Defence(_) | Packet::Stance(_) => return Err(invalid()),
    };
    let mut out = vec![tag];
    out.extend(guid.to_le_bytes());
    out.extend(receipt_ms.to_le_bytes());
    out.extend(inner);
    if out.len() > MAX_PAYLOAD {
        return Err(invalid());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attack() -> AttackVerdict {
        AttackVerdict {
            sequence: 7,
            attack_id: 1001,
            swing_ms: 123456,
            weapon_class: 2,
            attack_kind: 5,
            targets: vec![
                AttackTarget {
                    guid: 0xAABB,
                    damage: 240,
                    flags: TARGET_FLAG_CRIT,
                },
                AttackTarget {
                    guid: 0xCCDD,
                    damage: 90,
                    flags: TARGET_FLAG_GLANCING,
                },
            ],
            stamina_milli: None,
        }
    }

    fn pose() -> Pose {
        Pose {
            bout: 3,
            sequence: 9,
            sample_ms: u32::MAX,
            active: true,
            guarding: true,
            action: 42,
            position: [1000.0, -20.0, 7.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            joints: vec![Joint {
                name: "HIPS".into(),
                translation: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            }],
        }
    }

    #[test]
    fn stance_is_ordered_and_not_a_visual_relay() {
        let packet = Packet::Stance(pose());
        let wire = packet.encode().unwrap();
        assert_eq!(&wire[..2], &[1, kind::STANCE]);
        assert_eq!(decode(&wire).unwrap(), packet);
        assert!(encode_relay(3, 10, &wire).is_err());
        for end in 0..wire.len() {
            assert!(decode(&wire[..end]).is_err());
        }
    }

    #[test]
    fn attack_roundtrips_and_relays() {
        let p = Packet::Attack(attack());
        let wire = p.encode().unwrap();
        assert_eq!(&wire[..2], &[1, kind::ATTACK]);
        assert_eq!(decode(&wire).unwrap(), p);
        let relay = encode_relay(0x1234, 55, &wire).unwrap();
        assert_eq!(
            decode_relay(&relay).unwrap(),
            Relay::Attack {
                attacker_guid: 0x1234,
                receipt_ms: 55,
                verdict: attack(),
            }
        );
    }

    #[test]
    fn defender_reply_preserves_correlated_damage_and_melee_eligibility() {
        let incoming = Incoming {
            attack_id: 9,
            source_guid: 7,
            school: 0,
            amount: 100,
            melee: true,
            stamina_milli: None,
        };
        let mut reply = DefenceVerdict {
            sequence: 1,
            attack_id: 9,
            source_guid: 7,
            outcome: DefenceOutcome::Block {
                absorbed: 60,
                taken: 40,
                stamina_cost: 11,
                guard_broken: false,
            },
        };
        assert!(reply.validate_for(&incoming).is_ok());
        reply.attack_id = 8;
        assert!(reply.validate_for(&incoming).is_err());
        reply.attack_id = 9;
        reply.source_guid = 6;
        assert!(reply.validate_for(&incoming).is_err());
        reply.source_guid = 7;
        reply.outcome = DefenceOutcome::Hit { taken: 99 };
        assert!(reply.validate_for(&incoming).is_err());
        reply.outcome = DefenceOutcome::Block {
            absorbed: u32::MAX,
            taken: 101,
            stamina_cost: 1,
            guard_broken: true,
        };
        assert!(reply.validate_for(&incoming).is_err());
        reply.outcome = DefenceOutcome::Dodge;
        assert!(reply.validate_for(&incoming).is_ok());
        assert!(reply
            .validate_for(&Incoming {
                melee: false,
                ..incoming
            })
            .is_err());
    }

    #[test]
    fn pose_roundtrips_and_relays() {
        let p = Packet::Pose(pose());
        let wire = p.encode().unwrap();
        assert_eq!(decode(&wire).unwrap(), p);
        let relay = encode_relay(0x9, 1, &wire).unwrap();
        assert_eq!(
            decode_relay(&relay).unwrap(),
            Relay::Pose {
                guid: 0x9,
                receipt_ms: 1,
                pose: pose(),
            }
        );
    }

    #[test]
    fn defence_roundtrips_every_outcome() {
        for outcome in [
            DefenceOutcome::Iframe,
            DefenceOutcome::Dodge,
            DefenceOutcome::Parry,
            DefenceOutcome::Block {
                absorbed: 120,
                stamina_cost: 35,
                guard_broken: true,
                taken: 15,
            },
            DefenceOutcome::Hit { taken: 310 },
        ] {
            let v = DefenceVerdict {
                sequence: 4,
                attack_id: 77,
                source_guid: 0xBEEF,
                outcome,
            };
            let wire = Packet::Defence(v).encode().unwrap();
            assert_eq!(decode(&wire).unwrap(), Packet::Defence(v));
        }
    }

    #[test]
    fn incoming_roundtrips_and_is_not_relayable_client_side() {
        let incoming = Incoming {
            attack_id: 0xDEADBEEF,
            source_guid: 0x42,
            school: 2,
            amount: 450,
            melee: true,
            stamina_milli: None,
        };
        assert_eq!(
            decode_relay(&encode_incoming(&incoming)).unwrap(),
            Relay::Incoming(incoming)
        );
        // A defender's answer and a hello are never wrapped for relay.
        assert!(encode_relay(1, 1, &Packet::Hello.encode().unwrap()).is_err());
        let defence = Packet::Defence(DefenceVerdict {
            sequence: 0,
            attack_id: 1,
            source_guid: 2,
            outcome: DefenceOutcome::Iframe,
        })
        .encode()
        .unwrap();
        assert!(encode_relay(1, 1, &defence).is_err());
    }

    #[test]
    fn reference_stamina_tail_roundtrips_without_breaking_stock_envelopes() {
        let mut offence = attack();
        offence.stamina_milli = Some(23_750);
        let wire = Packet::Attack(offence.clone()).encode().unwrap();
        assert_eq!(decode(&wire).unwrap(), Packet::Attack(offence));
        let mut malformed = wire.clone();
        malformed.push(0);
        assert!(decode(&malformed).is_err());
        let incoming = Incoming {
            attack_id: 1,
            source_guid: 2,
            school: 0,
            amount: 50,
            melee: true,
            stamina_milli: Some(23_750),
        };
        let body = encode_incoming(&incoming);
        assert_eq!(body.len(), 23);
        assert_eq!(decode_relay(&body).unwrap(), Relay::Incoming(incoming));
        let stock = Incoming {
            stamina_milli: None,
            ..incoming
        };
        assert_eq!(encode_incoming(&stock).len(), 19);
        assert_eq!(
            decode_relay(&encode_incoming(&stock)).unwrap(),
            Relay::Incoming(stock)
        );
        let bad = Incoming {
            stamina_milli: Some(u32::MAX),
            ..incoming
        };
        assert!(decode_relay(&encode_incoming(&bad)).is_err());
    }

    #[test]
    fn hello_both_directions() {
        assert_eq!(decode(&HELLO).unwrap(), Packet::Hello);
        assert_eq!(decode_relay(&HELLO).unwrap(), Relay::Hello);
        assert_eq!(Packet::Hello.encode().unwrap(), HELLO);
    }

    #[test]
    fn rejects_truncation_and_trailing_bytes() {
        for p in [Packet::Attack(attack()), Packet::Pose(pose())] {
            let wire = p.encode().unwrap();
            for end in 0..wire.len() {
                assert!(decode(&wire[..end]).is_err(), "{end}");
            }
            let mut extra = wire.clone();
            extra.push(0);
            assert!(decode(&extra).is_err());
            let relay = encode_relay(1, 2, &wire).unwrap();
            for end in 0..relay.len() {
                assert!(decode_relay(&relay[..end]).is_err(), "relay {end}");
            }
            let mut extra = relay;
            extra.push(0);
            assert!(decode_relay(&extra).is_err());
        }
        assert!(decode(&[1, 99]).is_err());
        assert!(decode_relay(&[99]).is_err());
        assert!(decode(&vec![0; MAX_PAYLOAD + 1]).is_err());
    }

    #[test]
    fn rejects_forged_attacks() {
        // No targets.
        let mut v = attack();
        v.targets.clear();
        assert!(Packet::Attack(v).encode().is_err());
        // A duplicate target guid is a forged damage multiplier.
        let mut v = attack();
        v.targets[1].guid = v.targets[0].guid;
        assert!(Packet::Attack(v).encode().is_err());
        // A zero guid.
        let mut v = attack();
        v.targets[0].guid = 0;
        assert!(Packet::Attack(v).encode().is_err());
        // Unknown flag bits.
        let mut v = attack();
        v.targets[0].flags = 0x80;
        assert!(Packet::Attack(v).encode().is_err());
        // An out-of-range weapon class.
        let mut v = attack();
        v.weapon_class = 25;
        assert!(Packet::Attack(v).encode().is_err());
        // Too many targets.
        let mut v = attack();
        v.targets = (1..=(MAX_TARGETS as u64 + 1))
            .map(|guid| AttackTarget {
                guid,
                damage: 1,
                flags: 0,
            })
            .collect();
        assert!(Packet::Attack(v).encode().is_err());
    }

    #[test]
    fn rejects_malformed_pose_fields() {
        let mut p = pose();
        p.rotation = [0.0; 4];
        assert!(Packet::Pose(p).encode().is_err());
        let mut p = pose();
        p.active = false;
        assert!(Packet::Pose(p).encode().is_err());
        let mut p = pose();
        p.joints[0].name = "bad-name".into();
        assert!(Packet::Pose(p).encode().is_err());
        let mut p = pose();
        p.joints = vec![p.joints[0].clone(); MAX_JOINTS + 1];
        assert!(Packet::Pose(p).encode().is_err());
    }

    #[test]
    fn a_guid_prefix_is_not_a_hello() {
        // Only the exact two-byte frame acknowledges; a relay that happens to start 1,0 must carry
        // a valid frame tag, which 0x11.. do not collide with.
        let relay = encode_relay(
            0x1_0000_0000_0000,
            0,
            &Packet::Pose(pose()).encode().unwrap(),
        )
        .unwrap();
        assert!(matches!(decode_relay(&relay).unwrap(), Relay::Pose { .. }));
    }
}
