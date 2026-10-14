// SPDX-License-Identifier: GPL-2.0-or-later
// Exercises the linked native core and bot module with synthetic database records.
#include "botpch.h"
#include "playerbot/WorldPosition.h"
#include "playerbot/RandomPlayerbotMgr.h"
#include "playerbot/PlayerbotAIConfig.h"
#include "playerbot/PlayerbotDbStore.h"
#include "playerbot/PlayerbotTextMgr.h"
#include "game/Chat/Channel.h"
#include "ahbot/AhBotConfig.h"
#include "ahbot/AhBot.h"
#include "shared/Config/Config.h"
#include <fstream>
#include <limits>
#include <cassert>
#include <iostream>
#include <type_traits>
DatabaseType WorldDatabase, CharacterDatabase, LoginDatabase, LogsDatabase;
uint32 realmID = 1;
std::string realmName = "fixture";
char const* g_mainLogFileName = "fixture.log";

static void CheckNativeContracts()
{
  static_assert(sizeof(uint8) == 1 && sizeof(uint16) == 2 && sizeof(uint32) == 4 && sizeof(uint64) == 8);
  static_assert(sizeof(dtPolyRef) == 8 && sizeof(dtTileRef) == 8, "Use the pinned native Detour ABI");
  static_assert(std::is_same_v<decltype(std::declval<Field>().GetCppString()), std::string>);
  auto rows = CharacterDatabase.Query("SELECT NULL, '', 'first', 255, 65535, 4294967295, 18446744073709551615, -2147483648, -9223372036854775808 UNION ALL SELECT NULL, '', 'second', 0, 0, 0, 0, 0, 0");
  assert(rows && rows->GetRowCount() == 2);
  Field* fields = rows->Fetch();
  assert(fields[0].IsNULL() && fields[0].GetString() == nullptr);
  assert(fields[0].GetCppString().empty() && fields[0].GetUInt64() == 0 && !fields[0].GetBool());
  assert(!fields[1].IsNULL() && fields[1].GetCppString().empty());
  assert(fields[3].GetUInt8() == UINT8_MAX && fields[4].GetUInt16() == UINT16_MAX);
  assert(fields[5].GetUInt32() == UINT32_MAX && fields[6].GetUInt64() == UINT64_MAX);
  assert(fields[7].GetInt32() == INT32_MIN && fields[8].GetInt64() == INT64_MIN);
  std::string owned = fields[2].GetCppString();
  assert(rows->NextRow() && rows->Fetch()[2].GetCppString() == "second");
  rows.reset();
  assert(owned == "first"); // No MYSQL_ROW pointers survive row/result destruction.

  ObjectGuid guid(HIGHGUID_UNIT, uint32(12345), uint32(0x00abcdef));
  WorldPacket attack(CMSG_ATTACKSWING);
  attack << guid;
  assert(attack.size() == sizeof(uint64));
  WorldPackets::Combat::AttackSwing decoded;
  attack.rpos(attack.size()); // Adapter must reset the reader before native decoding.
  assert(DecodeBotPacket(decoded, attack) && decoded.targetGuid == guid);
  WorldPacket copy = attack;
  attack.clear();
  assert(DecodeBotPacket(decoded, copy) && decoded.targetGuid.GetRawValue() == guid.GetRawValue());
  WorldPacket shortPacket(CMSG_ATTACKSWING);
  shortPacket << uint32(123);
  assert(!DecodeBotPacket(decoded, shortPacket));
  WorldPacket cancel(CMSG_CANCEL_CAST);
  cancel << uint32(UINT32_MAX);
  WorldPackets::Spell::CancelCast cancelDecoded;
  assert(DecodeBotPacket(cancelDecoded, cancel) && cancelDecoded.spellId == UINT32_MAX);
  WorldPacket open(CMSG_OPEN_ITEM);
  open << uint8(255) << uint8(254);
  WorldPackets::Spell::OpenItem openDecoded;
  assert(DecodeBotPacket(openDecoded, open) && openDecoded.bagIndex == 255 && openDecoded.slot == 254);

  Channel channel("Compatibility");
  ObjectGuid player(HIGHGUID_PLAYER, uint32(42)), other(HIGHGUID_PLAYER, uint32(43));
  assert(!channel.HasMember(player));
  channel.m_players[player] = Channel::PlayerInfo{player, 0};
  assert(channel.HasMember(player) && !channel.HasMember(other) && channel.GetNumPlayers() == 1);
  channel.m_players.erase(player);
  assert(!channel.HasMember(player));
  assert(sObjectMgr.GetIndexForLocale(LOCALE_enUS) == -1);
  for (uint32 i = 1; i < MAX_LOCALE; ++i) {
    auto locale = LocaleConstant(i);
    int index = sObjectMgr.GetOrNewIndexForLocale(locale);
    assert(index >= 0 && sObjectMgr.GetIndexForLocale(locale) == index && sObjectMgr.GetLocaleForIndex(index) == locale);
  }
}

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
  CheckNativeContracts();
  if (std::string(argv[4]) == "cold") {
  WorldDatabase.DirectExecute("CREATE TABLE area_template (entry INT, map_id INT, zone_id INT, flag INT, flags INT, level INT, name TEXT, team INT, liquid INT)");
  WorldDatabase.DirectExecute("INSERT INTO area_template VALUES (1,0,0,10,0,1,'Test Zone',0,0),(2,0,1,11,0,1,'Test Area',0,0)");
  WorldDatabase.DirectExecute("CREATE TABLE map_template (entry INT,parent INT,type INT,zone INT,players INT,reset INT,ghost INT,x FLOAT,y FLOAT,name TEXT,script TEXT)");
  WorldDatabase.DirectExecute("INSERT INTO map_template VALUES (0,0,0,2,0,0,0,0,0,'Test Continent',''),(33,0,1,2,5,0,0,0,0,'Test Dungeon','')");
  WorldDatabase.DirectExecute("CREATE TABLE ai_playerbot_rpg_races (entry INT,race INT,minl INT,maxl INT)");
  WorldDatabase.DirectExecute("INSERT INTO ai_playerbot_rpg_races VALUES (100,0,1,2)");
  WorldDatabase.DirectExecute("CREATE TABLE ai_playerbot_texts (name TEXT, text TEXT, say_type INT, reply_type INT, text_loc1 TEXT, text_loc2 TEXT, text_loc3 TEXT, text_loc4 TEXT, text_loc5 TEXT, text_loc6 TEXT, text_loc7 TEXT, text_loc8 TEXT)");
  WorldDatabase.DirectExecute("INSERT INTO ai_playerbot_texts VALUES ('nullable',NULL,0,0,'translated',NULL,NULL,NULL,NULL,NULL,NULL,NULL)");

  }
  sAreaStorage.Load();sMapStorage.Load();
  sPlayerbotTextMgr.LoadBotTexts();
  auto const& text = sPlayerbotTextMgr.botTexts.at("nullable").at(0);
  assert(text.m_text.empty());
  assert(text.m_text_locales.at(sObjectMgr.GetIndexForLocale(LOCALE_koKR)) == "translated");
  assert(text.m_text_locales.at(sObjectMgr.GetIndexForLocale(LOCALE_frFR)).empty());
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
  // Exercise the enabled constructor, including persisted bot-event loading.
  sPlayerbotAIConfig.enabled = true;
  sPlayerbotAIConfig.randomBotAutologin = true;
  sPlayerbotAIConfig.commandServerPort = 0;
  std::cout << "Starting enabled bot manager: " << argv[4] << std::endl;
  RandomPlayerbotMgr manager;
  if (std::string(argv[4]) != "cold") {
    assert(manager.GetEventValue(42, "add") == 1);
    assert(manager.GetEventValue(42, "login") == 0); // Constructor clears stale logins.
    assert(manager.GetEventData(42, "add").empty());
    assert(manager.GetEventValue(42, "boundary") == UINT32_MAX);
  }
  manager.SetEventValue(42, "add", 1, 86400);
  manager.SetEventValue(42, "login", 1, 86400);
  manager.SetEventValue(42, "boundary", UINT32_MAX, 86400);
  std::string const eventName = "quoted'event";
  std::string const payload = "O'Brien\\path\n100% Grüße";
  manager.SetEventValue(42, eventName, 17, 86400, payload);
  manager.eventCache.clear();
  assert(manager.GetEventValue(42, eventName) == 17 && manager.GetEventData(42, eventName) == payload);
  manager.SetEventValue(42, eventName, 0, 0);
  manager.eventCache.clear();
  assert(manager.GetEventValue(42, eventName) == 0);
  sPlayerbotDbStore.SaveValue(UINT32_MAX, "quoted'preset", "quoted'key", payload);
  auto preset = CharacterDatabase.Query("SELECT guid,preset,`key`,value FROM ai_playerbot_db_store WHERE guid=4294967295 ORDER BY id DESC LIMIT 1");
  assert(preset && preset->Fetch()[0].GetUInt64() == UINT32_MAX);
  assert(preset->Fetch()[1].GetCppString() == "quoted'preset" && preset->Fetch()[2].GetCppString() == "quoted'key");
  assert(preset->Fetch()[3].GetCppString() == payload);
  auto eventRow = CharacterDatabase.Query("SELECT data FROM ai_playerbot_random_bots WHERE bot=42 AND event='add'");
  assert(eventRow && eventRow->Fetch()[0].IsNULL());
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
