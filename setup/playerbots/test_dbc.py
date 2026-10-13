#!/usr/bin/env python3
"""Exercise the port's WMO name layout with synthetic data, not game assets.

Usage: python3 setup/playerbots/test_dbc.py BUILD_DIRECTORY
Uses that build's compiler and include paths to test the actual patched loader.
"""
import json
from pathlib import Path
import shlex
import struct
import subprocess
import sys
import tempfile


def main(build):
    commands = json.loads((build / "compile_commands.json").read_text())
    command = next(c for c in commands if c["file"].endswith("/Database/DBCStores.cpp"))
    source = Path(command["file"]).resolve().parents[3]
    args = command.get("arguments") or shlex.split(command["command"])
    flags = []
    skip = False
    for arg in args[1:]:
        if skip:
            skip = False
            continue
        if arg in ("-o", "-MF", "-MT", "-MQ"):
            skip = True
        elif arg not in ("-c", "-MD", "-MMD", command["file"]):
            flags.append(arg)
    with tempfile.TemporaryDirectory(prefix="playerbots-dbc-") as temp:
        root = Path(temp)
        strings = b"\0Bot Test Hall\0Salle de test\0"
        # WMOAreaTable: ID, root, ADT, group, five ignored fields, flags,
        # area ID, eight localized strings, locale mask. Negative group is valid.
        row = [7, 123, 4, 0xFFFFFFFF, 0, 0, 0, 0, 0, 8, 42, 1, 15, 0, 0, 0, 0, 0, 0, 0]
        (root / "test.dbc").write_bytes(
            b"WDBC" + struct.pack("<4I", 1, 20, 80, len(strings))
            + struct.pack("<20I", *row) + strings
        )
        test = root / "test.cpp"
        test.write_text(r'''
#include "Database/DBCFileLoader.h"
#include "DBCStructure.h"
#include "DBCfmt.h"
#include <cstring>
#include <iostream>
int main(int argc, char** argv)
{
    if (argc != 2) return 1;
    if (DBCFileLoader::GetFormatRecordSize(WMOAreaTableEntryfmt) != sizeof(WMOAreaTableEntry)) return 2;
    DBCFileLoader loader;
    if (!loader.Load(argv[1], WMOAreaTableEntryfmt)) return 3;
    uint32 count = 0;
    char** index = nullptr;
    char* data = loader.AutoProduceData(WMOAreaTableEntryfmt, count, index);
    if (!data || count <= 7 || !index[7]) return 4;
    char* strings = loader.AutoProduceStrings(WMOAreaTableEntryfmt, data);
    auto entry = reinterpret_cast<WMOAreaTableEntry const*>(index[7]);
    bool valid = entry->Id == 7 && entry->rootId == 123 && entry->adtId == 4
        && entry->groupId == -1 && entry->Flags == 8 && entry->areaId == 42
        && std::strcmp(entry->Name[0], "Bot Test Hall") == 0
        && std::strcmp(entry->Name[1], "Salle de test") == 0
        && entry->Name[7][0] == '\0';
    delete[] strings;
    delete[] data;
    delete[] index;
    if (!valid) return 5;
    std::cout << "WMO DBC layout and localized names passed\n";
}
''')
        binary = root / "test"
        subprocess.run([args[0], *flags, str(test),
                        str(source / "src/shared/Database/DBCFileLoader.cpp"),
                        "-o", str(binary)], cwd=command["directory"], check=True)
        subprocess.run([str(binary), str(root / "test.dbc")], check=True)


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
