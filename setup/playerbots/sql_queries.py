"""Extract active vanilla SQL literals for schema regression checks.

Only preprocessing is used; no includes or game assets are needed. Dynamic SQL
assembled through streams is covered by the linked runtime tests where exercised.
"""
import ast
from pathlib import Path
import re
import subprocess


def database_queries(module: Path, database='CharacterDatabase', compiler='c++'):
    for source in sorted(module.rglob('*.cpp')):
        text = source.read_text()
        if database + '.' not in text:
            continue
        # Evaluate the upstream expansion guards and remove C++ comments before
        # extracting literals. Inactive arena SQL must not be tested as vanilla.
        text = re.sub(r'^\s*#\s*include[^\n]*', '', text, flags=re.M)
        result = subprocess.run([compiler, '-E', '-P', '-x', 'c++', '-DCMANGOS',
                                 '-DVMANGOS', '-DMANGOSBOT_ZERO', '-DPLATFORM=0',
                                 '-DPLATFORM_WINDOWS=1', '-'], input=text,
                                capture_output=True, text=True, check=True)
        pattern = re.escape(database) + r'\.(?:PQuery|PExecute|Query|Execute|DirectPExecute)\s*\(\s*((?:"(?:[^"\\]|\\.)*"\s*)+)'
        for match in re.finditer(pattern, result.stdout):
            query = ''.join(ast.literal_eval(v) for v in re.findall(r'"(?:[^"\\]|\\.)*"', match[1]))
            query = query.replace('%%', '__PERCENT__')
            query = re.sub(r'%[-+0 #]*\d*(?:\.\d+)?(?:ll|l|z)?[sudiIfFgGeExX]', '1', query)
            yield source.relative_to(module), query.replace('__PERCENT__', '%')


def character_queries(module: Path, compiler="c++"):
    return database_queries(module, "CharacterDatabase", compiler)
