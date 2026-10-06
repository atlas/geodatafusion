## geoarrow-array 0.9.0 minimal reproductions

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

Doc literals parsed: 1158; native round trip changed: 9 ["st_clusterintersecting.slt:8", "st_clusterwithin.slt:8", "st_collectionextract.slt:15", "st_collectionextract.slt:23", "st_collectionhomogenize.slt:8", "st_collectionhomogenize.slt:14", "st_collectionhomogenize.slt:26", "st_force_collection.slt:8", "st_split.slt:25"]; WKB changed: 0
