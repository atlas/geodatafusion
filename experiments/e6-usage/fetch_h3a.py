"""H3a: downstream users of geodatafusion.

- crates.io crate metadata and reverse dependencies (every dependent version is checked).
- Source of each dependent crate's latest version (static.crates.io), unpacked for grepping.
- pypistats.org recent and overall downloads.
- GitHub code search for geodatafusion in Rust, Python, Cargo.toml, pyproject/requirements.

Usage: uv run --with requests python fetch_h3a.py
"""

import io
import sys
import json
import tarfile

import common

CRATES = "https://crates.io/api/v1"
SRC = common.CACHE / "h3a_src"

GH_QUERIES = [
    "geodatafusion language:Rust",
    "geodatafusion language:Python",
    "geodatafusion filename:Cargo.toml",
    "geodatafusion filename:pyproject.toml",
    "geodatafusion filename:requirements.txt",
    "geodatafusion language:TOML",
    "geodatafusion language:Markdown",
    "geodatafusion language:\"Jupyter Notebook\"",
]


def crates_get(path: str, params: dict | None = None) -> dict:
    return common.cached_get("crates", f"{CRATES}{path}", params, interval=1.1)


def download_crate(name: str, version: str) -> None:
    dest = SRC / f"{name}-{version}"
    if dest.exists():
        return
    url = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
    r = common._session.get(url, timeout=120)
    r.raise_for_status()
    SRC.mkdir(parents=True, exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(r.content), mode="r:gz") as tf:
        tf.extractall(SRC, filter="data")


def main() -> None:
    meta = crates_get("/crates/geodatafusion")
    print("geodatafusion downloads:", meta["crate"]["downloads"], "recent:", meta["crate"]["recent_downloads"])
    rev = crates_get("/crates/geodatafusion/reverse_dependencies", {"per_page": 100})
    dependents = {}
    for v in rev["versions"]:
        dependents[v["crate"]] = v["num"]
    for dep in rev["dependencies"]:
        print("  dep edge:", dep["crate_id"], dep["req"], "features", dep["features"],
              "default_features", dep["default_features"], "optional", dep["optional"])
    for name, version in sorted(dependents.items()):
        info = crates_get(f"/crates/{name}")
        print(name, version, "downloads", info["crate"]["downloads"], "recent", info["crate"]["recent_downloads"],
              info["crate"]["repository"])
        download_crate(name, version)
    for path in ("/api/packages/geodatafusion/recent", "/api/packages/geodatafusion/overall"):
        d = common.cached_get("pypistats", f"https://pypistats.org{path}", interval=2)
        print(path, json.dumps(d)[:200])
    if "--no-gh" in sys.argv:
        return
    for q in GH_QUERIES:
        page = 1
        while True:
            d = common.gh_code_search(q, page=page)
            items = d.get("items", [])
            print(q, "page", page, "total", d.get("total_count"), "items", len(items))
            if page * 100 >= min(d.get("total_count", 0), 1000) or not items:
                break
            page += 1

    # Repositories listed on GitHub's dependency graph page (scraped by dependents.sh): find where
    # each one mentions geodatafusion, to separate direct from lockfile-only (transitive) use.
    deps = common.CACHE / "dependents" / "REPOSITORY.txt"
    if deps.exists():
        for repo in deps.read_text().split():
            d = common.gh_code_search(f"geodatafusion repo:{repo}")
            paths = sorted({it["path"] for it in d.get("items", [])})
            print("dependent", repo, d.get("total_count"), paths[:8])


if __name__ == "__main__":
    main()
