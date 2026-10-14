// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once
#include "shared/Database/DatabaseEnv.h"
#include "playerbot/VMangosDatabase.h"
#include <cassert>
#include <type_traits>

inline void CheckNativeDatabaseContracts()
{
  static_assert(sizeof(uint8) == 1 && sizeof(uint16) == 2 && sizeof(uint32) == 4 && sizeof(uint64) == 8);
  static_assert(std::is_same_v<decltype(std::declval<Field>().GetCppString()), std::string>);
  auto rows = CharacterDatabase.Query("SELECT NULL, '', 'first', 255, 65535, 4294967295, 18446744073709551615, -2147483648, -9223372036854775808 UNION ALL SELECT NULL, '', 'second', 0, 0, 0, 0, 0, 0");
  assert(rows && rows->GetRowCount() == 2);
  Field* fields = rows->Fetch();
  assert(fields[0].IsNULL() && fields[0].GetString() == nullptr);
  assert(fields[0].GetCppString().empty() && fields[0].GetUInt64() == 0 && !fields[0].GetBool());
  assert(!fields[1].IsNULL() && fields[1].GetCppString().empty());
  assert(fields[3].GetUInt8() == UINT8_MAX && fields[4].GetUInt16() == UINT16_MAX);
  assert(fields[5].GetUInt32() == UINT32_MAX && fields[6].GetUInt64() == UINT64_MAX);
  assert(fields[7].GetInt32() == INT32_MIN && fields[8].GetInt64() == INT64_MIN);
  std::string owned = fields[2].GetCppString();
  assert(rows->NextRow() && rows->Fetch()[2].GetCppString() == "second");
  rows.reset();
  assert(owned == "first"); // No MYSQL_ROW pointers survive row/result destruction.

  std::string const payload = "O'Brien\\path\n100% Grüße";
  auto escaped = CharacterDatabase.PQuery("SELECT '%s'", EscapeBotSql(CharacterDatabase, payload).c_str());
  assert(escaped && escaped->Fetch()[0].GetCppString() == payload);

  // The item cache uses native bound statements; check signedness, NULL binds,
  // and parameter ownership with the actual client library as well.
  assert(CharacterDatabase.DirectExecute("CREATE TABLE pb_native_contract (u BIGINT UNSIGNED, s BIGINT, t TEXT, n TEXT)"));
  static SqlStatementID insertId;
  auto statement = CharacterDatabase.CreateStatement(insertId, "INSERT INTO pb_native_contract VALUES (?, ?, ?, ?)");
  statement.addUInt64(UINT64_MAX);
  statement.addInt64(INT64_MIN);
  std::string transient = payload;
  statement.addString(transient);
  transient = "changed after binding";
  statement.addNull();
  assert(statement.Execute());
  auto bound = CharacterDatabase.Query("SELECT u,s,t,n FROM pb_native_contract");
  assert(bound && bound->Fetch()[0].GetUInt64() == UINT64_MAX && bound->Fetch()[1].GetInt64() == INT64_MIN);
  assert(bound->Fetch()[2].GetCppString() == payload && bound->Fetch()[3].IsNULL());
  bound.reset();
  assert(CharacterDatabase.DirectExecute("DROP TABLE pb_native_contract"));
}
