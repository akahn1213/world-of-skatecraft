#!/usr/bin/env python3
"""Test independent bot/auction configuration and atomic reload without game assets."""
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


def main(build):
    commands = json.loads((build / "compile_commands.json").read_text())
    command = next(c for c in commands if c["file"].endswith("/VMangosBotConfig.cpp"))
    args = shlex.split(command["command"])
    flags = []
    skip = False
    for arg in args[1:]:
        if skip:
            skip = False
        elif arg in ("-o", "-MF", "-MT", "-MQ", "-include"):
            skip = True
        elif arg not in ("-c", "-MD", "-MMD", command["file"], "-DNDEBUG") and not arg.startswith("-O"):
            flags.append(arg)
    with tempfile.TemporaryDirectory(prefix="playerbots-config-") as temp:
        root = Path(temp)
        (root / "ai.conf").write_text('[Playerbots]\nAiPlayerbot.Enabled = yes\nAiPlayerbot.Count = 4\nAiPlayerbot.Label = "Mixed Case"\n')
        (root / "auction.conf").write_text('AiPlayerbot.Enabled = no\nAiPlayerbot.Count = 19\n')
        source = root / "test.cpp"
        source.write_text(r'''
#include "playerbot/VMangosBotConfig.h"
#include "playerbot/VMangosConfigPath.h"
#include <cstdlib>
#include <cassert>
#include <fstream>
int main(int argc, char** argv)
{
    assert(argc == 3);
    unsetenv("PLAYERBOTS_CONFIG_DIR");
    auto directory = std::filesystem::path(argv[1]).parent_path();
    assert(ResolveBotConfigPath((directory / "mangosd.conf").string(), "aiplayerbot.conf") == (directory / "aiplayerbot.conf").string());
    setenv("PLAYERBOTS_CONFIG_DIR", "/relocated/container/config", 1);
    assert(ResolveBotConfigPath("/old/build/etc/mangosd.conf", "aiplayerbot.conf") == "/relocated/container/config/aiplayerbot.conf");
    assert(ResolveBotConfigPath("/old/build/etc/mangosd.conf", "playerbots-ahbot.conf") == "/relocated/container/config/playerbots-ahbot.conf");
    unsetenv("PLAYERBOTS_CONFIG_DIR");
    BotConfig ai, auction;
    assert(ai.SetSource(argv[1], "PLAYERBOTS_TEST_"));
    assert(auction.SetSource(argv[2], "AUCTION_TEST_"));
    assert(ai.GetBoolDefault("aiplayerbot.enabled", false));
    assert(!auction.GetBoolDefault("AiPlayerbot.Enabled", true));
    assert(ai.GetIntDefault("AiPlayerbot.Count", 0) == 7);
    assert(auction.GetIntDefault("AiPlayerbot.Count", 0) == 19);
    assert(ai.GetStringDefault("AIPLAYERBOT.LABEL") == "Mixed Case");
    assert(ai.GetValues("aiplayerbot.").size() == 3);
    assert(ai.GetFloatDefault("Missing", 0.75f) == 0.75f);
    std::ofstream(argv[1]) << "AiPlayerbot.Enabled = no\ninvalid line\n";
    assert(!ai.Reload());
    assert(ai.GetBoolDefault("AiPlayerbot.Enabled", false));
    assert(auction.GetIntDefault("AiPlayerbot.Count", 0) == 19);
}
''')
        executable = root / "config-test"
        subprocess.run([args[0], *flags, "-O0", str(source), command["file"], "-o", str(executable), "-pthread"], cwd=command["directory"], check=True)
        env = dict(os.environ, PLAYERBOTS_TEST_AiPlayerbot_Count="7")
        subprocess.run([str(executable), str(root / "ai.conf"), str(root / "auction.conf")], env=env, check=True)
    print("Configuration relocation, independent settings, environment overrides, and atomic failed reload passed")


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
