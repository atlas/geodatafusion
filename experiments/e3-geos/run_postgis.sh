#!/usr/bin/env bash
# Runs the same (id, op) tasks as the GEOS runner through PostGIS (one psql session, temp objects only)
# and canonicalises the output with the runner's `render` mode.
# Usage: run_postgis.sh <corpus.tsv> <geos-results.tsv (task list)> <out.tsv> <runner binary>
set -euo pipefail
PG_URL="${PG_URL:-postgresql://postgres:postgres@localhost:54329/postgres}"
CORPUS="$(realpath "$1")"; TASKS_FROM="$(realpath "$2")"; OUT="$(realpath -m "$3")"; RUNNER="$4"
WORK="$(dirname "$OUT")"
tail -n +2 "$TASKS_FROM" | awk -F'\t' '{print NR "\t" $1 "\t" $2}' > "$WORK/pg_tasks.tsv"
psql "$PG_URL" -v ON_ERROR_STOP=1 -q <<SQL
CREATE TEMP TABLE e3_corpus(id int, kind text, param float8, a text, b text);
\copy e3_corpus FROM '$CORPUS' WITH (FORMAT csv, DELIMITER E'\t', HEADER true)
CREATE TEMP TABLE e3_tasks(ord int, id int, op text);
\copy e3_tasks FROM '$WORK/pg_tasks.tsv'
CREATE FUNCTION pg_temp.e3(op text, a geometry, b geometry, p float8) RETURNS text AS \$\$
DECLARE r geometry; t text;
BEGIN
  CASE op
    WHEN 'buffer' THEN r := ST_Buffer(a, p);
    WHEN 'buffer_neg' THEN r := ST_Buffer(a, -p);
    WHEN 'makevalid' THEN r := ST_MakeValid(a);
    WHEN 'simplifypt' THEN r := ST_SimplifyPreserveTopology(a, p);
    WHEN 'pointonsurface' THEN r := ST_PointOnSurface(a);
    WHEN 'convexhull' THEN r := ST_ConvexHull(a);
    WHEN 'linemerge' THEN r := ST_LineMerge(a);
    WHEN 'intersection' THEN r := ST_Intersection(a, b);
    WHEN 'union' THEN r := ST_Union(a, b);
    WHEN 'isvalidreason' THEN
      t := ST_IsValidReason(a);
      RETURN CASE WHEN t IS NULL THEN 'N' ELSE 'T:' || replace(replace(t, E'\t', ' '), E'\n', ' ') END;
  END CASE;
  IF r IS NULL THEN RETURN 'N'; END IF;
  RETURN 'G:' || encode(ST_AsEWKB(r, 'NDR'), 'hex');
EXCEPTION WHEN OTHERS THEN
  RETURN 'E:' || replace(replace(SQLERRM, E'\n', ' '), E'\t', ' ');
END \$\$ LANGUAGE plpgsql;
\copy (SELECT t.id, t.op, pg_temp.e3(t.op, ST_GeomFromWKB(decode(c.a, 'hex')), CASE WHEN c.b IS NOT NULL THEN ST_GeomFromWKB(decode(c.b, 'hex')) END, c.param) FROM e3_tasks t JOIN e3_corpus c USING (id) ORDER BY t.ord) TO '$WORK/pg_raw.tsv'
SQL
"$RUNNER" render "$WORK/pg_raw.tsv" "$OUT"
