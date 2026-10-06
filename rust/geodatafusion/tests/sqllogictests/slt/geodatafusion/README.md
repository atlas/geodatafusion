# Hand-written parity tests

Put hand-written `.slt` files here, one per function (`st_area.slt`, ...). Use them for edge
cases the PostGIS docs don't cover: empty geometries, NULLs, Z/M dimensions, SRIDs, geometry
collections, invalid input, error cases.

Write only the queries, without expected results, and let PostGIS fill those in:

```sql
query T
SELECT ST_Area('POLYGON EMPTY'::geometry)
```

```bash
dev/postgis.sh start
cargo slt --complete geodatafusion/st_area
```

Unlike `../postgis_docs/`, these files are never regenerated, so they're safe to edit. Never
type expected output by hand. It must always come from PostGIS.
