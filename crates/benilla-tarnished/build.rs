//! The data-independent WoW-combat integration in this crate needs no game files and always
//! builds. The `er-data` feature additionally compiles the vendored ELDEN RING simulation
//! (`third_party/elden-ring-combat-rewrite`), which is driven by an action table generated from
//! the player's own copy of the game. That table is never in the repository, so rather than let
//! the compiler fail on a missing module, say plainly what to do — exactly as the upstream
//! sandbox's own `build.rs` does.

mod build_support;

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
                // Sampled coordinates are measurements, not approximations of math constants.
                &format!(
                    "#[allow(clippy::approx_constant)]\n#[path = {:?}]\npub mod extracted;",
                    generated
                ),
            )
            .replace(
                "mod tests;",
                "#[allow(clippy::needless_range_loop)]\nmod tests;",
            );
        let mut text = text;
        if name == "player" {
            // Host terrain owns support. Rebase the authored arc without changing its samples,
            // and let real collision trigger upstream landing/air-attack transitions.
            text.push_str(
                r#"
impl Player {
    // The host already chose this block at visible contact. Never re-test a later roll/guard.
    pub fn host_guard_impact(&mut self, stamina_damage: f32, broken: bool) {
        if self.is_dead() { return; }
        self.stamina = (self.stamina - stamina_damage * GUARD_STAMINA_TAKEN).max(0.0);
        if broken || self.stamina <= 0.0 {
            self.start(ActionId::GuardBreak);
        } else {
            self.start(ActionId::GuardHit);
            self.guard_counter = GUARD_COUNTER_WINDOW;
        }
    }
    pub fn host_rebase_y(&mut self, y: f32) {
        let delta = y - self.pos.y;
        self.jump_base += delta;
        self.peak += delta;
        self.pos.y = y;
    }
    pub fn host_land(&mut self, inp: &Input, locked: bool) {
        let jumped = match self.state {
            State::Act(a) => matches!(a.id, ActionId::Jump(_)),
            State::Air(a) => a.jumped && !a.falling(),
            _ => return,
        };
        // WoW owns fall damage, including death; only use the reference landing selection.
        self.peak = self.peak.min(self.pos.y + FALL_DAMAGE_START - 0.01);
        self.land(self.pos.y, inp, jumped, locked);
    }
}
"#,
            );
        }
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
    // Upstream is a Bevy sandbox binary, not a Cargo library. Keep its pure animation
    // selection and layering code verbatim apart from the host's types/clock plumbing.
    let vendor = source.parent().unwrap();
    println!(
        "cargo:rerun-if-changed={}",
        vendor.join("anim.rs").display()
    );
    let anim =
        std::fs::read_to_string(vendor.join("anim.rs")).expect("read pinned animation loader");
    let anim = build_support::adapt_anim(&anim);
    std::fs::write(out.join("anim.rs"), anim).expect("write animation loader");
    let rig =
        std::fs::read_to_string(vendor.join("rig.rs")).expect("read pinned animation mechanism");
    let take = |start: &str, end: &str| {
        let start = rig.find(start).expect("pinned rig start");
        let end = rig[start..].find(end).expect("pinned rig end") + start;
        &rig[start..end]
    };
    let constants = take("const IDLE:", "/// The finger joints");
    let playing =
        take("fn playing(", "/// Rotation taking").replace("rig: &mut Rig", "rig: &mut Animator");
    let sampling = take("    // Frozen on a hit", "    // Show whatever")
        .replace("rendered.alpha * DF", "ahead")
        .replace("time.elapsed_secs()", "elapsed")
        .replace("rig, &clips,", "rig, clips,")
        .replace("&mut target", "target")
        .replace("extend_from_slice(&target)", "extend_from_slice(target)")
        .replacen("return;", "return Vec::new();", 1)
        .replace(
            "crate::audio::heard_frame(frame, looped, clip.frames)",
            "frame",
        );
    std::fs::write(out.join("animation_mechanism.rs"), format!(
        "{constants}\n{playing}\n\
         fn sample_pose(player: &Player, rig: &mut Animator, clips: &Clips, elapsed: f32, dt: f32, ahead: f32) -> Vec<f32> {{\n\
         let mut buffer = Vec::new(); let target = &mut buffer; let mut layer = Vec::new();\n\
         {sampling}\npose\n}}\n"
    )).expect("write pinned animation mechanism");
    let camera = std::fs::read_to_string(vendor.join("camera.rs")).expect("read pinned camera");
    let constants =
        &camera[camera.find("const DISTANCE:").unwrap()..camera.find("pub fn setup(").unwrap()];
    let look =
        &camera[camera.find("    cam.idle =").unwrap()..camera.find("\npub fn follow(").unwrap()];
    let look = look.replace("world.target()", "target");
    let follow = &camera[camera.find("    let goal = rendered.pos").unwrap()
        ..camera.find("    // Pull the camera").unwrap()];
    let follow = follow.replace("rendered.pos", "player.pos");
    std::fs::write(out.join("camera_mechanism.rs"), format!(
        "{}\nfn look(cam: &mut CamRig, player: &Player, target: Option<Vec3>, delta: Vec2, dt: f32) {{\n{look}\n\
         fn follow(cam: &mut CamRig, player: &Player, dt: f32) -> ([f32; 3], [f32; 3]) {{\n\
         {follow}\n((cam.pivot - look * DISTANCE).to_array(), cam.pivot.to_array())\n}}\n",
        constants.replace("#[derive(Resource)]", "")
    )).expect("write pinned camera mechanism");
    for name in ["anim", "rig", "camera"] {
        println!(
            "cargo:rerun-if-changed={}",
            vendor.join(format!("{name}.rs")).display()
        );
    }
}
