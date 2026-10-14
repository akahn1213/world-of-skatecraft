// SPDX-License-Identifier: GPL-2.0-or-later
// Exercises the linked native core and bot module with synthetic database records.
#include "botpch.h"
#include "playerbot/WorldPosition.h"
#include "playerbot/RandomPlayerbotMgr.h"
#include "playerbot/PlayerbotAIConfig.h"
#include "ahbot/AhBotConfig.h"
#include "ahbot/AhBot.h"
#include "shared/Config/Config.h"
#include <fstream>
#include <limits>
#include <cassert>
#include <iostream>
DatabaseType WorldDatabase, CharacterDatabase, LoginDatabase, LogsDatabase;
uint32 realmID = 1;
std::string realmName = "fixture";
char const* g_mainLogFileName = "fixture.log";
int main(int argc,char** argv) {
  assert(argc==5);
  std::ofstream("mangosd.conf") << "[Mangosd]\nMaxPlayerLevel = 2\n";
  assert(sConfig.LoadFromFile("mangosd.conf"));
  AhBotConfig missing;
  assert(!missing.enabled && missing.updateInterval == 0);
  missing.enabled = true;
  assert(!missing.Initialize() && !missing.enabled);
  std::ofstream("playerbots-ahbot.conf") << "AhBot.Enabled = 1\nAhBot.UpdateIntervalInSeconds = 900\n";
  assert(missing.Initialize() && missing.enabled && missing.updateInterval == 900);
  std::remove("playerbots-ahbot.conf");
  assert(!missing.Initialize() && !missing.enabled);
  assert(WorldDatabase.Initialize((std::string(".;0;")+argv[3]+";fixture;"+argv[1]).c_str()));
  assert(CharacterDatabase.Initialize((std::string(".;0;")+argv[3]+";fixture;"+argv[2]).c_str()));
  if (std::string(argv[4]) == "cold") {
  WorldDatabase.DirectExecute("CREATE TABLE area_template (entry INT, map_id INT, zone_id INT, flag INT, flags INT, level INT, name TEXT, team INT, liquid INT)");
  WorldDatabase.DirectExecute("INSERT INTO area_template VALUES (1,0,0,10,0,1,'Test Zone',0,0),(2,0,1,11,0,1,'Test Area',0,0)");
  WorldDatabase.DirectExecute("CREATE TABLE map_template (entry INT,parent INT,type INT,zone INT,players INT,reset INT,ghost INT,x FLOAT,y FLOAT,name TEXT,script TEXT)");
  WorldDatabase.DirectExecute("INSERT INTO map_template VALUES (0,0,0,2,0,0,0,0,0,'Test Continent',''),(33,0,1,2,5,0,0,0,0,'Test Dungeon','')");
  WorldDatabase.DirectExecute("CREATE TABLE ai_playerbot_rpg_races (entry INT,race INT,minl INT,maxl INT)");
  WorldDatabase.DirectExecute("INSERT INTO ai_playerbot_rpg_races VALUES (100,0,1,2)");

  }
  sAreaStorage.Load();sMapStorage.Load();
  assert(ai::WorldPosition(0,1,2,3).getAreaName(true,true)=="Test Zone");
  assert(ai::WorldPosition(0,1,2,3).getAreaName(true,false)=="Test Zone Test Area");
  assert(ai::WorldPosition(33,1,2,3).getAreaName()=="Test Dungeon");
  sWorld.setConfig(CONFIG_UINT32_MAX_PLAYER_LEVEL,2u);
  sPlayerbotAIConfig.enabled=false;sPlayerbotAIConfig.randomBotMaxLevel=2;sPlayerbotAIConfig.randomBotMaps={0};sPlayerbotAIConfig.randomBotTeleLevel=2;
  auto npc=std::make_unique<CreatureInfo>();npc->entry=100;npc->npc_flags=UNIT_NPC_FLAG_VENDOR;
  sObjectMgr.m_creatureInfoMap[100]=std::move(npc);
  auto mob=std::make_unique<CreatureInfo>();mob->entry=101;mob->type=CREATURE_TYPE_HUMANOID;mob->loot_id=1;mob->level_min=mob->level_max=1;
  sObjectMgr.m_creatureInfoMap[101]=std::move(mob);
  auto& spawn=sObjectMgr.NewOrExistCreatureData(7);spawn.creature_id[0]=100;spawn.position=WorldLocation(0,1,2,3);
  auto& beast=sObjectMgr.NewOrExistCreatureData(8);beast.creature_id[0]=101;beast.position=WorldLocation(0,2,3,4);
  // Spawn workers must return addresses owned by ObjectMgr, not temporary pairs.
  auto creatures = ai::WorldPosition().getCreaturesNear();
  assert(creatures.size() == 2);
  for (auto* record : creatures) assert(&record->second == sObjectMgr.GetCreatureData(record->first));
  RandomPlayerbotMgr manager;
  for(int run=0;run<2;++run) {
    std::cout<<"cache pass "<<run<<std::endl;
    manager.PrepareTeleportCache();
    assert(manager.locsPerLevelCache.at(1).size()==1);
    assert(manager.rpgLocsCacheLevel.at(1).at(1).size()==2);
  }
  CharacterDatabase.DirectExecute("INSERT INTO ai_playerbot_tele_cache (map_id,x,y,z,level) VALUES (99999,1,2,3,1),(0,1,2,3,257),(0,999999,2,3,1)");
  manager.PrepareTeleportCache();
  assert(manager.locsPerLevelCache.at(1).size()==1);
  assert(manager.locsPerLevelCache.size()==2);
  // A mismatched index must be rejected, not read through unchecked operator[].
  std::map<uint32, std::map<uint32, std::vector<uint64>>> incomplete;
  incomplete[1][1] = {};
  assert(manager.RpgLocationsNear(WorldLocation(0,1,2,3), incomplete).empty());
  assert(ai::WorldPosition(99999,1,2,3).getAreaName().empty());
  assert(ai::WorldPosition(0,std::numeric_limits<float>::quiet_NaN(),2,3).getAreaFlag()==0);
  // A cycle in zone parents must terminate without reading an invalid string.
  WorldDatabase.DirectExecute("UPDATE area_template SET zone_id=2 WHERE entry=1");
  sAreaStorage.Load();
  assert(!ai::WorldPosition(0,1,2,3).getAreaName().empty());
  WorldDatabase.DirectExecute("UPDATE area_template SET zone_id=0 WHERE entry=1");
  sAreaStorage.Load();
  CharacterDatabase.DirectExecute("INSERT INTO mail (id,receiver_guid,subject,deliver_time) VALUES (100,42,'AH Proposition: old',1),(101,43,'Ordinary mail',1),(102,44,'AH Proposition: recent',UNIX_TIMESTAMP())");
  auctionbot.CleanupPropositions();
  auto mails=CharacterDatabase.Query("SELECT id,receiver_guid FROM mail ORDER BY id");
  assert(mails && mails->GetRowCount()==2);
  assert(mails->Fetch()[0].GetUInt32()==101 && mails->Fetch()[1].GetUInt32()==43);
  assert(mails->NextRow() && mails->Fetch()[0].GetUInt32()==102);
  CharacterDatabase.DirectExecute("DELETE FROM mail");
  CharacterDatabase.HaltDelayThread();WorldDatabase.HaltDelayThread();
  std::cout<<"Area names and cold/warm teleport caches passed\n";
}
