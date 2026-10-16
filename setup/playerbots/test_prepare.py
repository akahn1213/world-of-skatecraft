#!/usr/bin/env python3
"""Source preparation tests; these do not claim the bot runtime is working."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import prepare


class PreparationTests(unittest.TestCase):
    def test_existing_destination_is_never_modified(self):
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "existing"
            destination.mkdir()
            sentinel = destination / "keep"
            sentinel.write_text("user files")
            with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
                prepare.prepare(destination)
            self.assertEqual(sentinel.read_text(), "user files")
            self.assertEqual(list(destination.iterdir()), [sentinel])

    def test_failed_patch_changes_no_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            file = root / "example"
            file.write_text("keep\n")
            invalid = b"diff --git a/example b/example\n--- a/example\n+++ b/example\n@@ -1 +1 @@\n-wrong\n+changed\n"
            with self.assertRaises(subprocess.CalledProcessError):
                prepare.apply(root, invalid)
            self.assertEqual(file.read_text(), "keep\n")

    @unittest.skipUnless(os.environ.get("PLAYERBOTS_PREPARED_SOURCE") and os.environ.get("PLAYERBOTS_UPSTREAM_SOURCE"),
                         "set prepared/upstream paths to verify the real source tree")
    def test_pinned_prepared_tree_retains_complete_module(self):
        core = Path(os.environ["PLAYERBOTS_PREPARED_SOURCE"])
        upstream = Path(os.environ["PLAYERBOTS_UPSTREAM_SOURCE"])
        bots = core / "src/game/AutonomousBots"
        for name, tree in [("vmangos", core), ("playerbots", bots)]:
            revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tree, text=True).strip()
            self.assertEqual(revision, prepare.SOURCES[name]["revision"])
        tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=upstream).decode().split("\0")
        for name in filter(None, tracked):
            self.assertTrue((bots / name).is_file(), name)
            if name.endswith(".sql") or ("/aiplayerbot.conf" in name and name != "playerbot/aiplayerbot.conf.dist.in"):
                self.assertEqual((bots / name).read_bytes(), (upstream / name).read_bytes(), name)
        config = (bots / "playerbot/aiplayerbot.conf.dist.in").read_text()
        self.assertIn("AiPlayerbot.LLMChatMinDelayMs = 3000", config)
        self.assertIn("AiPlayerbot.LLMConversationTurnDelayMs = 5000", config)
        self.assertIn("AiPlayerbot.LLMConversationCooldownMs = 60000", config)
        self.assertIn("AiPlayerbot.LLMBotToBotChatChance = 50", config)
        self.assertTrue((bots / "playerbot/LLMChatPacing.h").is_file())
        self.assertTrue((core / "src/game/Tarnished.cpp").is_file())
        self.assertIn("CollisionDamage", (core / "src/game/Skatecraft.cpp").read_text())
        self.assertEqual((bots / "vmangos.cmake").read_bytes(), (prepare.HERE / "vmangos.cmake").read_bytes())


if __name__ == "__main__":
    unittest.main()
