"""H15: fetch GitHub code search results (page 1, 100 items, text matches) for every searchable
inventory function, per language. Responses are cached; rerunning only fetches what's missing.

Usage: uv run --with requests python fetch_h15.py SQL PLpgSQL [Python ...]
"""

import sys

import common


def query(name: str, lang: str) -> str:
    return f'"{name}" language:{lang}'


def main() -> None:
    langs = sys.argv[1:] or ["SQL", "PLpgSQL"]
    rows = [r for r in common.inventory() if common.searchable(r)]
    for i, r in enumerate(rows):
        for lang in langs:
            data = common.gh_code_search(query(r["name"], lang))
            print(f"{i + 1}/{len(rows)} {r['name']} {lang}: {data.get('total_count', data)}", flush=True)


if __name__ == "__main__":
    main()
