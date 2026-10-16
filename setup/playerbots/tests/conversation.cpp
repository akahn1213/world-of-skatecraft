// SPDX-License-Identifier: GPL-2.0-or-later
#include "botpch.h"
#include "playerbot/LLMConversation.h"
#include "playerbot/LLMConversationText.h"
#include "playerbot/PlayerbotAI.h"
#include "playerbot/PlayerbotAIConfig.h"
#include "playerbot/PlayerbotLLMInterface.h"
#include "playerbot/strategy/actions/SayAction.h"
#include "Group.h"
#include <cassert>
#include <fstream>
#include <iostream>
DatabaseType WorldDatabase, CharacterDatabase, LoginDatabase, LogsDatabase;
uint32 realmID = 1;
std::string realmName = "fixture";
char const* g_mainLogFileName = "fixture.log";
using namespace ai::conversation;
int main(int argc, char** argv)
{
    assert(argc == 2);
    assert(Clip("é猫🙂", 4) == "é");
    assert(Clip("é猫🙂", 5) == "é猫");
    assert(Expand("<a> <b>", {{"<a>","<b>"},{"<b>","safe"}}) == "<b> safe");
    Journal journal;
    journal.Add({1,100,"private","Human","secret"},100,3,20);
    journal.Add({2,101,"party","Bot","reply"},101,3,20);
    assert(journal.Render("party",false,102,3,20,1024).find("secret") == std::string::npos);
    assert(journal.Render("party",true,102,3,20,1024).find("secret") != std::string::npos);
    assert(journal.Render("party",true,130,3,20,1024).empty());
    for (int i=0;i<200;++i) journal.Add({1,i,"scope","name",std::to_string(i)},i,2,1000);
    assert(journal.Render("scope",false,199,2,1000,1024).find(": 197\n") == std::string::npos);
    assert(journal.Render("scope",false,199,2,1000,1).empty());
    std::string lines="first\né猫\n"; FitHistory(lines,6); assert(lines=="é猫\n");
    sMapMgr.InitStateMachine();
    WorldSession session(100,nullptr,SEC_PLAYER,0,LOCALE_enUS);
    Player bot(&session); bot.Object::_Create(100,0,HIGHGUID_PLAYER); bot.SetName("Bot");
    bot.m_playerbotAI.reset(new PlayerbotAI());
    bot.GetPlayerbotAI()->bot = &bot;
    BotConfig config;
    Configure(config); assert(Snapshot(&bot).empty()); assert(!HistoryEnabled());
    {
        std::ofstream out("context.conf");
        out << "AiPlayerbot.LLMGameplayContextEnabled = 1\nAiPlayerbot.LLMHistoryEnabled = 1\n"
               "AiPlayerbot.LLMGameplayContextFields = activity, travel,needs,quests,party,unknown\n"
               "AiPlayerbot.LLMGameplayContextMaxBytes = 4096\nAiPlayerbot.LLMHistoryMaxMessages = 16\n";
    }
    assert(config.SetSource("context.conf","CONVERSATION_FIXTURE")); Configure(config);
    assert(Snapshot(nullptr).empty());
    auto snapshot=Snapshot(&bot);
    assert(snapshot.find("travel: unavailable")!=std::string::npos);
    assert(snapshot.find("these needs do not imply a trip")!=std::string::npos);
    assert(snapshot.find("not grouped")!=std::string::npos);
    bot.SetQuestSlot(0,7654321); // Missing template must be harmless.
    assert(Snapshot(&bot).find("no readable active quests")!=std::string::npos);
    auto quest=std::make_unique<Quest>(); quest->Title="Testing é猫";
    quest->ReqCreatureOrGOCount[0]=3; quest->ObjectiveText[0]="Gather clues";
    sObjectMgr.m_QuestTemplatesMap[7654321]=std::move(quest);
    bot.GetQuestStatusMap()[7654321].m_status=QUEST_STATUS_INCOMPLETE;
    bot.GetQuestStatusMap()[7654321].m_creatureOrGOcount[0]=1;
    assert(Snapshot(&bot).find("1/3")!=std::string::npos);
    bot.GetQuestStatusMap()[7654321].m_creatureOrGOcount[0]=2;
    assert(Snapshot(&bot).find("2/3")!=std::string::npos);
    uint64 peer=ObjectGuid(HIGHGUID_PLAYER,uint32(101)).GetRawValue();
    Remember(&bot,CHAT_MSG_WHISPER,peer,"",peer,"Human","Earlier question é猫 \"quoted\" <bot name>");
    Remember(&bot,CHAT_MSG_WHISPER,peer,"",bot.GetObjectGuid().GetRawValue(),"Bot","Delivered answer");
    auto history=History(&bot,CHAT_MSG_WHISPER,peer,"");
    assert(history.find("Earlier question")!=std::string::npos && history.find("Delivered answer")!=std::string::npos);
    assert(History(&bot,CHAT_MSG_WHISPER,peer+1,"").empty());
    assert(History(&bot,CHAT_MSG_CHANNEL,0,"General").empty());
    Remember(&bot,CHAT_MSG_CHANNEL,0,"General",peer,"Human","Public hello");
    assert(History(&bot,CHAT_MSG_CHANNEL,0,"Trade").empty());
    Group group; group.m_Id=10;
    bot.SetGroup(&group,0);
    Remember(&bot,CHAT_MSG_PARTY,0,"",peer,"Human","Old party");
    assert(!History(&bot,CHAT_MSG_PARTY,0,"").empty());
    group.m_Id=11; assert(History(&bot,CHAT_MSG_PARTY,0,"").empty());
    bot.SetGroup(nullptr);
    sPlayerbotAIConfig.llmGlobalContext=true;
    assert(History(&bot,CHAT_MSG_SAY,0,"").find("Earlier question")!=std::string::npos);
    sPlayerbotAIConfig.llmGlobalContext=false;
    std::map<std::string,std::string> values{{"<pre prompt>",Snapshot(&bot)}, {"<context>",history}, {"<prompt>","What now?"}};
    for (auto& pair:values) pair.second=PlayerbotLLMInterface::SanitizeForJson(pair.second);
    auto json=Expand(R"({"messages":[{"role":"user","content":"<pre prompt> <context> <prompt>"}]})",values);
    sPlayerbotAIConfig.llmEndPointUrl={"127.0.0.1","/api/chat",std::stoi(argv[1]),false};
    sPlayerbotAIConfig.llmGenerationTimeout=5; sPlayerbotAIConfig.llmMaxSimultaniousGenerations=1;
    WorldPacket templ(CMSG_MESSAGECHAT); templ<<uint32(CHAT_MSG_SAY)<<uint32(LANG_COMMON);
    auto packets=ai::ChatReplyAction::GenerateResponsePackets(json,templ,templ,templ,R"(("content":\s*"))","\"","","",false,LLMChatPacing{});
    assert(!packets.empty());
    Forget(bot.GetObjectGuid().GetRawValue()); assert(History(&bot,CHAT_MSG_WHISPER,peer,"").empty());
    Configure(BotConfig{}); assert(Snapshot(&bot).empty());
    sObjectMgr.m_QuestTemplatesMap.erase(7654321);
    std::cout<<"Native snapshot, fresh quest counters, scope separation, bounded history, UTF-8 and HTTP prompt tests passed\n";
}
