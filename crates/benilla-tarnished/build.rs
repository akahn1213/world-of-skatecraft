//! The data-independent WoW-combat integration in this crate needs no game files and always
//! builds. The `er-data` feature additionally compiles the vendored ELDEN RING simulation
//! (`third_party/elden-ring-combat-rewrite`), which is driven by an action table generated from
//! the player's own copy of the game. That table is never in the repository, so rather than let
//! the compiler fail on a missing module, say plainly what to do — exactly as the upstream
//! sandbox's own `build.rs` does.

use std::path::PathBuf;

fn main() {
    if std::env::var_os("CARGO_FEATURE_ER_DATA").is_none() {
        return;
    }
    // Where the fork places its generated action table, overridable for an out-of-tree layout.
    let generated = std::env::var_os("ER_EXTRACTED")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../third_party/elden-ring-combat-rewrite/src/sim/extracted.rs")
        });
    println!("cargo:rerun-if-changed={}", generated.display());
    println!("cargo:rerun-if-env-changed=ER_EXTRACTED");
    if !generated.exists() {
        eprintln!();
        eprintln!("benilla-tarnished: the ELDEN RING action table is missing:");
        eprintln!("    {}", generated.display());
        eprintln!();
        eprintln!("It is generated from your own copy of the game and is not in the repository.");
        eprintln!("Follow the \"Tarnished combat\" section of README.md / INSTALL.md: unpack your");
        eprintln!("ELDEN RING files, set ER_FILES and ER_GAME_DIR, then run:");
        eprintln!();
        eprintln!("    python third_party/elden-ring-combat-rewrite/tools/setup.py");
        eprintln!();
        eprintln!("or build without the `er-data` feature (the default), which leaves Tarnished");
        eprintln!("combat unavailable at runtime with the same explanation.");
        eprintln!();
        std::process::exit(1);
    }
    println!("cargo:rustc-env=ER_EXTRACTED_PATH={}", generated.display());
}
