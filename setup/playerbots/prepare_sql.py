#!/usr/bin/env python3
"""Generate first-install SQL for the pinned vanilla bot module.

The output creates only bot-owned tables. Existing bot tables make installation fail;
this deliberately does not reset bot characters, caches, strategies, or auction history.
"""
import argparse
from pathlib import Path
import re

FILES = {
    "characters": [
        "characters/ai_playerbot_names.sql", "characters/ai_playerbot_ahbot.sql",
        "characters/ai_playerbot_cache.sql", "characters/ai_playerbot_custom_strategy.sql",
        "characters/ai_playerbot_db_store.sql", "characters/ai_playerbot_random_bots.sql",
    ],
    "world": [
        "world/ai_playerbot_texts.sql", "world/ai_playerbot_rpg_races.sql",
        "world/classic/ai_playerbot_enchants.sql", "world/classic/ai_playerbot_named_location.sql",
        "world/classic/ai_playerbot_travel_nodes.sql", "world/classic/ai_playerbot_weightscales.sql",
        "world/classic/ai_playerbot_zone_level.sql",
    ],
}


def adapt(source):
    # This upstream cleanup belongs to the deprecated CMaNGOS NPC bot menu,
    # which vmangos has never installed. Never delete native gossip entries.
    source = re.sub(r"^\s*delete FROM gossip_menu_option where option_id = 99;\s*$", "", source, flags=re.M | re.I)
    tables = re.findall(r"(?:CREATE TABLE(?: IF NOT EXISTS)?|DROP TABLE(?: IF EXISTS)?|DELETE FROM|INSERT INTO)\s+`([^`]+)`", source, re.I)
    if not tables or any(not t.startswith(("ai_playerbot_", "ahbot_")) for t in tables):
        raise ValueError("SQL contains a non-bot table or no supported tables")
    for line in source.splitlines():
        match = re.match(r"\s*(?:CREATE TABLE(?: IF NOT EXISTS)?|DROP TABLE(?: IF EXISTS)?|DELETE FROM|INSERT INTO)\s+`?([a-zA-Z0-9_]+)", line, re.I)
        if match and not match[1].startswith(("ai_playerbot_", "ahbot_")):
            raise ValueError(f"SQL touches native table: {match[1]}")
    source = re.sub(r"^\s*DROP TABLE(?: IF EXISTS)?\s+`[^`]+`;\s*$", "", source, flags=re.M | re.I)
    source = re.sub(r"^\s*DELETE FROM\s+`[^`]+`;\s*$", "", source, flags=re.M | re.I)
    source = re.sub(r"CREATE TABLE IF NOT EXISTS", "CREATE TABLE", source, flags=re.I)
    return source


def prepare_sql(module, destination):
    destination.mkdir(parents=True, exist_ok=True)
    for database, names in FILES.items():
        chunks = ["-- First installation only. Import with the SQL client stopping on errors.\n"
                  "-- Creates bot-owned tables; does not modify vmangos core tables.\n"]
        for name in names:
            chunks.append(f"\n-- Source: {name}\n" + adapt((module / "sql" / name).read_text(encoding="utf-8-sig")))
        (destination / f"{database}.sql").write_text("\n".join(chunks))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("module", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    prepare_sql(args.module, args.destination)
