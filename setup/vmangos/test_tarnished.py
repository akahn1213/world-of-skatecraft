#!/usr/bin/env python3
"""Standalone checks of the patch's actual C++ validators and the image build wrapper."""
import hashlib
import os
from pathlib import Path
import re
import json
import shlex
import shutil
import subprocess
import unittest

from prepare_patch import prepare

ROOT = Path(__file__).resolve().parents[2]
PATCH = ROOT / "setup/vmangos/tarnished.patch"


def added_file(name):
    text = PATCH.read_text()
    section = text.split(f"diff --git a/{name} b/{name}\n", 1)[1]
    section = section.split("\ndiff --git ", 1)[0]
    return "\n".join(
        line[1:] for line in section.splitlines()
        if line.startswith("+") and not line.startswith("+++")
    )


def definition(text, marker, structure=False):
    start = text.index(marker)
    begin = text.index("{", start)
    depth = 1
    end = begin + 1
    while depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    return text[start:end + int(structure)]


class TarnishedServerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = ROOT / ".setup" / f"tarnished-tests-{os.getpid()}"
        cls.scratch.mkdir(parents=True)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.scratch)

    def test_actual_cpp_equipment_spell_policy_and_queued_replacement(self):
        cpp = added_file("src/game/Tarnished.cpp")
        code = r"""
#include <cassert>
#include <cstdint>
using uint8 = uint8_t; using uint32 = uint32_t;
constexpr int ITEM_CLASS_ARMOR=4, ITEM_CLASS_WEAPON=2, ITEM_SUBCLASS_ARMOR_SHIELD=6;
constexpr int INVTYPE_2HWEAPON=17, BASE_ATTACK=0, INVENTORY_SLOT_BAG_0=0, EQUIPMENT_SLOT_OFFHAND=16;
constexpr int CURRENT_MELEE_SPELL=0, SPELL_DAMAGE_CLASS_MELEE=2;
struct ItemProto { int Class=2, SubClass=7, InventoryType=13; };
struct Item {
    ItemProto proto; bool broken=false;
    bool IsBroken() const { return broken; }
    ItemProto const* GetProto() const { return &proto; }
};
struct Unit {};
struct SpellEntry {
    int DmgClass=0; bool queued=false;
    bool IsNextMeleeSwingSpell() const { return queued; }
};
struct Spell {
    bool m_tarnishedWeaponOverride=false, success=true;
    uint32 m_tarnishedWeaponDamage=0; int casts=0;
    struct Targets { Unit* target=nullptr; void setUnitTarget(Unit* unit) { target=unit; } } m_targets;
    void cast() { ++casts; }
    bool isSuccessCast() const { return success; }
};
struct Player {
    Item* main=nullptr; Item* off=nullptr; Spell* queued=nullptr;
    Item* GetWeaponForAttack(int, bool, bool) const { return main; }
    Item* GetItemByPos(int, int) const { return off; }
    Spell* GetCurrentSpell(int) const { return queued; }
};
"""
        for marker in ["static uint8 Weapon(", "static bool Armament(", "bool MeleeAbility(", "static bool ReplaceSwing("]:
            code += definition(cpp, marker) + "\n"
        code += r"""
int main() {
    Player p; Unit target; Item right, left;
    assert(Armament(&p, 20, 0));
    p.main=&right;
    int subclasses[]={0,1,4,5,6,7,8,10,13,15,2,3,18,19,20};
    int codes[]={7,16,6,14,9,1,3,13,20,0,20,20,20,20,20};
    for (int i=0;i<15;++i) {
        right.proto.SubClass=subclasses[i];
        assert(Armament(&p, codes[i], 0));
        assert(!Armament(&p, 24, 0));
    }
    right.proto.SubClass=7; p.off=&left;
    assert(Armament(&p, 1, 26)); // paired swords
    left.proto.SubClass=15;
    assert(Armament(&p, 0, 20)); // offhand dagger
    assert(!Armament(&p, 1, 26)); // mixed hands cannot use paired swords
    right.proto.InventoryType=17;
    assert(!Armament(&p, 0, 20)); // two-hander puts the off hand away
    right.broken=true; assert(Armament(&p, 20, 0));
    for (int cls=0;cls<4;++cls) {
        SpellEntry s; s.DmgClass=cls;
        assert(MeleeAbility(&s)==(cls==2));
        s.queued=true; assert(!MeleeAbility(&s));
    }
    assert(!ReplaceSwing(&p, &target, 200, 0));
    Spell spell; p.queued=&spell;
    assert(ReplaceSwing(&p, &target, 260, 0));
    assert(spell.casts==1 && spell.m_targets.target==&target);
    assert(spell.m_tarnishedWeaponOverride && spell.m_tarnishedWeaponDamage==260);
    assert(ReplaceSwing(&p, &target, 520, 1));
    assert(spell.m_tarnishedWeaponDamage==260); // native spell rolls crit once
    spell.success=false;
    assert(!ReplaceSwing(&p, &target, 100, 0));
    assert(!spell.m_tarnishedWeaponOverride); // failed queued cast permits normal swing
}
"""
        source = self.scratch / "gameplay.cpp"
        binary = self.scratch / "gameplay"
        source.write_text(code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_actual_cpp_allowed_accepts_neutral_creatures_but_preserves_restrictions(self):
        cpp = added_file("src/game/Tarnished.cpp")
        code = r"""
#include <cassert>
struct Unit {
    bool player = false, creature = true, hostile = true, friendly = false;
    bool world = true, alive = true, los = true, range = true, evade = false;
    bool targetable = true, detectable = true;
    int map = 1;
    bool IsPlayer() const { return player; }
    bool IsCreature() const { return creature; }
    Unit const* ToCreature() const { return this; }
    bool IsInEvadeMode() const { return evade; }
    bool IsInWorld() const { return world; }
    bool IsAlive() const { return alive; }
    int GetMap() const { return map; }
    bool IsWithinLOSInMap(Unit const* target) const { return target->los; }
    bool IsWithinDistInMap(Unit const* target, float) const { return target->range; }
    // Pinned Unit::CanAttack: force permits nonfriendly player-initiated attacks,
    // then still checks IsTargetableBy and invisibility detection.
    bool CanAttack(Unit const* target, bool force = false) const {
        return !target->friendly && (target->hostile || (force && player)) &&
            target->targetable && target->detectable;
    }
};
"""
        code += definition(cpp, "static bool Allowed")
        code += r"""
int main() {
    Unit source; source.player = true; source.creature = false;
    Unit target;
    assert(Allowed(&source, &target, true));
    target.hostile = false;
    assert(Allowed(&source, &target, true));
    assert(!Allowed(&source, &target, false)); // leave unrelated spell policy unchanged
    target.friendly = true; assert(!Allowed(&source, &target, true)); target.friendly = false;
    target.creature = false; target.player = true;
    assert(!Allowed(&source, &target, true)); // no broadened player-vs-player permission
    target = Unit{};
    assert(!Allowed(nullptr, &target, true));
    assert(!Allowed(&source, nullptr, true));
    assert(!Allowed(&source, &source, true));
    for (bool Unit::* field : {&Unit::world, &Unit::alive, &Unit::los, &Unit::range,
                               &Unit::targetable, &Unit::detectable}) {
        target.*field = false; assert(!Allowed(&source, &target, true)); target.*field = true;
    }
    target.evade = true; assert(!Allowed(&source, &target, true)); target.evade = false;
    target.map = 2; assert(!Allowed(&source, &target, true)); target.map = 1;
    source.alive = false; assert(!Allowed(&source, &target, true)); source.alive = true;
    source.world = false; assert(!Allowed(&source, &target, true));
}
"""
        source = self.scratch / "allowed.cpp"
        binary = self.scratch / "allowed"
        source.write_text("#include <initializer_list>\n" + code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror",
                        str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_actual_cpp_contact_batches_do_not_multiply_swing_damage(self):
        cpp = added_file("src/game/Tarnished.cpp")
        header = added_file("src/game/Tarnished.h")
        code = r"""
#include <cassert>
#include <cstdint>
#include <deque>
#include <limits>
#include <unordered_set>
#include <vector>
using uint8 = uint8_t; using uint32 = uint32_t; using uint64 = uint64_t;
struct Pending {};
"""
        code += definition(header, "struct State", structure=True) + "\n"
        code += definition(cpp, "static bool Newer") + "\n"
        code += definition(cpp, "static bool ContactBatch") + "\n"
        code += r"""
int main() {
    State s;
    assert(ContactBatch(s, 1, 1, 0, {10}));
    s.hasAttack = true; s.attackId = 1; s.attackWeapon = 1; s.attackKind = 0;
    s.attackTargets = {10};
    assert(ContactBatch(s, 1, 1, 0, {11}));
    assert(!ContactBatch(s, 1, 1, 0, {10}));
    assert(!ContactBatch(s, 1, 1, 1, {11}));
    assert(!ContactBatch(s, 1, 24, 0, {11}));
    assert(!ContactBatch(s, 1, 1, 0, {11}, 20000));
    assert(!ContactBatch(s, 0, 1, 0, {11}));
    assert(ContactBatch(s, 2, 1, 0, {10}));
    s.attackStamina = 20000;
    assert(ContactBatch(s, 1, 1, 0, {11}, 20000));
    assert(!ContactBatch(s, 1, 1, 0, {11}, 19000));
    s.attackStamina = UINT32_MAX;
    for (uint64 guid = 11; guid <= 25; ++guid) s.attackTargets.insert(guid);
    assert(!ContactBatch(s, 1, 1, 0, {26}));
    s.attackId = UINT32_MAX;
    assert(ContactBatch(s, 0, 1, 0, {26}));
}
"""
        source = self.scratch / "contact_batches.cpp"
        binary = self.scratch / "contact_batches"
        source.write_text(code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror",
                        str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_actual_cpp_pose_and_defence_validation(self):
        cpp = added_file("src/game/Tarnished.cpp")
        header = added_file("src/game/Tarnished.h")
        code = r"""
#include <algorithm>
#include <limits>
#include <cassert>
#include <cmath>
#include <cstring>
#include <deque>
#include <functional>
#include <limits>
#include <string>
#include <unordered_set>
#include <vector>
#include <cstdint>
using uint8 = uint8_t; using uint16 = uint16_t;
using uint32 = uint32_t; using uint64 = uint64_t;
struct Unit {}; class Player;
struct ObjectGuid { uint64 value = 0; operator uint64() const { return value; } };
static uint32 clockMs = 100;
struct WorldTimer {
    static uint32 getMSTime() { return clockMs; }
    static uint32 getMSTimeDiff(uint32 a, uint32 b) { return b - a; }
};
class ByteBuffer {
public:
    std::vector<uint8> bytes; size_t cursor = 0;
    size_t size() const { return bytes.size(); }
    size_t rpos() const { return cursor; }
    void rpos(size_t p) { cursor = p; }
    template<class T> ByteBuffer& operator>>(T& value) {
        assert(cursor + sizeof(T) <= bytes.size());
        std::memcpy(&value, bytes.data() + cursor, sizeof(T));
        cursor += sizeof(T); return *this;
    }
    template<class T> void put(T value) {
        auto p = reinterpret_cast<uint8 const*>(&value);
        bytes.insert(bytes.end(), p, p + sizeof(T));
    }
};
namespace Tarnished {
constexpr uint32 TIMEOUT = 350;
constexpr uint32 VISUAL_TIMEOUT = 2500;
"""
        for name in ("Answer", "Pending", "State"):
            code += definition(header, f"struct {name}", True) + "\n"
        code += "}\nclass Player { public: Tarnished::State tarnished; bool active = true; };\n"
        code += """
namespace Tarnished {
static unsigned resolved = 0;
static Answer last;
static bool Active(Player* p) { return p->active; }
static void Resolve(Player*, Pending, Answer const* a) { ++resolved; last = *a; }
"""
        for name in ("Newer", "Float", "Transform", "Pose", "PoseOrder", "Budget", "Defence"):
            code += definition(cpp, f"static {'void' if name == 'Defence' else 'bool'} {name}") + "\n"
        code += r"""
}
static ByteBuffer pose(bool active, uint8 joints = 0) {
    ByteBuffer b; b.put<uint8>(1); b.put<uint8>(4);
    b.put<uint32>(1); b.put<uint32>(1); b.put<uint32>(10);
    b.put<uint8>(active); b.put<uint8>(0); b.put<uint16>(0);
    for (unsigned i = 0; i < 6; ++i) b.put<float>(0.f);
    b.put<float>(1.f); b.put<uint8>(joints); return b;
}
static void joint(ByteBuffer& b, std::string name, float scale = 1.f) {
    b.put<uint8>(name.size());
    for (auto ch : name) b.put<uint8>(ch);
    for (unsigned i = 0; i < 6; ++i) b.put<float>(0.f);
    b.put<float>(1.f);
    for (unsigned i = 0; i < 3; ++i) b.put<float>(scale);
}
static bool valid(ByteBuffer const& b) {
    uint32 bout, seq; bool active; return Tarnished::Pose(b, bout, seq, active);
}
static ByteBuffer verdict(uint8 outcome, uint32 seq = 1, uint32 token = 77, uint64 guid = 99) {
    ByteBuffer b; b.put<uint8>(1); b.put<uint8>(3);
    b.put<uint32>(seq); b.put<uint32>(token); b.put<uint64>(guid); b.put<uint8>(outcome);
    if (outcome == 4) b.put<uint32>(100);
    if (outcome == 3) {
        b.put<uint32>(40); b.put<uint16>(10); b.put<uint8>(0); b.put<uint32>(60);
    }
    return b;
}
static Player defender(bool melee = true) {
    Player p; Tarnished::Pending pending;
    pending.token = 77; pending.receipt = 90; pending.source.value = 99;
    pending.amount = 100; pending.melee = melee;
    p.tarnished.pending.push_back(pending); return p;
}
int main() {
    Tarnished::State state;
    assert(!Tarnished::PoseOrder(state, 4, 1, 1, true));
    assert(Tarnished::PoseOrder(state, 1, 1, 1, true));
    state.hasPose = state.active = true; state.bout = 1; state.poseSequence = 1;
    assert(Tarnished::PoseOrder(state, 4, 1, 2, true));
    assert(!Tarnished::PoseOrder(state, 4, 1, 2, false));
    assert(!Tarnished::PoseOrder(state, 4, 2, 2, true));
    assert(Tarnished::PoseOrder(state, 1, 1, 2, false));
    state.active = false; state.blockedBout = true; state.retiredBout = 1;
    assert(!Tarnished::PoseOrder(state, 4, 1, 3, true));
    assert(!Tarnished::PoseOrder(state, 1, 1, 3, true));
    assert(Tarnished::PoseOrder(state, 1, 2, 3, true));
    assert(valid(pose(false))); assert(valid(pose(true)));
    auto b = pose(true, 1); joint(b, "ROOT"); assert(valid(b));
    auto trailing = b; trailing.put<uint8>(0); assert(!valid(trailing));
    for (size_t n = 0; n < b.size(); ++n) {
        auto truncated = b; truncated.bytes.resize(n); assert(!valid(truncated));
    }
    auto duplicate = pose(true, 2); joint(duplicate, "ROOT"); joint(duplicate, "ROOT");
    assert(!valid(duplicate));
    auto badName = pose(true, 1); joint(badName, "ROOT!"); assert(!valid(badName));
    auto badScale = pose(true, 1); joint(badScale, "ROOT", 0.f); assert(!valid(badScale));
    auto offWithJoints = b; offWithJoints.bytes[14] = 0; assert(!valid(offWithJoints));
    auto badBool = b; badBool.bytes[15] = 2; assert(!valid(badBool));
    auto badQuat = pose(true); float zero = 0.f;
    std::memcpy(badQuat.bytes.data() + 42, &zero, 4); assert(!valid(badQuat));
    auto nan = pose(true); float value = NAN;
    std::memcpy(nan.bytes.data() + 18, &value, 4); assert(!valid(nan));
    assert(Tarnished::Newer(0, UINT32_MAX)); assert(!Tarnished::Newer(1, 1));
    assert(!Tarnished::Newer(0x80000000, 0));
    auto p = defender(); auto v = verdict(4);
    Tarnished::Defence(&p, v, 100); assert(Tarnished::resolved == 1 && p.tarnished.pending.empty());
    Tarnished::Defence(&p, v, 100); assert(Tarnished::resolved == 1);
    p = defender(); Tarnished::Defence(&p, verdict(4, 1, 78), 100);
    assert(Tarnished::resolved == 1 && p.tarnished.pending.size() == 1);
    Tarnished::Defence(&p, verdict(4, 1, 77, 98), 100); assert(Tarnished::resolved == 1);
    p.active = false; Tarnished::Defence(&p, v, 100); assert(Tarnished::resolved == 1);
    p = defender(false); Tarnished::Defence(&p, verdict(1), 100); assert(Tarnished::resolved == 1);
    p = defender(); auto badAmount = verdict(4); badAmount.bytes[19] = 99;
    Tarnished::Defence(&p, badAmount, 100); assert(Tarnished::resolved == 1);
    p = defender(); auto block = verdict(3); block.bytes[25] = 2;
    Tarnished::Defence(&p, block, 100); assert(Tarnished::resolved == 1);
    block = verdict(3); block.bytes[26] = 61;
    Tarnished::Defence(&p, block, 100); assert(Tarnished::resolved == 1);
    block = verdict(3);
    Tarnished::Defence(&p, block, 100); assert(Tarnished::resolved == 2);
    assert(Tarnished::last.absorbed == 40 && Tarnished::last.taken == 60);
    p = defender(); clockMs = 440;
    Tarnished::Defence(&p, v, 100); assert(Tarnished::resolved == 2); clockMs = 100;
    p = defender(); p.tarnished.hasCommand = true; p.tarnished.commandSequence = 1;
    Tarnished::Defence(&p, v, 100); assert(Tarnished::resolved == 2);
    p = defender(); p.tarnished.commandTokens = 0; p.tarnished.budgetTime = 100;
    Tarnished::Defence(&p, v, 100); assert(Tarnished::resolved == 2);
    p = defender(false); Tarnished::Defence(&p, verdict(0), 100); assert(Tarnished::resolved == 3);
    // A visual contact may occur after the old 350 ms deadline. Both receipt and processing
    // must use this pending hit's deadline; an ordinary ER contact retains its short bound.
    p = defender(); p.tarnished.pending.front().timeout = Tarnished::VISUAL_TIMEOUT;
    clockMs = 800; Tarnished::Defence(&p, verdict(0), 790); assert(Tarnished::resolved == 4);
    p = defender(false); p.tarnished.pending.front().timeout = Tarnished::VISUAL_TIMEOUT;
    Tarnished::Defence(&p, verdict(3), 790); assert(Tarnished::resolved == 5);
    p = defender(); Tarnished::Defence(&p, verdict(0), 790); assert(Tarnished::resolved == 5);
    p = defender(); p.tarnished.pending.front().timeout = Tarnished::VISUAL_TIMEOUT;
    clockMs = 2590; Tarnished::Defence(&p, verdict(0), 2580); assert(Tarnished::resolved == 5);
}
"""
        source = self.scratch / "validator.cpp"
        binary = self.scratch / "validator"
        source.write_text(code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror",
                        str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_build_cache_and_repo_local_context(self):
        root = self.scratch / "wrapper"
        (root / "setup/vmangos").mkdir(parents=True)
        (root / "deploy/docker/server").mkdir(parents=True)
        (root / "deploy/docker/patches").mkdir()
        shutil.copyfile(ROOT / "setup/build-server.sh", root / "setup/build-server.sh")
        shutil.copyfile(ROOT / "setup/vmangos/prepare_patch.py",
                        root / "setup/vmangos/prepare_patch.py")
        patches = root / "setup/vmangos"
        (patches / "skatecraft.patch").write_text("skate\n")
        (patches / "tarnished.patch").write_text("tarnished\n")
        (root / "deploy/docker/server/Dockerfile").write_text('RUN make -j"$(nproc)"\n')
        compose = root / "deploy/compose.yaml"
        compose.write_text("services:\n  server:\n    image: ghcr.io/mserajnik/vmangos-server:5875\n")
        bin_dir = root / "mockbin"
        bin_dir.mkdir()
        docker = bin_dir / "docker"
        docker.write_text("""#!/usr/bin/env bash
set -euo pipefail
if [[ "$1 $2" == "image inspect" ]]; then exit 1; fi
[[ "$1" == build ]]
context="${@: -1}"
[[ "$context" == "$MOCK_ROOT"/.setup/server-build-* ]]
cmp "$MOCK_ROOT/setup/vmangos/skatecraft.patch" "$context/docker/patches/zz-skatecraft.patch"
cmp "$MOCK_ROOT/setup/vmangos/tarnished.patch" "$context/docker/patches/zzz-tarnished.patch"
grep -q 'make -j2' "$context/Dockerfile.skatecraft"
printf '%s\\n' "$*" >> "$MOCK_ROOT/build.log"
""")
        docker.chmod(0o755)
        env = dict(os.environ, PATH=f"{bin_dir}:{os.environ['PATH']}",
                   MOCK_ROOT=str(root), SKATECRAFT_BUILD_JOBS="2")
        hashes = []
        for name in (None, "skatecraft.patch", "tarnished.patch"):
            if name:
                with (patches / name).open("a") as stream:
                    stream.write("changed\n")
            subprocess.run(["bash", str(root / "setup/build-server.sh"), str(root / "deploy")],
                           env=env, check=True, stdout=subprocess.DEVNULL)
            digest = hashlib.sha256(
                b"0e3ff01e76d4758e8a7c3108b2717cc785ed56fa\n"
                + hashlib.sha256((patches / "skatecraft.patch").read_bytes()).hexdigest().encode()
                + b"\n"
                + hashlib.sha256((patches / "tarnished.patch").read_bytes()).hexdigest().encode()
                + b"\n"
            ).hexdigest()[:12]
            hashes.append(digest)
            self.assertIn(f"5875-{digest}", compose.read_text())
            self.assertFalse(list((root / ".setup").glob("server-build-*")))
        self.assertEqual(len(set(hashes)), 3)
        env["SKATECRAFT_BUILD_JOBS"] = "0"
        result = subprocess.run(["bash", str(root / "setup/build-server.sh"), str(root / "deploy")],
                                env=env, stdout=subprocess.PIPE, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len((root / "build.log").read_text().splitlines()), 3)

    def test_actual_cpp_melee_continuation_rolls_once(self):
        cpp = added_file("src/game/Objects/Unit.cpp")
        header = added_file("src/game/Tarnished.h")
        code = r"""
#include <algorithm>
#include <limits>
#include <cassert>
#include <cstdint>
#include <functional>
using uint8 = uint8_t; using uint32 = uint32_t; using uint64 = uint64_t; using int32 = int32_t;
class Unit; class Player;
uint32 GetFirstSchoolInMask(uint32 mask) { return mask; }
enum MeleeHitOutcome { MELEE_HIT_NORMAL, MELEE_HIT_CRIT, MELEE_HIT_CRUSHING,
    MELEE_HIT_GLANCING, MELEE_HIT_MISS, MELEE_HIT_DODGE, MELEE_HIT_PARRY, MELEE_HIT_BLOCK };
constexpr uint32 HITINFO_CRITICALHIT = 1, HITINFO_GLANCING = 2, HITINFO_CRUSHING = 4;
constexpr uint32 HITINFO_MISS = 8, HITINFO_BLOCK = 16;
constexpr uint32 PROC_EX_NORMAL_HIT = 1, PROC_EX_CRITICAL_HIT = 2, PROC_EX_BLOCK = 4;
constexpr uint32 SPELL_AURA_MOD_CRIT_PERCENT_VERSUS = 0;
namespace Tarnished {
"""
        code += definition(header, "struct Answer", True) + "\n"
        code += r"""
static bool delay = true;
static uint32 amount = 0;
static std::function<void(Unit*, Player*, Answer const*)> pending;
static bool Defer(Unit*, Player*, uint32 value, uint32, bool,
                  std::function<void(Unit*, Player*, Answer const*)> callback, uint32, uint8 visual, uint32) {
    assert(visual == 0x14);
    amount = value; pending = callback; return delay;
}
}
struct CalcDamageInfo {
    Unit* attacker = nullptr; Unit* target = nullptr;
    uint32 totalDamage = 0, HitInfo = 0, procEx = 0, cleanDamage = 0;
    MeleeHitOutcome hitOutCome = MELEE_HIT_NORMAL;
    struct { uint32 damage = 0; } subDamage[2];
};
class Unit {
public:
    unsigned rolls = 0, applications = 0;
    uint8 components = 1;
    uint8 GetWeaponDamageCount(uint32) { return components; }
    uint32 GetMeleeDamageSchoolMask() { return 0; }
    MeleeHitOutcome next = MELEE_HIT_NORMAL;
    CalcDamageInfo last;
    bool IsAlive() const { return true; }
    void* GetMap() const { return nullptr; }
    Player* ToPlayer();
    uint32 GetCreatureTypeMask() const { return 1; }
    uint32 GetShieldBlockValue() const { return 20; }
    int32 GetTotalAuraModifierByMiscMask(uint32, uint32) const { return 0; }
    float GetGlancingBlowDamageMultiplier(Unit*, uint32) const { return .7f; }
    void RollMeleeOutcomeAgainst(MeleeHitOutcome& out, uint32& flags, Unit*, uint32) {
        ++rolls; out = next;
        if (out == MELEE_HIT_MISS) flags |= HITINFO_MISS;
        if (out == MELEE_HIT_BLOCK) flags |= HITINFO_BLOCK;
        if (out == MELEE_HIT_CRIT) flags |= HITINFO_CRITICALHIT;
        if (out == MELEE_HIT_CRUSHING) flags |= HITINFO_CRUSHING;
        if (out == MELEE_HIT_GLANCING) flags |= HITINFO_GLANCING;
    }
    void FinishMeleeCalculation(CalcDamageInfo* hit, Tarnished::Answer const* answer, bool, bool) {
        assert(answer); // delayed paths never re-enter the stock roller
        hit->hitOutCome = MELEE_HIT_NORMAL; hit->procEx = PROC_EX_NORMAL_HIT;
        if (answer->outcome <= 2 || answer->outcome == 5) {
            hit->totalDamage = 0;
            hit->hitOutCome = answer->outcome == 5 ? MELEE_HIT_MISS :
                answer->outcome <= 1 ? MELEE_HIT_DODGE : MELEE_HIT_PARRY;
        }
        if (answer->outcome == 3) {
            hit->totalDamage -= answer->absorbed;
            hit->hitOutCome = MELEE_HIT_BLOCK; hit->procEx |= PROC_EX_BLOCK;
        }
    }
    void ApplyMeleeDamage(CalcDamageInfo& hit) { ++applications; last = hit; }
    void FinishTarnishedMelee(CalcDamageInfo*, Tarnished::Answer const*, bool);
    void swing(Unit* pVictim);
};
class Player : public Unit {};
Player* Unit::ToPlayer() { return static_cast<Player*>(this); }
"""
        code += definition(cpp, "void Unit::FinishTarnishedMelee") + "\n"
        code += """
void Unit::swing(Unit* pVictim) {
    CalcDamageInfo damageInfo; damageInfo.totalDamage = 100;
    damageInfo.attacker = this; damageInfo.target = pVictim;
    damageInfo.subDamage[0].damage = components == 1 ? 100 : 70;
    damageInfo.subDamage[1].damage = components == 1 ? 0 : 30;
    uint32 attType = 0; bool deferred = true;
"""
        code += definition(cpp, "if (deferred)") + "\n}\n"
        code += r"""
int main() {
    Player defender; Unit source; source.next = MELEE_HIT_CRIT;
    source.swing(&defender); assert(source.rolls == 1 && source.applications == 0);
    assert(Tarnished::amount == 200);
    Tarnished::Answer block; block.outcome = 3; block.absorbed = 40; block.taken = 160;
    Tarnished::pending(&source, &defender, &block);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 160);
    assert(source.last.procEx == (PROC_EX_CRITICAL_HIT | PROC_EX_BLOCK));
    assert(source.last.HitInfo & HITINFO_BLOCK);
    source = Unit{}; source.next = MELEE_HIT_CRIT; source.swing(&defender);
    Tarnished::pending(&source, &defender, nullptr);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 200);
    source = Unit{}; source.next = MELEE_HIT_DODGE; source.swing(&defender);
    Tarnished::pending(&source, &defender, nullptr);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 0);
    source = Unit{}; source.next = MELEE_HIT_MISS; source.swing(&defender);
    Tarnished::Answer accepted; accepted.taken = 100;
    Tarnished::pending(&source, &defender, &accepted);
    assert(source.last.totalDamage == 100 && !(source.last.HitInfo & HITINFO_MISS));
    source = Unit{}; source.next = MELEE_HIT_BLOCK; source.swing(&defender);
    Tarnished::pending(&source, &defender, &accepted);
    assert(source.last.totalDamage == 100 && !(source.last.HitInfo & HITINFO_BLOCK));
    source = Unit{}; source.swing(&defender);
    Tarnished::Answer iframe; iframe.outcome = 0;
    Tarnished::pending(&source, &defender, &iframe);
    assert(!source.last.totalDamage && source.last.hitOutCome == MELEE_HIT_DODGE);
    source = Unit{}; source.next = MELEE_HIT_MISS; source.swing(&defender);
    Tarnished::pending(&source, &defender, nullptr);
    assert(!source.last.totalDamage && (source.last.HitInfo & HITINFO_MISS));
    source = Unit{}; source.next = MELEE_HIT_CRUSHING; source.swing(&defender);
    assert(Tarnished::amount == 150);
    Tarnished::Answer hit; hit.taken = 150;
    Tarnished::pending(&source, &defender, &hit);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 150);
    source = Unit{}; source.components = 2; source.next = MELEE_HIT_CRIT;
    source.swing(&defender);
    hit.taken = 200;
    Tarnished::pending(&source, &defender, &hit);
    assert(source.last.totalDamage == 200);
    assert(source.last.subDamage[0].damage == 140 && source.last.subDamage[1].damage == 60);
    Tarnished::delay = false; source = Unit{}; source.next = MELEE_HIT_CRIT;
    source.swing(&defender);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 200);
}
"""
        source = self.scratch / "melee.cpp"
        binary = self.scratch / "melee"
        source.write_text(code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror",
                        str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_actual_guard_covers_all_components_without_a_shield(self):
        cpp = added_file("src/game/Objects/Unit.cpp")
        code = r"""
#include <algorithm>
#include <limits>
#include <cassert>
#include <cstdint>
using uint8 = uint8_t; using uint32 = uint32_t;
constexpr uint32 VICTIMSTATE_BLOCKS=2, PROC_EX_NORMAL_HIT=1;
struct Answer { uint32 absorbed; };
struct Damage {
    uint32 totalDamage=100, blocked_amount=0, cleanDamage=0, TargetState=0, procEx=0;
    uint32 attackType=0;
    struct { uint32 damage; } subDamage[2] = {{70}, {30}};
};
void apply(Damage* damageInfo, Answer const* answer) {
    uint8 m_weaponDamageCount[1]={2};
    do {
"""
        code += definition(cpp, "if (answer)\n            {\n                // Held ER guard")
        code += r"""
    } while (false);
}
int main() {
    Damage hit; Answer block{100}; apply(&hit, &block);
    assert(!hit.totalDamage && !hit.subDamage[0].damage && !hit.subDamage[1].damage);
    assert(hit.TargetState==VICTIMSTATE_BLOCKS && hit.blocked_amount==100);
    hit=Damage{}; block.absorbed=80; apply(&hit, &block);
    assert(hit.totalDamage==20 && !hit.subDamage[0].damage && hit.subDamage[1].damage==20);
    assert(hit.procEx==PROC_EX_NORMAL_HIT && hit.cleanDamage==80);
}
"""
        source = self.scratch / "guard.cpp"
        binary = self.scratch / "guard"
        source.write_text(code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror",
                        str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_movement_keeps_tarnished_casts_and_channels_but_still_removes_food(self):
        section = PATCH.read_text().split("diff --git a/src/game/Objects/Unit.cpp", 1)[1]
        section = section.split("\ndiff --git ", 1)[0]
        # Retain unchanged context as well as additions, so this executes the real movement gate.
        cpp = "\n".join(line[1:] for line in section.splitlines()
                        if line.startswith(("+", " ")) and not line.startswith("+++"))
        start = cpp.index("void Unit::HandleInterruptsOnMovement")
        end = cpp.index("// Fix bug after 1.11", start)
        body = cpp[start:end] + "}\n"
        code = r"""
#include <cassert>
#include <cstdint>
using uint32 = uint32_t;
constexpr int CURRENT_CHANNELED_SPELL=1, SPELL_INTERRUPT_FLAG_MOVEMENT=1;
constexpr int AURA_INTERRUPT_MOVING_CANCELS=8, AURA_INTERRUPT_TURNING_CANCELS=16;
struct SpellInfo { uint32 Id=5143; };
struct Spell { SpellInfo info; SpellInfo* m_spellInfo=&info; };
struct Unit {
    bool active=false; Spell* channel=nullptr;
    int casts=0, channels=0, auraRemovals=0; uint32 exempt=0;
    Spell* GetCurrentSpell(int) { return channel; }
    void InterruptSpellsWithInterruptFlags(int) { ++casts; }
    void InterruptSpellsWithChannelFlags(int) { ++channels; }
    void RemoveAurasWithInterruptFlags(int, uint32 except) { ++auraRemovals; exempt=except; }
    void HandleEmoteState(int) {}
    void HandleInterruptsOnMovement(bool);
};
namespace Tarnished { bool Active(Unit* unit) { return unit->active; } }
"""
        code += body
        code += r"""
int main() {
    Unit normal; normal.HandleInterruptsOnMovement(true);
    assert(normal.casts==1 && normal.channels==1 && normal.auraRemovals==1 && !normal.exempt);
    Unit stance; Spell channel; stance.active=true; stance.channel=&channel;
    stance.HandleInterruptsOnMovement(true);
    assert(!stance.casts && !stance.channels && stance.auraRemovals==1 && stance.exempt==5143);
    stance.HandleInterruptsOnMovement(false);
    assert(stance.auraRemovals==2 && stance.exempt==5143);
    stance.active=false; stance.HandleInterruptsOnMovement(true);
    assert(stance.casts==1 && stance.channels==1 && !stance.exempt);
}
"""
        source = self.scratch / "moving-cast.cpp"
        binary = self.scratch / "moving-cast"
        source.write_text(code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror",
                        str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_actual_cpp_spell_continuation_clears_missed_damage(self):
        cpp = added_file("src/game/Spells/Spell.cpp")
        header = added_file("src/game/Tarnished.h")
        code = r"""
#include <cassert>
#include <cstdint>
using uint8 = uint8_t; using uint32 = uint32_t;
enum SpellMissInfo { SPELL_MISS_NONE, SPELL_MISS_MISS, SPELL_MISS_RESIST,
    SPELL_MISS_DODGE, SPELL_MISS_PARRY };
struct Unit {};
struct Player : Unit { bool alive = true; bool IsAlive() const { return alive; } };
namespace Tarnished {
"""
        code += definition(header, "struct Answer", True) + "\n}\n"
        code += r"""
struct TargetInfo {
    bool tarnishedStock = false;
    SpellMissInfo missCondition = SPELL_MISS_NONE, tarnishedMiss = SPELL_MISS_NONE;
    uint32 damage = 100, tarnishedUnresisted = 120, tarnishedBlock = 0;
};
class Spell {
public:
    unsigned holds = 1, applications = 0;
    TargetInfo last;
    void SetExecutedCurrently(bool on) { if (on) ++holds; else { assert(holds); --holds; } }
    void DoAllEffectOnTarget(TargetInfo* target) { ++applications; last = *target; }
    void resume(TargetInfo snapshot, Unit* source, Player* defender,
                Tarnished::Answer const* answer) {
        auto resume = snapshot;
"""
        code += definition(cpp, "auto finish = [this, resume]") + ";\n"
        code += "finish(source, defender, answer);\n}\n};\n"
        code += r"""
int main() {
    Unit source; Player defender; TargetInfo target;
    Spell spell; Tarnished::Answer iframe; iframe.outcome = 0;
    spell.resume(target, &source, &defender, &iframe);
    assert(!spell.holds && spell.applications == 1);
    assert(spell.last.missCondition == SPELL_MISS_DODGE && !spell.last.damage);
    assert(!spell.last.tarnishedBlock && !spell.last.tarnishedStock);
    for (auto miss : {SPELL_MISS_MISS, SPELL_MISS_RESIST, SPELL_MISS_DODGE, SPELL_MISS_PARRY}) {
        spell = Spell{}; target.tarnishedMiss = miss;
        spell.resume(target, &source, &defender, nullptr);
        assert(!spell.holds && spell.applications == 1 && spell.last.tarnishedStock);
        assert(spell.last.missCondition == miss && !spell.last.damage);
    }
    spell = Spell{}; target.tarnishedMiss = SPELL_MISS_NONE;
    spell.resume(target, &source, &defender, nullptr);
    assert(spell.last.damage == 120 && spell.last.tarnishedStock && !spell.holds);
    spell = Spell{}; Tarnished::Answer block;
    block.outcome = 3; block.taken = 60; block.absorbed = 40;
    spell.resume(target, &source, &defender, &block);
    assert(spell.last.damage == 60 && spell.last.tarnishedBlock == 40 && !spell.holds);
    spell = Spell{}; block.taken = 0; block.absorbed = 100;
    spell.resume(target, &source, &defender, &block);
    assert(!spell.last.damage && spell.last.tarnishedBlock == 100 && !spell.holds);
    spell = Spell{}; spell.resume(target, nullptr, &defender, nullptr);
    assert(!spell.holds && !spell.applications);
    spell = Spell{}; defender.alive = false;
    spell.resume(target, &source, &defender, nullptr);
    assert(!spell.holds && !spell.applications);
}
"""
        code = "#include <initializer_list>\n" + code
        source = self.scratch / "spell.cpp"
        binary = self.scratch / "spell"
        source.write_text(code)
        subprocess.run(["g++", "-std=c++14", "-Wall", "-Wextra", "-Werror",
                        str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)

    def test_patch_applies_to_skatecraft_patched_pinned_source(self):
        source = os.environ.get("TARNISHED_VMANGOS_SOURCE")
        if not source:
            self.skipTest("set TARNISHED_VMANGOS_SOURCE to the pinned, Skatecraft-patched source")
        directory = Path(source).resolve().relative_to(ROOT)
        prepared = self.scratch / "tarnished.patch"
        prepared.write_bytes(prepare(PATCH.read_bytes()))
        subprocess.run(["git", "apply", "--check", f"--directory={directory}", str(prepared)],
                       cwd=ROOT, check=True)

    def test_modified_cpp_translation_units_compile(self):
        build = os.environ.get("TARNISHED_VMANGOS_BUILD")
        if not build:
            self.skipTest("set TARNISHED_VMANGOS_BUILD to a configured, patched pinned-source build")
        directory = ROOT / Path(build).resolve().relative_to(ROOT)
        entries = json.loads((directory / "compile_commands.json").read_text())
        names = {"Tarnished.cpp", "Skatecraft.cpp", "SpellEffects.cpp", "Unit.cpp", "Spell.cpp", "SpellCaster.cpp", "WorldSession.cpp",
                 "Opcodes.cpp", "Player.cpp", "CharacterHandler.cpp", "ChatHandler.cpp", "Object.cpp"}
        checked = set()
        for entry in entries:
            source = Path(entry["file"])
            if source.name not in names or "/src/game/" not in str(source):
                continue
            source.resolve().relative_to(ROOT)
            args = shlex.split(entry["command"])
            output = args.index("-o")
            del args[output:output + 2]
            args.remove("-c")
            args.insert(1, "-fsyntax-only")
            subprocess.run(args, cwd=entry["directory"], check=True)
            checked.add(source.name)
        self.assertEqual(checked, names)

    def test_patch_hunk_lengths(self):
        unit = prepare(PATCH.read_bytes()).split(
            b"diff --git a/src/game/Objects/Unit.cpp b/src/game/Objects/Unit.cpp\n", 1
        )[1].split(b"diff --git ", 1)[0]
        in_hunk = False
        for line in unit.splitlines(keepends=True):
            if line.startswith(b"@@"):
                in_hunk = True
            elif in_hunk and line[:1] in (b" ", b"+", b"-"):
                self.assertTrue(line.endswith(b"\r\n"), "pinned Unit.cpp has CRLF context")
        self.assertEqual(prepare(prepare(PATCH.read_bytes())), prepare(PATCH.read_bytes()))
        lines = PATCH.read_text().splitlines()
        for index, line in enumerate(lines):
            if not line.startswith("@@"):
                continue
            match = re.match(r"@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@", line)
            self.assertIsNotNone(match)
            end = index + 1
            while end < len(lines) and not lines[end].startswith(("@@", "diff --git ")):
                end += 1
            hunk = lines[index + 1:end]
            self.assertEqual(sum(s.startswith((" ", "-")) for s in hunk),
                             int(match[1] or 1), line)
            self.assertEqual(sum(s.startswith((" ", "+")) for s in hunk),
                             int(match[2] or 1), line)


if __name__ == "__main__":
    unittest.main()
