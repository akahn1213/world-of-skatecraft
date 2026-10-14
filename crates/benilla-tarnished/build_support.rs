//! Transform only the generated copy of the pinned animation loader.
pub fn adapt_anim(source: &str) -> String {
    let anim = source.replace("\r\n", "\n");
    let anim = anim
        .replace("//!", "//")
        .replace("use bevy::prelude::*;", "use glam::Vec3;")
        .replace("#[derive(Resource)]", "")
        .replace("joints: Vec<String>", "pub joints: Vec<String>")
        .replace("oriented: Vec<String>", "pub oriented: Vec<String>")
        .replace("pub fn load() -> io::Result<Self> {\n        let data = fs::read(PATH)?;", "pub fn load(path: &std::path::Path) -> io::Result<Self> {\n        let data = fs::read(path)?;")
        .replace("let mut c = Cursor { data: &data, at: 0 };", "Self::from_bytes(&data)\n    }\n    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {\n        let mut c = Cursor { data, at: 0 };")
        .replace("let joints = (0..joint_count)", "if joint_count == 0 || joint_count > 96 || oriented_count > 96 || clip_count > 10000 {\nreturn Err(io::Error::new(io::ErrorKind::InvalidData, \"invalid animation counts\"));\n}\nlet joints = (0..joint_count)")
        .replace("let frames = c.u32()? as usize;", "let frames = c.u32()? as usize;\nif frames == 0 || frames > 36000 || !blend.is_finite() || blend < 0.0 {\nreturn Err(io::Error::new(io::ErrorKind::InvalidData, \"invalid animation clip\"));\n}")
        .replace(".collect();\n            clips.insert", ".collect::<Vec<_>>();\nif data.iter().any(|v| !v.is_finite()) {\nreturn Err(io::Error::new(io::ErrorKind::InvalidData, \"nonfinite animation pose\"));\n}\n            clips.insert")
        .replace("raw.chunks_exact(4)", "raw.as_chunks::<4>().0.iter()")
        .replace("f32::from_le_bytes(b.try_into().unwrap())", "f32::from_le_bytes(*b)")
        .replace("Ok(Self { joints, oriented, stride, clips })", "if c.at != data.len() { return Err(io::Error::new(io::ErrorKind::InvalidData, \"trailing animation bytes\")); }\nOk(Self { joints, oriented, stride, clips })");
    assert!(
        anim.contains("pub fn load(path: &std::path::Path)"),
        "pinned anim.rs load signature changed"
    );
    assert!(
        !anim.contains("fs::read(PATH)"),
        "pinned anim.rs still reads a fixed path"
    );
    assert!(
        anim.contains("pub fn from_bytes(data: &[u8])"),
        "pinned anim.rs byte loader changed"
    );
    for expected in [
        "invalid animation counts",
        "invalid animation clip",
        "nonfinite animation pose",
        "trailing animation bytes",
    ] {
        assert!(
            anim.contains(expected),
            "pinned anim.rs validation transformation failed: {expected}"
        );
    }
    anim
}

#[cfg(test)]
mod tests {
    use super::adapt_anim;
    const PINNED: &str = include_str!("../../third_party/elden-ring-combat-rewrite/src/anim.rs");
    #[test]
    fn checkout_line_endings_produce_identical_loader() {
        let lf = PINNED.replace("\r\n", "\n");
        assert_eq!(adapt_anim(&lf), adapt_anim(&lf.replace('\n', "\r\n")));
    }
    #[test]
    #[should_panic(expected = "load signature changed")]
    fn upstream_drift_fails_during_generation() {
        adapt_anim(&PINNED.replace("pub fn load()", "pub fn load_new()"));
    }
}
