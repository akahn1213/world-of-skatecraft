// SPDX-License-Identifier: GPL-2.0-or-later
// Native typed packet decode, queue classification and holder processing. The handler
// observer covers every mode; whisper/party also run native handlers and capture socket output.
#include "botpch.h"
#include "playerbot/PlayerbotMgr.h"
// Compile the production scheduler here to inspect its private lifecycle/clock state.
#include "playerbot/VMangosBotHooks.cpp"
#include "playerbot/PlayerbotAIConfig.h"
#include "playerbot/strategy/actions/SayAction.h"
#include "game/Server/Packets/Chat.h"
#include "game/Chat/MasterPlayer.h"
#include "game/Group/Group.h"
#include <cassert>
#include <iostream>
DatabaseType WorldDatabase, CharacterDatabase, LoginDatabase, LogsDatabase;
uint32 realmID = 1;
std::string realmName = "fixture";
char const* g_mainLogFileName = "fixture.log";
static std::vector<uint32> handled;
static bool nativeHandlers = false;
static std::vector<std::pair<WorldSession*, WorldPacket>> sent;
extern "C" void __wrap__ZN12WorldSession10SendPacketEPK11WorldPacket(WorldSession* session, WorldPacket const* packet)
{
    sent.emplace_back(session, *packet);
}
extern "C" void __real__ZN12WorldSession23HandleChatMessageOpcodeERKN12WorldPackets4Chat11ChatMessageE(
    WorldSession*, WorldPackets::Chat::ChatMessage const&);

extern "C" void __wrap__ZN12WorldSession23HandleChatMessageOpcodeERKN12WorldPackets4Chat11ChatMessageE(
    WorldSession* session, WorldPackets::Chat::ChatMessage const& packet)
{
    if (nativeHandlers) {
        __real__ZN12WorldSession23HandleChatMessageOpcodeERKN12WorldPackets4Chat11ChatMessageE(session, packet);
        return;
    }
    assert(packet.message == "PONG.");
    if (packet.type == CHAT_MSG_WHISPER) assert(packet.whisperTargetOrChannel == "Human");
    if (packet.type == CHAT_MSG_CHANNEL) assert(packet.whisperTargetOrChannel == "General");
    handled.push_back(packet.type);
}
static WorldPacket Chat(uint32 type)
{
    WorldPacket p(CMSG_MESSAGECHAT);
    p << type << uint32(LANG_COMMON);
    if (type == CHAT_MSG_WHISPER) p << std::string("Human");
    if (type == CHAT_MSG_CHANNEL) p << std::string("General");
    p << std::string("PONG.");
    return p;
}
int main(int argc, char** argv)
{
    assert(argc == 2);
    // Player cleanup constructs the map manager; match its native grid-state lifecycle.
    sMapMgr.InitStateMachine();
    LLMChatPacing pacing;
    assert(pacing.Delay("PONG.", false) == 3000);
    assert(pacing.Delay("PONG.", true) == 5000);
    assert(pacing.Delay(std::string(1000, 'a'), false) == 15000);
    pacing.minimum = 0; pacing.turn = 0;
    assert(pacing.Delay("é猫🙂", true) == 300);
    pacing.minimum = 6000; pacing.maximum = 100; pacing.Normalize();
    assert(pacing.maximum == 6000 && pacing.Delay("x", true) == 6000);
    pacing = {0,0,0,0,0}; pacing.Normalize(); assert(pacing.Delay("anything", true) == 0);
    pacing = {UINT32_MAX,UINT32_MAX,UINT32_MAX,UINT32_MAX,UINT32_MAX}; pacing.Normalize();
    assert(pacing.maximum == 60000 && pacing.cooldown == 3600000 && pacing.perCharacter == 1000);

    WorldPacket templ(CMSG_MESSAGECHAT); templ << uint32(CHAT_MSG_PARTY) << uint32(LANG_COMMON);
    auto packets = ai::ChatReplyAction::LinesToPackets({"First.", "*waves*", std::string(401,'x')}, templ);
    assert(packets.size() == 5 && packets[0].second == 5000 && packets[1].second == 3000);
    size_t bytes = 0;
    for (size_t i=2;i<packets.size();++i) { auto p=packets[i].first; p.rpos(8); std::string text; p >> text; bytes += text.size(); }
    assert(bytes == 401); // hard split must not discard a character
    auto limited = ai::ChatReplyAction::LinesToPackets({std::string(10000,'x')}, templ);
    assert(limited.size() == 32);

    sPlayerbotAIConfig.llmEndPointUrl = {"127.0.0.1", "/api/chat", std::stoi(argv[1]), false};
    sPlayerbotAIConfig.llmGenerationTimeout = 5;
    sPlayerbotAIConfig.llmMaxSimultaniousGenerations = 2;
    std::string start = R"(("content":\s*"))";
    for (bool debug : {false, true}) {
        auto reply = ai::ChatReplyAction::GenerateResponsePackets("{}", templ, templ, templ,
            start, "\"", "", "", debug, LLMChatPacing{});
        assert(!reply.empty() && reply.front().second == 5000);
        auto p = reply.front().first; p.rpos(8); std::string text; p >> text; assert(text == "PONG.");
        assert(reply.size() == (debug ? 2 : 1));
    }

    auto* session = new WorldSession(100, nullptr, SEC_PLAYER, 0, LOCALE_enUS);
    auto* player = new Player(session);
    player->Object::_Create(100, 0, HIGHGUID_PLAYER);
    session->SetPlayer(player); session->SetAutonomousBot();
    player->m_inWorld = true; // Queue/filter fixture only; handlers above do not access a map.
    struct Holder : PlayerbotHolder { void OnBotLoginInternal(Player*) override {} } holder;
    holder.GetAllBots()[100] = player;
    std::vector<uint32> types{CHAT_MSG_WHISPER,CHAT_MSG_PARTY,CHAT_MSG_GUILD,CHAT_MSG_OFFICER,
        CHAT_MSG_CHANNEL,CHAT_MSG_RAID,CHAT_MSG_RAID_LEADER,CHAT_MSG_RAID_WARNING,
        CHAT_MSG_BATTLEGROUND,CHAT_MSG_BATTLEGROUND_LEADER,CHAT_MSG_SAY,CHAT_MSG_YELL,CHAT_MSG_EMOTE};
    for (auto type : types) { auto p=std::make_unique<WorldPacket>(Chat(type)); session->QueueBinaryPacket(p); }
    // The real holder must leave human sessions to the normal world scheduler.
    session->SetAutonomousBot(false); holder.UpdateSessions(1); assert(handled.empty());
    session->SetAutonomousBot(); holder.UpdateSessions(1);
    assert(handled.size() == types.size());
    for (auto type : types) assert(std::count(handled.begin(), handled.end(), type) == 1);
    holder.UpdateSessions(1); assert(handled.size() == types.size());
    // Exercise actual delayed-delivery scheduler with controlled monotonic deadlines.
    sObjectAccessor.AddObject(player);
    AutonomousBots::loginGenerations[player->GetObjectGuid()] = 1;
    sPlayerbotAIConfig.llmChatPacing = LLMChatPacing{};
    auto schedule = [&](std::vector<std::pair<WorldPacket,uint32>> packets, bool ambient = false) {
        std::promise<AutonomousBots::DelayedPackets> promise;
        auto future = promise.get_future(); promise.set_value(std::move(packets));
        AutonomousBots::SchedulePackets(player, std::move(future), false, ambient);
    };
    handled.clear();
    schedule({{Chat(CHAT_MSG_WHISPER),5000},{Chat(CHAT_MSG_PARTY),5000}}, true);
    assert(!AutonomousBots::ChatAvailable(player, false));
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1); assert(handled.empty());
    auto& delivery = AutonomousBots::deliveries.front();
    delivery.due = std::chrono::steady_clock::now() - std::chrono::hours(1);
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1);
    assert(handled.size() == 1); // A late tick cannot flush the entire response.
    delivery.due = std::chrono::steady_clock::now() - std::chrono::hours(1);
    AutonomousBots::nextSpeech.clear();
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1);
    assert(handled.size() == 2 && AutonomousBots::deliveries.empty());
    assert(handled[0] == CHAT_MSG_WHISPER && handled[1] == CHAT_MSG_PARTY);
    assert(!AutonomousBots::ChatAvailable(player, true));
    assert(AutonomousBots::ChatAvailable(player, false)); // Human replies bypass ambient cooldown.
    schedule({{Chat(CHAT_MSG_WHISPER),0}});
    AutonomousBots::loginGenerations[player->GetObjectGuid()] = 2;
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1);
    assert(handled.size() == 2 && AutonomousBots::deliveries.empty());
    std::promise<AutonomousBots::DelayedPackets> slow;
    AutonomousBots::SchedulePackets(player, slow.get_future(), false);
    AutonomousBots::loginGenerations.erase(player->GetObjectGuid());
    auto beforePoll = std::chrono::steady_clock::now();
    AutonomousBots::PollDelayedPackets();
    assert(std::chrono::steady_clock::now() - beforePoll < std::chrono::milliseconds(100));
    slow.set_value({{Chat(CHAT_MSG_WHISPER),0}});
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1);
    assert(handled.size() == 2 && AutonomousBots::deliveries.empty());
    AutonomousBots::loginGenerations[player->GetObjectGuid()] = 3;
    std::promise<AutonomousBots::DelayedPackets> first;
    AutonomousBots::SchedulePackets(player, first.get_future(), false);
    schedule({{Chat(CHAT_MSG_PARTY),0}});
    AutonomousBots::nextSpeech.clear();
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1);
    assert(handled.size() == 2); // Later ready work cannot overtake a slow earlier request.
    first.set_value({{Chat(CHAT_MSG_WHISPER),0}});
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1);
    assert(handled.size() == 3 && handled.back() == CHAT_MSG_WHISPER);
    AutonomousBots::nextSpeech.clear();
    AutonomousBots::PollDelayedPackets(); holder.UpdateSessions(1);
    assert(handled.size() == 4 && handled.back() == CHAT_MSG_PARTY);
    assert(AutonomousBots::deliveries.empty());
    sObjectAccessor.RemoveObject(player);
    // Now retain the native handler and capture only its outgoing socket boundary.
    player->SetName("Bot"); player->LearnLanguage(LANG_COMMON);
    player->m_team = ALLIANCE;
    auto* humanSession = new WorldSession(101, nullptr, SEC_PLAYER, 0, LOCALE_enUS);
    auto* human = new Player(humanSession);
    human->Object::_Create(101, 0, HIGHGUID_PLAYER); human->SetName("Human");
    human->m_team = ALLIANCE; humanSession->SetPlayer(human);
    auto* master = new MasterPlayer(session); master->LoadPlayer(player);
    auto* humanMaster = new MasterPlayer(humanSession); humanMaster->LoadPlayer(human);
    session->SetMasterPlayer(master); humanSession->SetMasterPlayer(humanMaster);
    sObjectAccessor.AddObject(master); sObjectAccessor.AddObject(humanMaster);
    nativeHandlers = true;
    auto deliver = [&](uint32 type) {
        auto p = std::make_unique<WorldPacket>(Chat(type)); session->QueueBinaryPacket(p);
        holder.UpdateSessions(1);
    };
    auto countHuman = [&](uint8 type) {
        return std::count_if(sent.begin(), sent.end(), [&](auto const& item) {
            return item.first == humanSession && item.second.GetOpcode() == SMSG_MESSAGECHAT
                && item.second.size() && item.second.template read<uint8>(0) == type;
        });
    };
    deliver(CHAT_MSG_WHISPER);
    assert(countHuman(CHAT_MSG_WHISPER) == 1);
    holder.UpdateSessions(1); assert(countHuman(CHAT_MSG_WHISPER) == 1);
    Group group;
    group.m_memberSlots.push_back({player->GetObjectGuid(), "Bot", 0, false});
    group.m_memberSlots.push_back({human->GetObjectGuid(), "Human", 0, false});
    player->SetGroup(&group, 0); human->SetGroup(&group, 0);
    deliver(CHAT_MSG_PARTY); assert(countHuman(CHAT_MSG_PARTY) == 1);
    human->SetGroup(&group, 1);
    deliver(CHAT_MSG_PARTY); assert(countHuman(CHAT_MSG_PARTY) == 1); // Native subgroup filter.
    player->RemoveLanguage(LANG_COMMON);
    deliver(CHAT_MSG_WHISPER); assert(countHuman(CHAT_MSG_WHISPER) == 1); // Native language validation.
    player->SetGroup(nullptr); human->SetGroup(nullptr); group.m_memberSlots.clear();
    session->SetMasterPlayer(nullptr); humanSession->SetMasterPlayer(nullptr);
    delete master; delete humanMaster;
    humanSession->SetPlayer(nullptr); delete human; delete humanSession;
    holder.GetAllBots().clear(); player->m_inWorld = false;
    session->SetPlayer(nullptr); delete player; delete session;
    std::cout << "Native chat queue drain, exactly-once mode dispatch, human exclusion, HTTP parsing, pacing and UTF-8 checks passed\n";
}
