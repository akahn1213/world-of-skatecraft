#!/usr/bin/env python3
"""Compile the prepared core's actual energy helper/update gate with a small player fixture.
No game assets or DB; this does not replace a full server build or live packet test.
"""
import argparse
from pathlib import Path
import subprocess
import tempfile


def function(source, signature):
    start = source.index(signature)
    brace = source.index('{', start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    args = parser.parse_args()
    source = (args.source / 'src/game/Objects/Player.cpp').read_text()
    helper = source[source.index('// Skatecraft deviation: energy'):source.index('#define ZONE_UPDATE_INTERVAL')]
    regen = function(source, 'void Player::RegenerateAll()')
    start = source.index('    if (IsAlive())\n    {\n        RegenerateEnergy')
    gate = source[start:source.index('    if (m_deathState == JUST_DIED)', start)]
    assert 'ResetEnergyRegen(this);' in function(source, 'Player::~Player()')
    assert 'Regenerate(POWER_ENERGY)' not in regen
    harness = r'''
#include <cassert>
#include <cmath>
#include <cstdint>
#include <mutex>
#include <unordered_map>
#include <vector>
#include <limits>
#include <iostream>
using uint32 = uint32_t; using uint64 = uint64_t; using int32 = int32_t;
enum { POWER_ENERGY, POWER_MANA, POWER_RAGE, CONFIG_FLOAT_RATE_POWER_ENERGY,
 SPELL_AURA_MOD_POWER_REGEN_PERCENT, SPELL_AURA_MOD_REGEN_DURING_COMBAT,
 SPELL_AURA_MOD_HEALTH_REGEN_IN_COMBAT, SPELL_AURA_INTERRUPT_REGEN };
constexpr int REGEN_TIME_PLAYER_FULL = 2000;
struct Modifier { int m_miscvalue, m_amount; };
struct Aura { Modifier mod; Modifier const* GetModifier() const { return &mod; } };
struct World { double rate = 1; double getConfig(int) const { return rate; } } sWorld;
struct Player {
 uint32 energy=0, maximum=100, manaTicks=0, healthTicks=0, rageTicks=0;
 int32 m_regenTimer=2000; bool alive=true, combat=false;
 std::vector<Aura const*> auras;
 uint32 GetMaxPower(int) const { return maximum; }
 uint32 GetPower(int) const { return energy; }
 void SetPower(int, uint32 value) { energy=value; }
 auto const& GetAurasByType(int) const { return auras; }
 bool IsAlive() const { return alive; }
 bool IsInCombat() const { return combat; }
 bool HasAuraType(int) const { return false; }
 bool IsPolymorphed() const { return false; }
 void RegenerateHealth() { ++healthTicks; }
 void Regenerate(int power) { if(power==POWER_MANA) ++manaTicks; else if(power==POWER_RAGE) ++rageTicks; else assert(false); }
 void RegenerateAll(); void Update(uint32 update_diff);
};
'''
    tests = r'''
int main() {
 Player p;
 p.Update(99); assert(p.energy==0 && p.manaTicks==0);
 p.Update(1); assert(p.energy==1 && p.manaTicks==0);
 for(int i=1;i<20;++i) { p.Update(100); assert(p.energy==uint32(i+1)); }
 assert(p.energy==20 && p.manaTicks==1 && p.healthTicks==1 && p.rageTicks==1);
 ResetEnergyRegen(&p); p.energy=0; p.combat=true;
 p.Update(450); assert(p.energy==4); p.Update(50); assert(p.energy==5);
 assert(p.healthTicks==1 && p.rageTicks==1);
 ResetEnergyRegen(&p); p.energy=0; sWorld.rate=0.5;
 for(int i=0;i<40;++i) p.Update(50);
 assert(p.energy==10);
 ResetEnergyRegen(&p); p.energy=0; sWorld.rate=1.33;
 for(int i=0;i<40;++i) p.Update(100);
 assert(p.energy==53);
 ResetEnergyRegen(&p); p.energy=0; sWorld.rate=1;
 Aura rush{{POWER_ENERGY,100}}, unrelated{{POWER_MANA,100}};
 p.auras={&rush,&unrelated}; p.Update(100); assert(p.energy==2);
 p.auras.clear(); p.Update(100); assert(p.energy==3);
 p.energy=99; p.Update(5000); assert(p.energy==100);
 p.Update(5000); p.energy=0; p.Update(100); assert(p.energy==1);
 ResetEnergyRegen(&p); p.energy=0; p.Update(50);
 p.alive=false; p.Update(1000); assert(p.energy==0 && !energyRegenStates.count(&p));
 p.alive=true; p.Update(50); assert(p.energy==0); p.Update(50); assert(p.energy==1);
 assert(p.manaTicks>1); // Existing immediate post-death mana tick preserved.
 p.maximum=0; p.Update(50); assert(!energyRegenStates.count(&p));
 Player other; other.Update(100); assert(other.energy==1); ResetEnergyRegen(&other);
 EnergyRegenState state;
 assert(state.Advance(UINT32_MAX,1,0,100)==100 && state.elapsed==95);
 assert(state.Advance(100,-1,0,100)==0);
 assert(state.Advance(100,std::numeric_limits<double>::infinity(),0,100)==0);
 std::cout << "100ms cadence, 2s other powers, combat, jitter/catch-up, modifiers, fractions, cap, death and isolation passed\n";
}
'''
    with tempfile.TemporaryDirectory(prefix='energy-test-') as temp:
        cpp = Path(temp) / 'energy.cpp'
        cpp.write_text(harness + helper + regen + '\nvoid Player::Update(uint32 update_diff) {\n' + gate + '\n}\n' + tests)
        exe = Path(temp) / 'energy'
        subprocess.run(['c++', '-std=c++14', '-Wall', '-Wextra', '-Werror', '-fsanitize=address,undefined',
                        '-fno-omit-frame-pointer', '-g', str(cpp), '-pthread', '-o', str(exe)], check=True)
        subprocess.run([str(exe)], check=True)


if __name__ == '__main__':
    main()
