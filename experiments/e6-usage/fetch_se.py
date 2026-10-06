"""H15 secondary source: GIS Stack Exchange post counts (questions and answers) per function.

Uses /2.3/search/excerpts?site=gis&q=<name>&filter=total. Without an API key the quota is about
300 requests/day per IP, so this fetches a fixed sample: every second searchable inventory
function (inventory order) plus every function proposed for skipping.

Usage: uv run --with requests python fetch_se.py
"""

import time

import common

SKIP = ["ST_GeomFromGML", "ST_GMLToSQL", "ST_GeomFromKML", "ST_GeomFromMARC21", "ST_AsMARC21",
        "ST_AsX3D", "ST_Letters", "ST_MemSize", "ST_HasArc", "ST_CurveToLine", "ST_ForceSFS",
        "ST_LineToCurve", "ST_ForceCurve"]


def sample() -> list[str]:
    names = [r["name"] for r in common.inventory() if common.searchable(r)]
    picked = names[::2]
    picked += [n for n in SKIP if n not in picked]
    return picked


def main() -> None:
    for name in sample():
        d = common.cached_get("se", "https://api.stackexchange.com/2.3/search/excerpts",
                              {"q": name, "site": "gis", "filter": "total"}, interval=1.0)
        print(name, d.get("total"), flush=True)
        if "backoff" in d:
            time.sleep(d["backoff"] + 1)


if __name__ == "__main__":
    main()
