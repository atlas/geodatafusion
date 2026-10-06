"""Parse postgis_docs .slt files into (file, line, sql, expected) records."""
import re, glob, os
DIR = os.path.join(os.path.dirname(__file__), '../../rust/geodatafusion/tests/sqllogictests/slt/postgis_docs')
def records():
    out = []
    for f in sorted(glob.glob(os.path.join(DIR, '*.slt'))):
        lines = open(f).read().split('\n')
        i = 0
        while i < len(lines):
            l = lines[i]
            if l.startswith('query') or l.startswith('statement'):
                start = i + 1
                sql = []
                i += 1
                while i < len(lines) and lines[i].strip() != '' and not lines[i].startswith('----'):
                    sql.append(lines[i]); i += 1
                exp = []
                if i < len(lines) and lines[i].startswith('----'):
                    i += 1
                    while i < len(lines) and lines[i].strip() != '':
                        exp.append(lines[i]); i += 1
                out.append((os.path.basename(f), start, '\n'.join(sql), '\n'.join(exp), l))
            i += 1
    return out
