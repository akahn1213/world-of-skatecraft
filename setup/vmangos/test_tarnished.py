#!/usr/bin/env python3
"""Standalone checks of the patch's actual C++ validators and the image build wrapper."""
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import unittest

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

    def test_actual_cpp_pose_and_defence_validation(self):
        cpp = added_file("src/game/Tarnished.cpp")
        header = added_file("src/game/Tarnished.h")
        code = r"""
#include <algorithm>
#include <cassert>
#include <cmath>
#include <cstring>
#include <deque>
#include <functional>
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
        for name in ("Newer", "Float", "Transform", "Pose", "Budget", "Defence"):
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
#include <cassert>
#include <cstdint>
#include <functional>
using uint8 = uint8_t; using uint32 = uint32_t; using int32 = int32_t;
class Unit; class Player;
enum MeleeHitOutcome { MELEE_HIT_NORMAL, MELEE_HIT_CRIT, MELEE_HIT_CRUSHING,
    MELEE_HIT_GLANCING, MELEE_HIT_MISS, MELEE_HIT_DODGE, MELEE_HIT_PARRY, MELEE_HIT_BLOCK };
constexpr uint32 HITINFO_CRITICALHIT = 1, HITINFO_GLANCING = 2, HITINFO_CRUSHING = 4;
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
                  std::function<void(Unit*, Player*, Answer const*)> callback) {
    amount = value; pending = callback; return delay;
}
}
struct CalcDamageInfo {
    Unit* attacker = nullptr; Unit* target = nullptr;
    uint32 totalDamage = 0, HitInfo = 0, procEx = 0, cleanDamage = 0;
    MeleeHitOutcome hitOutCome = MELEE_HIT_NORMAL;
    struct { uint32 damage = 0; } subDamage[1];
};
class Unit {
public:
    unsigned rolls = 0, applications = 0;
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
        if (out == MELEE_HIT_CRIT) flags |= HITINFO_CRITICALHIT;
        if (out == MELEE_HIT_CRUSHING) flags |= HITINFO_CRUSHING;
        if (out == MELEE_HIT_GLANCING) flags |= HITINFO_GLANCING;
    }
    void FinishMeleeCalculation(CalcDamageInfo* hit, Tarnished::Answer const* answer, bool, bool) {
        assert(answer); // delayed paths never re-enter the stock roller
        hit->hitOutCome = MELEE_HIT_NORMAL; hit->procEx = PROC_EX_NORMAL_HIT;
        if (answer->outcome <= 2) {
            hit->totalDamage = 0;
            hit->hitOutCome = answer->outcome == 0 ? MELEE_HIT_MISS :
                answer->outcome == 1 ? MELEE_HIT_DODGE : MELEE_HIT_PARRY;
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
    source = Unit{}; source.next = MELEE_HIT_CRIT; source.swing(&defender);
    Tarnished::pending(&source, &defender, nullptr);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 200);
    source = Unit{}; source.next = MELEE_HIT_DODGE; source.swing(&defender);
    Tarnished::pending(&source, &defender, nullptr);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 0);
    source = Unit{}; source.next = MELEE_HIT_CRUSHING; source.swing(&defender);
    assert(Tarnished::amount == 150);
    Tarnished::Answer hit; hit.taken = 150;
    Tarnished::pending(&source, &defender, &hit);
    assert(source.rolls == 1 && source.applications == 1 && source.last.totalDamage == 150);
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

    def test_patch_applies_to_skatecraft_patched_pinned_source(self):
        source = os.environ.get("TARNISHED_VMANGOS_SOURCE")
        if not source:
            self.skipTest("set TARNISHED_VMANGOS_SOURCE to the pinned, Skatecraft-patched source")
        directory = Path(source).resolve().relative_to(ROOT)
        subprocess.run(["git", "apply", "--check", f"--directory={directory}", str(PATCH)],
                       cwd=ROOT, check=True)

    def test_patch_hunk_lengths(self):
        unit = PATCH.read_bytes().split(
            b"diff --git a/src/game/Objects/Unit.cpp b/src/game/Objects/Unit.cpp\n", 1
        )[1].split(b"diff --git ", 1)[0]
        in_hunk = False
        for line in unit.splitlines(keepends=True):
            if line.startswith(b"@@"):
                in_hunk = True
            elif in_hunk and line[:1] in (b" ", b"+", b"-"):
                self.assertTrue(line.endswith(b"\r\n"), "pinned Unit.cpp has CRLF context")
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
