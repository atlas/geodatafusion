## geoarrow-array 0.8.0 minimal reproductions

| input | GeometryBuilder (native) | WkbBuilder |
|---|---|---|
| `GEOMETRYCOLLECTION(POINT(1 2))` | CHANGED -> POINT(1 2) | same |
| `GEOMETRYCOLLECTION(LINESTRING(0 0,1 1))` | CHANGED -> LINESTRING(0 0,1 1) | same |
| `GEOMETRYCOLLECTION(GEOMETRYCOLLECTION(POINT(1 2)))` | CHANGED -> POINT(1 2) | same |
| `GEOMETRYCOLLECTION(POINT(1 2),POINT(3 4))` | same | same |
| `GEOMETRYCOLLECTION Z(POINT Z(1 2 3))` | CHANGED -> POINT Z(1 2 3) | same |
| `GEOMETRYCOLLECTION EMPTY` | same | same |
| `MULTIPOLYGON(EMPTY,((0 0,1 0,1 1,0 0)))` | PANIC called `Option::unwrap()` on a `None` value | same |
| `MULTIPOINT(EMPTY,(1 2))` | wkt parse error: Missing closing parenthesis for type | |
| `POINT EMPTY` | same | same |
| WKB `GEOMETRYCOLLECTION(POINT Z(1 2 3), POINT(4 5))` (XY GC, Z member) | PANIC called `Result::unwrap()` on an `Err` value: IncorrectGeometryType("coord dimension must be XY for this buffer; got Xyz.") | n/a |
| WKB `GEOMETRYCOLLECTION Z(POINT Z(1 2 3), POINT(4 5))` (Z GC, XY member) | PANIC called `Result::unwrap()` on an `Err` value: IncorrectGeometryType("coord dimension must be XYZ for this buffer; got Xy.") | n/a |

## End to end through geodatafusion (DataFusion 54)

- `SELECT ST_AsText(ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 2))'))` → ok → POINT(1 2)
- `SELECT ST_GeometryType(ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 2))'))` → ok → ST_Point
- `SELECT ST_AsText(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('GEOMETRYCOLLECTION(LINESTRING(0 0,1 1))'))))` → ok → LINESTRING(0 0,1 1)
- `SELECT ST_AsText(ST_GeomFromText('MULTIPOLYGON(EMPTY,((0 0,1 0,1 1,0 0)))'))` → PANIC (task): called `Option::unwrap()` on a `None` value
- `SELECT ST_AsText(ST_GeomFromWKB(decode('01070000000200000001e9030000000000000000f03f00000000000000400000000000000840010100000000000000000010400000000000001440', 'hex')))` → PANIC (task): called `Result::unwrap()` on an `Err` value: IncorrectGeometryType("coord dimension must be XY for this buffer; got Xyz.")
- `SELECT ST_AsText(decode('01070000000200000001e9030000000000000000f03f00000000000000400000000000000840010100000000000000000010400000000000001440', 'hex'))` → ok → GEOMETRYCOLLECTION(POINT Z(1 2 3),POINT(4 5))
