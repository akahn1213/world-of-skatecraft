#!/usr/bin/env python3
"""Exercise WMO names, model collision width, and emote sounds with synthetic DBCs.

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
        model = [0] * 16
        model[0], model[1], model[2] = 9, 3, 1
        for field, value in ((4, 1.25), (14, 0.61), (15, 2.17)):
            model[field] = struct.unpack("<I", struct.pack("<f", value))[0]
        model_strings = b"\0TestModel.m2\0"
        (root / "model.dbc").write_bytes(b"WDBC" + struct.pack("<4I", 1, 16, 64, len(model_strings)) + struct.pack("<16I", *model) + model_strings)
        (root / "emote.dbc").write_bytes(b"WDBC" + struct.pack("<4I", 1, 5, 20, 1) + struct.pack("<5I", 4, 101, 3, 1, 999) + b"\0")
        test = root / "test.cpp"
        test.write_text(r'''
#include "Database/DBCFileLoader.h"
#include "DBCStructure.h"
#include "DBCfmt.h"
#include <cstring>
#include <iostream>
#include <cmath>
int main(int argc, char** argv)
{
    if (argc != 4) return 1;
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
    if (DBCFileLoader::GetFormatRecordSize(CreatureModelDatafmt) != sizeof(CreatureModelDataEntry)) return 6;
    DBCFileLoader models;
    if (!models.Load(argv[2], CreatureModelDatafmt)) return 7;
    data = models.AutoProduceData(CreatureModelDatafmt, count, index);
    strings = models.AutoProduceStrings(CreatureModelDatafmt, data);
    auto model = reinterpret_cast<CreatureModelDataEntry const*>(index[9]);
    valid = model && model->ID == 9 && model->flags == 3 && std::strcmp(model->modelName, "TestModel.m2") == 0
        && std::abs(model->modelScale - 1.25f) < .0001f && std::abs(model->collisionWidth - .61f) < .0001f
        && std::abs(model->collisionHeight - 2.17f) < .0001f;
    delete[] strings; delete[] data; delete[] index;
    if (!valid) return 8;
    if (DBCFileLoader::GetFormatRecordSize(EmotesTextSoundEntryfmt) != sizeof(EmotesTextSoundEntry)) return 9;
    DBCFileLoader emotes;
    if (!emotes.Load(argv[3], EmotesTextSoundEntryfmt)) return 10;
    data = emotes.AutoProduceData(EmotesTextSoundEntryfmt, count, index);
    auto emote = reinterpret_cast<EmotesTextSoundEntry const*>(index[4]);
    valid = emote && emote->Id == 4 && emote->EmotesTextId == 101 && emote->RaceId == 3 && emote->SexId == 1 && emote->SoundId == 999;
    delete[] data; delete[] index;
    if (!valid) return 11;
    std::cout << "WMO names, model collision dimensions, and emote-sound DBC layouts passed\n";
}
''')
        binary = root / "test"
        subprocess.run([args[0], *flags, str(test),
                        str(source / "src/shared/Database/DBCFileLoader.cpp"),
                        "-o", str(binary)], cwd=command["directory"], check=True)
        subprocess.run([str(binary), str(root / "test.dbc"), str(root / "model.dbc"), str(root / "emote.dbc")], check=True)


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve())
