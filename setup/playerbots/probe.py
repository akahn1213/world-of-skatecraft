#!/usr/bin/env python3
"""Compile the full experimental module or selected integration units; failures remain failures."""
import argparse
import json
from pathlib import Path
import subprocess

CRITICAL = {
    "PlayerbotAI.cpp", "PlayerbotMgr.cpp", "RandomPlayerbotMgr.cpp", "PlayerbotFactory.cpp",
    "ServerFacade.cpp", "WorldPosition.cpp", "QuestAction.cpp", "TravelMgr.cpp",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("build", type=Path, help="CMake/Ninja build directory with compile_commands.json")
    parser.add_argument("--critical", action="store_true", help="probe eight integration units instead of the complete module")
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    entries = json.loads((args.build / "compile_commands.json").read_text())
    module = [e for e in entries if "/AutonomousBots/" in e["file"]]
    if not module:
        parser.error("no Playerbots sources configured; configure with BUILD_PLAYERBOTS=ON")
    targets = ["autonomous_playerbots"]
    if args.critical:
        selected = [e for e in module if Path(e["file"]).name in CRITICAL]
        missing = CRITICAL - {Path(e["file"]).name for e in selected}
        if missing:
            parser.error(f"missing integration sources: {sorted(missing)}")
        targets = ["src/game/CMakeFiles/autonomous_playerbots.dir/AutonomousBots/" +
                   e["file"].split("/AutonomousBots/", 1)[1] + ".o" for e in selected]
    print(f"Configured {len(module)} upstream translation units; building {len(targets) if args.critical else 'all'}.", flush=True)
    result = subprocess.run(["ninja", "-C", str(args.build), "-k", "0", "-j", str(args.jobs), *targets])
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
