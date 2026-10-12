//! The data-independent WoW-combat integration in this crate needs no game files and always
//! builds. The `er-data` feature additionally compiles the vendored ELDEN RING simulation
//! (`third_party/elden-ring-combat-rewrite`), which is driven by an action table generated from
//! the player's own copy of the game. That table is never in the repository, so rather than let
//! the compiler fail on a missing module, say plainly what to do — exactly as the upstream
//! sandbox's own `build.rs` does.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=ER_EXTRACTED");
    if std::env::var_os("CARGO_FEATURE_ER_DATA").is_none() {
        return;
    }
    let generated = std::env::var_os("ER_EXTRACTED")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.setup/tarnished/src/sim/extracted.rs")
        });
    println!("cargo:rerun-if-changed={}", generated.display());
    if !generated.exists() {
        eprintln!();
        eprintln!("benilla-tarnished: the ELDEN RING action table is missing:");
        eprintln!("    {}", generated.display());
        eprintln!();
        eprintln!("It is generated from your own copy of the game and is not in the repository.");
        eprintln!("Follow the \"Tarnished combat\" section of README.md / INSTALL.md: unpack your");
        eprintln!("ELDEN RING files, set ER_FILES and ER_GAME_DIR, then run:");
        eprintln!();
        eprintln!("    python3 setup/tarnished.py");
        eprintln!();
        eprintln!("or build without the `er-data` feature (the default), which leaves Tarnished");
        eprintln!("combat unavailable at runtime with the same explanation.");
        eprintln!();
        std::process::exit(1);
    }
    let generated = generated.canonicalize().expect("resolve ER_EXTRACTED");
    println!("cargo:rustc-env=ER_EXTRACTED_PATH={}", generated.display());
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../third_party/elden-ring-combat-rewrite/src/sim");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let sim = out.join("sim");
    std::fs::create_dir_all(&sim).expect("create simulation build directory");
    // Keep the pinned vendor byte-identical; only the build copy switches vector imports.
    for name in ["mod", "data", "player", "dummy", "level", "tests"] {
        let path = source.join(format!("{name}.rs"));
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(path)
            .expect("read pinned simulation")
            .replace("bevy::math::", "glam::")
            .replace(
                "pub mod extracted;",
                &format!("#[path = {:?}]\npub mod extracted;", generated),
            )
            .replace(
                "mod tests;",
                "#[allow(clippy::needless_range_loop)]\nmod tests;",
            );
        std::fs::write(sim.join(format!("{name}.rs")), text).expect("write simulation build copy");
    }
    std::fs::write(
        out.join("sim_entry.rs"),
        format!(
            "#[allow(dead_code)]\n#[path = {:?}]\nmod sim;\n",
            sim.join("mod.rs")
        ),
    )
    .expect("write simulation module");
}
