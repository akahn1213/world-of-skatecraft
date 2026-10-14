#!/usr/bin/env python3
"""Load actual native/bot schemas into disposable MariaDB databases over a supplied socket.

Never connects over TCP or touches an existing database. Requires a test account allowed
 to create databases. Usage: test_sql.py PREPARED_SOURCE SOCKET [--client mariadb]
"""
import argparse
import re
from pathlib import Path
import subprocess
import tempfile
import uuid

from prepare_sql import prepare_sql
from sql_queries import database_queries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("socket", type=Path)
    parser.add_argument("--client", default="mariadb")
    parser.add_argument("--user", default="root")
    args = parser.parse_args()
    client = [args.client, "--no-defaults", f"--socket={args.socket}", f"--user={args.user}", "--batch", "--skip-column-names"]

    def sql(text, database=None, check=True):
        return subprocess.run(client + ([database] if database else []), input=text, text=True, capture_output=True, check=check)

    prefix = "playerbots_test_" + uuid.uuid4().hex[:12]
    databases = [prefix + "_characters", prefix + "_world", prefix + "_logon"]
    created = []
    try:
        with tempfile.TemporaryDirectory(prefix="playerbots-sql-") as temp:
            output = Path(temp)
            prepare_sql(args.source / "src/game/AutonomousBots", output)
            for name in databases:
                sql(f"CREATE DATABASE `{name}`")
                created.append(name)
            characters, world, logon = databases
            sql((args.source / "sql/characters.sql").read_text(), characters)
            sql((args.source / "sql/logon.sql").read_text(), logon)
            char_sql = (output / "characters.sql").read_text()
            sql(char_sql, characters)
            sql((output / "world.sql").read_text(), world)
            checked = 0
            for source, query in database_queries(args.source / "src/game/AutonomousBots"):
                # Explain verifies the actual referenced tables/columns without
                # executing deletes, updates or inserts from the runtime source.
                result = sql("EXPLAIN " + query, characters, check=False)
                assert result.returncode == 0, f"{source}: {query}\n{result.stderr}"
                checked += 1
            login_checked = 0
            for source, query in database_queries(args.source / "src/game/AutonomousBots", "LoginDatabase"):
                result = sql("EXPLAIN " + query, logon, check=False)
                assert result.returncode == 0, f"{source}: {query}\n{result.stderr}"
                login_checked += 1
            assert login_checked >= 10, login_checked
            print(f"Validated {login_checked} active vanilla login-database SQL literals")
            assert checked >= 75, checked
            print(f"Validated {checked} active vanilla character-database SQL literals")
            world_checked = 0
            for source, query in database_queries(args.source / "src/game/AutonomousBots", "WorldDatabase"):
                # The repository supplies bot world tables, not the full game-world
                # database. Validate only queries whose tables are all in those seeds.
                tables = re.findall(r"\b(?:FROM|JOIN|INTO|UPDATE)\s+`?(\w+)", query, re.I)
                if not tables or not all(table.startswith("ai_playerbot_") for table in tables):
                    continue
                if not re.match(r"\s*(SELECT|INSERT|UPDATE|DELETE)\b", query, re.I):
                    continue
                result = sql("EXPLAIN " + query, world, check=False)
                assert result.returncode == 0, f"{source}: {query}\n{result.stderr}"
                world_checked += 1
            assert world_checked >= 15, world_checked
            print(f"Validated {world_checked} bot-world SQL literals against their installed schemas")
            # Native renamed fields exercised by login, guild, and auction code.
            sql("SELECT account,guid,race,class,level,online,played_time_total,map,position_x,position_y,position_z,orientation FROM characters; SELECT guild_id,guid FROM guild_member; SELECT guild_id,leader_guid FROM guild; UPDATE auction SET buyer_guid=0,last_bid=0 WHERE id=0;", characters)
            assert int(sql("SELECT COUNT(*) FROM ai_playerbot_names", characters).stdout) > 1000
            assert int(sql("SELECT COUNT(*) FROM ai_playerbot_travelnode", world).stdout) > 100
            assert int(sql("SELECT COUNT(*) FROM ai_playerbot_weightscales", world).stdout) > 0
            sql("INSERT INTO ai_playerbot_db_store (guid,preset,`key`,`value`) VALUES (123,'test','keep','persisted')", characters)
            retry = sql(char_sql, characters, check=False)
            assert retry.returncode != 0 and "already exists" in retry.stderr
            assert sql("SELECT `value` FROM ai_playerbot_db_store WHERE guid=123 AND preset='test' AND `key`='keep'", characters).stdout.strip() == "persisted"
            native_world = sql("SELECT table_name FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name NOT LIKE 'ai_playerbot_%' AND table_name NOT LIKE 'ahbot_%'", world).stdout.strip()
            assert not native_world, native_world
            print("Native character schema, vanilla bot seeds, and preservation on repeated installation passed")
    finally:
        for name in created:
            sql(f"DROP DATABASE `{name}`")


if __name__ == "__main__":
    main()
