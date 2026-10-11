//! Opt-in skating visual extension; it never replaces the ordinary movement protocol.

use std::collections::HashSet;
use std::io::{self, Read};

pub const CMSG_SKATE: u16 = 0x03f0;
pub const SMSG_SKATE: u16 = 0x03f1;
pub const MAX_PAYLOAD: usize = 8192;
pub const MAX_JOINTS: usize = 96;
pub const HELLO: [u8; 2] = [1, 0];

#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub name: String,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub ride: u32,
    pub sequence: u32,
    pub sample_ms: u32,
    pub active: bool,
    /// Bevy axes, world yards.
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    /// Root-relative, engine meters.
    pub joints: Vec<Joint>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    Hello,
    Snapshot(Snapshot),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Relay {
    Hello,
    Snapshot {
        guid: u64,
        receipt_ms: u32,
        snapshot: Snapshot,
    },
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid skating payload")
}

fn take<const N: usize>(r: &mut &[u8]) -> io::Result<[u8; N]> {
    let mut value = [0; N];
    r.read_exact(&mut value)?;
    Ok(value)
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

impl Snapshot {
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

    pub fn encode(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        let mut out = vec![1, 1];
        for n in [self.ride, self.sequence, self.sample_ms] {
            out.extend(n.to_le_bytes());
        }
        out.push(u8::from(self.active));
        let append = |out: &mut Vec<u8>, values: &[f32]| {
            for v in values {
                out.extend(v.to_le_bytes());
            }
        };
        append(&mut out, &self.position);
        append(&mut out, &self.rotation);
        out.push(self.joints.len() as u8);
        for j in &self.joints {
            out.push(j.name.len() as u8);
            out.extend(j.name.as_bytes());
            append(&mut out, &j.translation);
            append(&mut out, &j.rotation);
            append(&mut out, &j.scale);
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
    if take::<2>(&mut r)? != [1, 1] {
        return Err(invalid());
    }
    let ride = u32::from_le_bytes(take(&mut r)?);
    let sequence = u32::from_le_bytes(take(&mut r)?);
    let sample_ms = u32::from_le_bytes(take(&mut r)?);
    let active = match take::<1>(&mut r)?[0] {
        0 => false,
        1 => true,
        _ => return Err(invalid()),
    };
    let position = floats(&mut r)?;
    let rotation = floats(&mut r)?;
    let count = take::<1>(&mut r)?[0] as usize;
    if count > MAX_JOINTS {
        return Err(invalid());
    }
    let mut joints = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take::<1>(&mut r)?[0] as usize;
        if !(1..=48).contains(&len) || r.len() < len {
            return Err(invalid());
        }
        let name = std::str::from_utf8(&r[..len])
            .map_err(|_| invalid())?
            .to_owned();
        r = &r[len..];
        joints.push(Joint {
            name,
            translation: floats(&mut r)?,
            rotation: floats(&mut r)?,
            scale: floats(&mut r)?,
        });
    }
    let snapshot = Snapshot {
        ride,
        sequence,
        sample_ms,
        active,
        position,
        rotation,
        joints,
    };
    snapshot.validate()?;
    if !r.is_empty() {
        return Err(invalid());
    }
    Ok(Packet::Snapshot(snapshot))
}

pub fn decode_relay(body: &[u8]) -> io::Result<Relay> {
    if body == HELLO {
        return Ok(Relay::Hello);
    }
    let mut r = body;
    let guid = u64::from_le_bytes(take(&mut r)?);
    let receipt_ms = u32::from_le_bytes(take(&mut r)?);
    match decode(r)? {
        Packet::Snapshot(snapshot) => Ok(Relay::Snapshot {
            guid,
            receipt_ms,
            snapshot,
        }),
        Packet::Hello => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            ride: 42,
            sequence: 9,
            sample_ms: u32::MAX,
            active: true,
            position: [1000.0, -20.0, 7.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            joints: vec![Joint {
                name: "SKATEBOARD_ROOT".into(),
                translation: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            }],
        }
    }

    #[test]
    fn roundtrip_and_relay_envelope() {
        let s = snapshot();
        let wire = s.encode().unwrap();
        assert_eq!(&wire[..6], &[1, 1, 42, 0, 0, 0]);
        assert_eq!(decode(&wire).unwrap(), Packet::Snapshot(s.clone()));
        let mut relay = 123u64.to_le_bytes().to_vec();
        relay.extend(456u32.to_le_bytes());
        relay.extend(wire);
        assert_eq!(
            decode_relay(&relay).unwrap(),
            Relay::Snapshot {
                guid: 123,
                receipt_ms: 456,
                snapshot: s
            }
        );
        assert_eq!(decode(&HELLO).unwrap(), Packet::Hello);
        assert_eq!(decode_relay(&HELLO).unwrap(), Relay::Hello);
    }

    #[test]
    fn rejects_every_truncation_and_trailing_bytes() {
        let wire = snapshot().encode().unwrap();
        for end in 0..wire.len() {
            assert!(decode(&wire[..end]).is_err(), "{end}");
        }
        let mut extra = wire;
        extra.push(0);
        assert!(decode(&extra).is_err());
        assert!(decode(&[1, 0, 0]).is_err());
        assert!(decode(&[2, 0]).is_err());
        assert!(decode(&vec![0; MAX_PAYLOAD + 1]).is_err());
        let mut wrapped_hello = vec![0; 12];
        wrapped_hello.extend(HELLO);
        assert!(decode_relay(&wrapped_hello).is_err());
    }

    #[test]
    fn rejects_malformed_fields_on_decode_and_encode() {
        let base = snapshot();
        let mut bad = Vec::new();
        let mut s = base.clone();
        s.position[0] = f32::NAN;
        bad.push(s);
        let mut s = base.clone();
        s.rotation = [0.0; 4];
        bad.push(s);
        let mut s = base.clone();
        s.joints[0].translation[0] = f32::INFINITY;
        bad.push(s);
        let mut s = base.clone();
        s.joints[0].rotation[3] = 2.0;
        bad.push(s);
        for scale in [0.0, -1.0, 100.01, f32::NAN] {
            let mut s = base.clone();
            s.joints[0].scale[0] = scale;
            bad.push(s);
        }
        for name in ["", "bad-name", "é", &"a".repeat(49)] {
            let mut s = base.clone();
            s.joints[0].name = name.into();
            bad.push(s);
        }
        let mut s = base.clone();
        s.joints.push(s.joints[0].clone());
        bad.push(s);
        let mut s = base.clone();
        s.active = false;
        bad.push(s);
        let mut s = base.clone();
        s.joints = vec![s.joints[0].clone(); 97];
        bad.push(s);
        for s in bad {
            assert!(s.encode().is_err());
        }
        let wire = base.encode().unwrap();
        for (offset, value) in [
            (0, 2),
            (1, 2),
            (14, 2),
            (43, 97),
            (44, 0),
            (44, 49),
            (45, b'-'),
        ] {
            let mut w = wire.clone();
            w[offset] = value;
            assert!(decode(&w).is_err(), "offset {offset}");
        }
        for (offset, value) in [
            (15, f32::NAN),
            (39, 0.0),
            (60, f32::INFINITY),
            (84, 2.0),
            (88, 0.0),
        ] {
            let mut w = wire.clone();
            w[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode(&w).is_err(), "offset {offset}");
        }
    }

    #[test]
    fn off_and_maximum_bounds() {
        let mut s = snapshot();
        s.active = false;
        s.joints.clear();
        assert_eq!(s.encode().unwrap().len(), 44);
        assert_eq!(decode(&s.encode().unwrap()).unwrap(), Packet::Snapshot(s));
        let mut s = snapshot();
        s.joints = (0..96)
            .map(|i| Joint {
                name: format!("B{i}"),
                ..s.joints[0].clone()
            })
            .collect();
        assert!(s.encode().is_ok());
        for j in &mut s.joints {
            j.name = format!("{:048}", j.name);
        }
        assert!(s.encode().is_err());
    }
}
