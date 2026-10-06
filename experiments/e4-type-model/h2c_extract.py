"""Extract geometry WKT/EWKT literals (SQL inputs) and geometry values (expected outputs) from
the postgis_docs .slt records, for the H2c round-trip test."""
import re, json
from scripts_parse_docs import records

KW = r'(?:SRID=-?\d+;)?(?:POINT|LINESTRING|POLYGON|MULTIPOINT|MULTILINESTRING|MULTIPOLYGON|GEOMETRYCOLLECTION|CIRCULARSTRING|COMPOUNDCURVE|CURVEPOLYGON|MULTICURVE|MULTISURFACE|POLYHEDRALSURFACE|TIN|TRIANGLE)\b'

def geoms_in(s):
    """Top-level geometry values in s (balanced parentheses or EMPTY)."""
    out = []
    i = 0
    while True:
        m = re.compile(KW, re.I).search(s, i)
        if not m:
            break
        j = m.end()
        # optional Z/M/ZM and whitespace
        mm = re.compile(r'\s*(ZM|Z|M)?\s*', re.I).match(s, j)
        j = mm.end()
        if s[j:j+5].upper() == 'EMPTY':
            out.append(s[m.start():j+5]); i = j + 5; continue
        if j < len(s) and s[j] == '(':
            depth = 0; k = j
            while k < len(s):
                if s[k] == '(': depth += 1
                elif s[k] == ')':
                    depth -= 1
                    if depth == 0: break
                k += 1
            out.append(s[m.start():k+1]); i = k + 1
        else:
            i = j
    return out

recs = records()
out = []
for i, (f, l, sql, exp, h) in enumerate(recs):
    for m in re.finditer(r"'((?:[^']|'')*)'", sql):
        s = m.group(1).strip()
        if re.match(KW, s, re.I):
            out.append(dict(rec=i, file=f, line=l, kind='input', text=' '.join(s.split())))
    for g in geoms_in(exp):
        out.append(dict(rec=i, file=f, line=l, kind='expected', text=' '.join(g.split())))
json.dump(dict(n_records=len(recs), lits=out), open('h2c_literals.json', 'w'))
print(len(recs), 'records;', len(out), 'literals')

# Normalize EWKT-isms the wkt crate doesn't parse: implicit Z/ZM (3 or 4 numbers per coordinate
# without a Z/M tag) and POINTM-style tags without a space.
TYPES = r'(POINT|LINESTRING|POLYGON|MULTIPOINT|MULTILINESTRING|MULTIPOLYGON|GEOMETRYCOLLECTION|TRIANGLE|TIN|POLYHEDRALSURFACE)'
def normalize(t):
    srid = ''
    m = re.match(r'(SRID=-?\d+;)(.*)', t, re.I | re.S)
    if m:
        srid, t = m.group(1), m.group(2)
    t = re.sub(TYPES + r'(ZM|M)\b', r'\1 \2', t, flags=re.I)
    if not re.search(TYPES + r'\s+(ZM|Z|M)\b', t, re.I):
        m = re.search(r'\(\s*([-+.\deE]+(?:\s+[-+.\deE]+)+)\s*[,)]', t)
        if m:
            n = len(m.group(1).split())
            tag = {3: 'Z', 4: 'ZM'}.get(n)
            if tag:
                t = re.sub(TYPES + r'(?=\s*\(|\s+EMPTY)', lambda mm: mm.group(1) + ' ' + tag, t, flags=re.I)
    return srid + t

d = json.load(open('h2c_literals.json'))
for o in d['lits']:
    o['text_raw'] = o['text']
    o['text'] = normalize(o['text'])
json.dump(d, open('h2c_literals.json', 'w'))
print('normalized', sum(1 for o in d['lits'] if o['text'] != o['text_raw']))
