#!/usr/bin/env python3
"""Run the pinned action extractor against the player's read-only unpacked ER files."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
from urllib.request import urlopen

REVISION = "177a3b5ec14a9bde39b9ab458a3400b4038224e3"
TOOLS = ("extract", "paths", "erfmt", "hkanim", "hkx", "param", "tae", "skel")
ROOT = Path(__file__).resolve().parent.parent


def main():
    inputs = []
    for variable, required in (("ER_FILES", "chr"), ("ER_GAME_DIR", "oo2core_6_win64.dll")):
        value = os.environ.get(variable)
        if not value or not (Path(value) / required).exists():
            sys.exit(f"Set {variable} to your own folder containing {required}; no files were written.")
        inputs.append(Path(value).resolve())
    workspace = ROOT / ".setup" / "tarnished"
    if any(workspace.resolve().is_relative_to(path) for path in inputs):
        sys.exit("Extraction output must be outside both input folders.")
    if sys.platform != "win32":
        if not shutil.which("wine") or not os.environ.get("WINE_PYTHON"):
            sys.exit("Linux/macOS extraction requires Wine and WINE_PYTHON pointing to Windows python.exe.")
        wine_python = Path(os.environ["WINE_PYTHON"]).resolve()
        if not wine_python.is_file():
            sys.exit("WINE_PYTHON must name an existing Windows python.exe; no automatic download.")
    tools = workspace / "tools"
    tools.mkdir(parents=True, exist_ok=True)
    (workspace / "src" / "sim").mkdir(parents=True, exist_ok=True)
    for name in TOOLS:
        url = f"https://raw.githubusercontent.com/Funny-Bones/ELDEN-RING-Combat-Rewrite/{REVISION}/tools/{name}.py"
        with urlopen(url, timeout=60) as response:
            (tools / f"{name}.py").write_bytes(response.read())
    env = os.environ.copy()
    env["ER_FILES"], env["ER_GAME_DIR"] = map(str, inputs)
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    scratch = workspace / "scratch"
    scratch.mkdir(exist_ok=True)
    for key in ("TMPDIR", "TMP", "TEMP"):
        env[key] = str(scratch)
    if sys.platform != "win32":
        env["WINE_PYTHON"] = str(wine_python)
        env["WINEPREFIX"] = str(workspace / "wine")
    subprocess.run([sys.executable, str(tools / "extract.py")], cwd=workspace, env=env, check=True)
    table = workspace / "src" / "sim" / "extracted.rs"
    if not table.is_file() or table.stat().st_size == 0:
        sys.exit("Extractor did not produce an action table.")
    print(f"Generated {table}; validate with cargo test -p benilla-tarnished --features er-data")


if __name__ == "__main__":
    main()
