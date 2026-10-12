//! The runtime profile: the bridge between the game-derived numbers and benilla's WoW glue.
//!
//! benilla never ships ELDEN RING data. The faithful motion values, guard absorptions and the
//! like come from a *profile* the player generates from their own copy of the game during setup
//! (`third_party/elden-ring-combat-rewrite/tools`). benilla-app loads that profile at runtime: if
//! it is present the Tarnished numbers are the game's own; if it is absent the feature is
//! unavailable with a precise message ([`crate::unavailable`]), never a silent default.
//!
//! The schema here is deliberately small and text-based so it carries no game data in the
//! repository — only the shape a generated file must fill. Each line is `weapon_code key=value
//! ...`; unknown keys are rejected so a schema drift is caught rather than silently ignored.

use std::collections::HashMap;

use crate::defence::Absorption;

/// One weapon's profile entry: the per-class numbers the generated file provides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponProfile {
    /// Light-attack motion value, scaled onto the WoW weapon in [`crate::scale`].
    pub light_mv: f32,
    /// Heavy-attack motion value.
    pub heavy_mv: f32,
    /// Guard absorption when this weapon (or its paired shield) guards.
    pub guard: Absorption,
}

/// A loaded profile: a weapon code → [`WeaponProfile`] table. Produced only from a generated
/// file; this crate never hard-codes one.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    by_code: HashMap<u8, WeaponProfile>,
}

/// Why a profile could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// The profile file was not generated. Carries [`crate::unavailable`]'s explanation.
    Absent,
    /// A line could not be parsed; carries the 1-based line number and reason.
    Malformed { line: usize, reason: String },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Absent => f.write_str(crate::unavailable()),
            ProfileError::Malformed { line, reason } => {
                write!(f, "Tarnished profile is malformed at line {line}: {reason}")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// The profile entry for an ELDEN RING weapon code, if the generated file carried one.
    pub fn get(&self, code: u8) -> Option<&WeaponProfile> {
        self.by_code.get(&code)
    }

    /// Number of weapon classes the profile covers.
    pub fn len(&self) -> usize {
        self.by_code.len()
    }

    /// No weapon classes loaded.
    pub fn is_empty(&self) -> bool {
        self.by_code.is_empty()
    }

    /// Load a profile from the generated file at `path`. A missing file is [`ProfileError::Absent`]
    /// — the expected state of a build without the player's data — not a hard error.
    pub fn load(path: &std::path::Path) -> Result<Self, ProfileError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ProfileError::Absent),
            Err(e) => Err(ProfileError::Malformed {
                line: 0,
                reason: e.to_string(),
            }),
        }
    }

    /// Parse a profile from its text form. Lines are `code light=.. heavy=.. phys=.. magic=..`;
    /// blank lines and `#` comments are skipped. Exposed for tests and for a fork that generates
    /// the text in memory.
    pub fn parse(text: &str) -> Result<Self, ProfileError> {
        let mut by_code = HashMap::new();
        for (i, raw) in text.lines().enumerate() {
            let line = i + 1;
            let content = raw.split('#').next().unwrap_or("").trim();
            if content.is_empty() {
                continue;
            }
            let mut fields = content.split_whitespace();
            let code = fields
                .next()
                .ok_or_else(|| malformed(line, "missing weapon code"))?
                .parse::<u8>()
                .map_err(|_| malformed(line, "weapon code is not a number"))?;
            let (mut light, mut heavy, mut phys, mut magic) = (None, None, None, None);
            for field in fields {
                let (key, value) = field
                    .split_once('=')
                    .ok_or_else(|| malformed(line, "expected key=value"))?;
                let num = value
                    .parse::<f32>()
                    .map_err(|_| malformed(line, "value is not a number"))?;
                match key {
                    "light" => light = Some(num),
                    "heavy" => heavy = Some(num),
                    "phys" => phys = Some(num),
                    "magic" => magic = Some(num),
                    other => return Err(malformed(line, &format!("unknown key `{other}`"))),
                }
            }
            let entry = WeaponProfile {
                light_mv: light.ok_or_else(|| malformed(line, "missing `light`"))?,
                heavy_mv: heavy.ok_or_else(|| malformed(line, "missing `heavy`"))?,
                guard: Absorption {
                    physical: phys.unwrap_or(1.0),
                    magic: magic.unwrap_or(0.0),
                },
            };
            if by_code.insert(code, entry).is_some() {
                return Err(malformed(line, "duplicate weapon code"));
            }
        }
        Ok(Self { by_code })
    }
}

fn malformed(line: usize, reason: &str) -> ProfileError {
    ProfileError::Malformed {
        line,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // An in-memory fixture, not game data: it exercises the schema without any real ELDEN RING
    // numbers. Real motion values come only from the player's generated file.
    const FIXTURE: &str = "\
# code light heavy phys magic
0 light=1.0 heavy=2.6 phys=1.0 magic=0.0
24 light=0.0 heavy=0.0 phys=1.0 magic=0.2  # shield
";

    #[test]
    fn parses_a_fixture() {
        let p = Profile::parse(FIXTURE).unwrap();
        assert_eq!(p.len(), 2);
        let sword = p.get(0).unwrap();
        assert_eq!(sword.light_mv, 1.0);
        assert_eq!(sword.heavy_mv, 2.6);
        assert_eq!(sword.guard.physical, 1.0);
        let shield = p.get(24).unwrap();
        assert_eq!(shield.guard.magic, 0.2);
    }

    #[test]
    fn missing_file_is_absent_not_an_error() {
        let err = Profile::load(std::path::Path::new(
            "crates/benilla-tarnished/this-file-does-not-exist.profile",
        ))
        .unwrap_err();
        assert_eq!(err, ProfileError::Absent);
        assert!(err.to_string().contains("Tarnished combat is unavailable"));
    }

    #[test]
    fn an_unknown_key_is_rejected() {
        let err = Profile::parse("0 light=1 heavy=1 poise=5").unwrap_err();
        assert!(matches!(err, ProfileError::Malformed { .. }));
        assert!(err.to_string().contains("unknown key"));
    }

    #[test]
    fn a_missing_required_field_is_rejected() {
        let err = Profile::parse("0 light=1").unwrap_err();
        assert!(err.to_string().contains("missing `heavy`"));
    }

    #[test]
    fn a_duplicate_code_is_rejected() {
        let err = Profile::parse("0 light=1 heavy=1\n0 light=2 heavy=2").unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }
}
