#!/usr/bin/env bash
# H3a: scrape GitHub's dependency-graph "Used by" pages for datafusion-contrib/geodatafusion.
# Writes HTML pages and REPOSITORY.txt / PACKAGE.txt under target/experiments/e6/dependents/.
set -euo pipefail
out="$(cd "$(dirname "$0")/../.." && pwd)/target/experiments/e6/dependents"
mkdir -p "$out"
cd "$out"
for t in REPOSITORY PACKAGE; do
  url="https://github.com/datafusion-contrib/geodatafusion/network/dependents?dependent_type=$t"
  i=1
  [ -f "${t}_1.html" ] || curl -sL -A "Mozilla/5.0 geodatafusion-e6" "$url" -o "${t}_1.html"
  f="${t}_1.html"
  while next=$(grep -o 'href="[^"]*dependents_after=[^"]*"' "$f" | tail -1 | sed 's/href="//;s/"$//;s/&amp;/\&/g'); [ -n "$next" ] && [ $i -lt 20 ]; do
    i=$((i + 1))
    [ -f "${t}_$i.html" ] || { sleep 2; curl -sL -A "Mozilla/5.0 geodatafusion-e6" "$next" -o "${t}_$i.html"; }
    f="${t}_$i.html"
  done
  cat "${t}"_*.html | grep -oE 'data-hovercard-type="repository"[^>]*href="/[^"]+"' \
    | sed 's/.*href="\///;s/"$//' | sort -u > "$t.txt"
  echo "$t: $(wc -l < "$t.txt") unique repositories listed"
done
