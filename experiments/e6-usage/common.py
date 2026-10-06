"""Shared helpers for experiment E6: cached HTTP calls and the inventory parser.

Every raw API response is cached under target/experiments/e6/ keyed by a hash of the request,
so the analysis scripts can be rerun without re-querying.
"""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
import time
from pathlib import Path

import requests

REPO = Path(__file__).resolve().parents[2]
CACHE = REPO / "target" / "experiments" / "e6"
OUT = Path(__file__).resolve().parent
INVENTORY = REPO / "plans" / "inventory.md"
UA = "geodatafusion-e6-usage-experiment (mikkel@starvik.no)"

_session = requests.Session()
_session.headers["User-Agent"] = UA
_last_call: dict[str, float] = {}


def _cache_path(kind: str, key: str) -> Path:
    h = hashlib.sha256(key.encode()).hexdigest()[:24]
    p = CACHE / kind / f"{h}.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    return p


def gh_token() -> str:
    return subprocess.run(["gh", "auth", "token"], capture_output=True, text=True, check=True).stdout.strip()


def _pace(bucket: str, interval: float) -> None:
    last = _last_call.get(bucket, 0.0)
    wait = interval - (time.time() - last)
    if wait > 0:
        time.sleep(wait)
    _last_call[bucket] = time.time()


def cached_get(kind: str, url: str, params: dict | None = None, headers: dict | None = None,
               interval: float = 0.0, max_tries: int = 8) -> dict:
    """GET a JSON URL with caching, pacing and backoff on 403/429/5xx."""
    key = json.dumps([url, params or {}, headers or {}], sort_keys=True)
    path = _cache_path(kind, key)
    if path.exists():
        return json.loads(path.read_text())["response"]
    for attempt in range(max_tries):
        _pace(kind, interval)
        r = _session.get(url, params=params, headers=headers or {}, timeout=60)
        if r.status_code == 200:
            data = r.json()
            path.write_text(json.dumps({"request": {"url": url, "params": params},
                                        "fetched": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                                        "response": data}))
            return data
        if r.status_code in (403, 429) or r.status_code >= 500:
            retry = r.headers.get("Retry-After")
            reset = r.headers.get("X-RateLimit-Reset")
            if retry:
                wait = int(retry) + 1
            elif reset and r.headers.get("X-RateLimit-Remaining") == "0":
                wait = max(int(reset) - time.time(), 0) + 2
            else:
                wait = min(60 * (attempt + 1), 300)
            print(f"  {r.status_code} on {kind}; sleeping {wait:.0f}s ({r.text[:120]!r})", file=sys.stderr)
            time.sleep(wait)
            continue
        if r.status_code == 422:
            data = {"error": 422, "message": r.text[:500]}
            path.write_text(json.dumps({"request": {"url": url, "params": params}, "response": data}))
            return data
        r.raise_for_status()
    raise RuntimeError(f"giving up on {url} {params}")


_GH_HEADERS: dict | None = None


def gh_headers() -> dict:
    global _GH_HEADERS
    if _GH_HEADERS is None:
        _GH_HEADERS = {"Authorization": f"Bearer {gh_token()}",
                       "Accept": "application/vnd.github.text-match+json",
                       "X-GitHub-Api-Version": "2022-11-28"}
    return _GH_HEADERS


def gh_code_search(q: str, page: int = 1, per_page: int = 100) -> dict:
    """GitHub REST code search (legacy index), 10 requests/minute -> one every 6.5 s."""
    key_headers = {"Accept": "application/vnd.github.text-match+json"}
    keyurl = "https://api.github.com/search/code"
    params = {"q": q, "per_page": per_page, "page": page}
    key = json.dumps([keyurl, params, key_headers], sort_keys=True)
    path = _cache_path("ghcode", key)
    if path.exists():
        return json.loads(path.read_text())["response"]
    data = cached_get("ghcode_live", keyurl, params, gh_headers(), interval=6.5)
    # Store under a token-free key so cache lookups don't depend on the token.
    path.write_text(json.dumps({"request": {"url": keyurl, "params": params},
                                "fetched": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                                "response": data}))
    return data


def gh_api(url: str, params: dict | None = None) -> dict:
    return cached_get("ghapi", url, params, {"Authorization": f"Bearer {gh_token()}"}, interval=0.2)


def raw_file(repo: str, sha_or_ref: str, path: str) -> str | None:
    """Fetch a file's raw text via raw.githubusercontent.com, cached."""
    url = f"https://raw.githubusercontent.com/{repo}/{sha_or_ref}/{path}"
    p = _cache_path("raw", url).with_suffix(".txt")
    if p.exists():
        return p.read_text(errors="replace")
    for attempt in range(4):
        _pace("raw", 0.1)
        r = _session.get(url, timeout=60)
        if r.status_code == 200:
            p.write_text(r.text)
            return r.text
        if r.status_code == 404:
            p.write_text("")
            return ""
        time.sleep(5 * (attempt + 1))
    return None


def inventory() -> list[dict]:
    rows = []
    for line in INVENTORY.read_text().splitlines():
        m = re.match(r"^\| (G[0-9]+(?:\+G[0-9]+)?|—) \| (\w+) \| (.+) \| (yes)? ?\| ([^|]+) \| (\w+) \|$", line)
        if m:
            rows.append({"group": m.group(1), "chapter": m.group(2), "name": m.group(3).strip(),
                         "implemented": m.group(4) == "yes", "doc_tests": m.group(5).strip(),
                         "file": m.group(6)})
    return rows


def searchable(row: dict) -> bool:
    """Operators and bare type names can't be searched meaningfully in code search."""
    return row["chapter"] not in ("operator", "type")
