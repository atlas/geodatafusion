"""Parse postgis_docs .slt files into (file, n, sql, expected) records that call ST_Transform*."""
import glob
import os
import re

SLT = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                   "../../../rust/geodatafusion/tests/sqllogictests/slt/postgis_docs")


def records():
    out = []
    for path in sorted(glob.glob(os.path.join(SLT, "*.slt"))):
        lines = open(path).read().split("\n")
        i, n = 0, 0
        while i < len(lines):
            if lines[i].startswith("query "):
                j = i + 1
                sql = []
                while j < len(lines) and lines[j] != "----" and lines[j].strip() != "":
                    sql.append(lines[j]); j += 1
                if j >= len(lines) or lines[j] != "----":
                    i = j
                    continue
                j += 1
                exp = []
                while j < len(lines) and lines[j].strip() != "":
                    exp.append(lines[j]); j += 1
                sqls = "\n".join(sql)
                if re.search(r"st_(inverse)?transform", sqls, re.I):
                    n += 1
                    out.append((os.path.basename(path), n, sqls, "\n".join(exp)))
                i = j
            else:
                i += 1
    return out


if __name__ == "__main__":
    for f, n, s, e in records():
        print(f"== {f} #{n}\n{s}\n-> {e}")
