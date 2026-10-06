"""H15 validity check: is first-page precision representative of the whole result set?

For a set of noisy or high-count names, fetch a deeper result page (page 5, results 401-500)
in SQL and compare its precision with page 1's.

Usage: uv run --with requests --with scipy python fetch_deep.py
"""

import common
from analyze_h15 import item_class

NAMES = ["ST_X", "ST_Y", "ST_Z", "ST_M", "ST_Point", "ST_Union", "GeometryType", "ST_Area",
         "ST_Length", "ST_Intersects", "Box2D", "ST_Collect", "ST_Transform", "ST_Buffer",
         "ST_Snap", "ST_Project", "ST_Points", "ST_AsX3D", "ST_CurveToLine"]
PAGE = 5


def precision(name: str, page: int) -> tuple[int, int, int]:
    d = common.gh_code_search(f'"{name}" language:SQL', page=page)
    items = d.get("items", [])
    use = sum(1 for it in items if item_class(name, it) == "use")
    return d.get("total_count", 0), len(items), use


def main() -> None:
    print("| Name | SQL total | Page 1 precision | Page 5 precision |")
    print("|---|---|---|---|")
    for name in NAMES:
        total, n1, u1 = precision(name, 1)
        if total < PAGE * 100:
            print(f"| {name} | {total} | {u1}/{n1} | (fewer than {PAGE * 100} results) |")
            continue
        _, n5, u5 = precision(name, PAGE)
        print(f"| {name} | {total} | {u1}/{n1} = {u1 / max(n1, 1):.2f} | {u5}/{n5} = {u5 / max(n5, 1):.2f} |")


if __name__ == "__main__":
    main()
