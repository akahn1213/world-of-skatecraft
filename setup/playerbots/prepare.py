#!/usr/bin/env python3
"""Prepare the complete, pinned experimental Playerbots port outside the live deployment.

This prepares source only. It never imports SQL, changes compose, or enables bots.
Build the mangosd target with BUILD_PLAYERBOTS=ON; see README.md for installation.
"""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SOURCES = json.loads((HERE / "sources.json").read_text())
sys.path.insert(0, str(ROOT / "setup/vmangos"))
from prepare_patch import prepare as prepare_tarnished
from prepare_sql import prepare_sql


def run(*args, cwd=None):
    subprocess.run(args, cwd=cwd, check=True)


def checkout(name, destination, cache=None):
    source = SOURCES[name]
    if cache:
        run("git", "clone", "--no-hardlinks", "--no-checkout", str(cache.resolve()), str(destination))
    else:
        run("git", "init", str(destination))
        run("git", "remote", "add", "origin", source["url"], cwd=destination)
        run("git", "fetch", "--depth", "1", "origin", source["revision"], cwd=destination)
    if cache and subprocess.run(["git", "cat-file", "-e", source["revision"] + "^{commit}"],
                                cwd=destination, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
        run("git", "fetch", "--depth", "1", source["url"], source["revision"], cwd=destination)
    run("git", "checkout", "--detach", source["revision"], cwd=destination)
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=destination, text=True).strip()
    if actual != source["revision"]:
        raise RuntimeError(f"{name}: expected {source['revision']}, got {actual}")


def apply(tree, patch, normalize=False):
    if normalize:
        # Port patches use LF. Normalize only the files they touch in this disposable tree.
        for name in re.findall(r"^\+\+\+ b/(.+)$", patch.decode(), re.M):
            file = tree / name
            if file.is_file():
                file.write_bytes(file.read_bytes().replace(b"\r\n", b"\n"))
    subprocess.run(["git", "apply", "--check", "--whitespace=nowarn", "-"], cwd=tree, input=patch, check=True)
    subprocess.run(["git", "apply", "--whitespace=nowarn", "-"], cwd=tree, input=patch, check=True)


def prepare(destination, core_cache=None, bots_cache=None):
    destination = destination.resolve()
    if destination.exists():
        raise ValueError(f"refusing to overwrite existing destination: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Publish a complete source tree only after every pin and patch has validated.
    with tempfile.TemporaryDirectory(prefix=".playerbots-", dir=destination.parent) as staging:
        tree = Path(staging) / "source"
        checkout("vmangos", tree, core_cache)
        apply(tree, (ROOT / "setup/vmangos/skatecraft.patch").read_bytes())
        apply(tree, prepare_tarnished((ROOT / "setup/vmangos/tarnished.patch").read_bytes()))
        apply(tree, (HERE / "core.patch").read_bytes(), normalize=True)
        apply(tree, (HERE / "energy.patch").read_bytes(), normalize=True)
        bots = tree / "src/game/AutonomousBots"
        checkout("playerbots", bots, bots_cache)
        apply(bots, (HERE / "module.patch").read_bytes(), normalize=True)
        apply(bots, (HERE / "conversation.patch").read_bytes(), normalize=True)
        shutil.copy2(HERE / "vmangos.cmake", bots / "vmangos.cmake")
        shutil.copy2(HERE / "sources.json", tree / "playerbots-sources.json")
        prepare_sql(bots, tree / "sql/playerbots")
        tree.rename(destination)
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--core-cache", type=Path, help="optional local git clone containing the pinned vmangos revision")
    parser.add_argument("--bots-cache", type=Path, help="optional local git clone containing the pinned Playerbots revision")
    args = parser.parse_args()
    try:
        output = prepare(args.destination, args.core_cache, args.bots_cache)
    except (ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Playerbots source preparation failed: {error}\n")
    print(f"Prepared Playerbots source at {output}. Build mangosd and install the bot schema/configuration before enabling bots.")


if __name__ == "__main__":
    main()
