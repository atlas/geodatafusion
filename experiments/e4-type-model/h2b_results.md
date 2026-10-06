## Source expressions and their output types

| key | expression | native type | WKB-mode type |
|---|---|---|---|
| P | `ST_Centroid(wkb)` | Struct<x,y> [geoarrow.point] | Binary [geoarrow.wkb] |
| G | `ST_GeomFromText(ST_AsText(wkb))` | Union(geometry) [geoarrow.geometry] | Binary [geoarrow.wkb] |
| W | `wkb` | Binary [geoarrow.wkb] | Binary [geoarrow.wkb] |
| PS | `pt_sep` | Struct<x,y> [geoarrow.point] | Binary [geoarrow.wkb] |
| PI | `pt_il` | FixedSizeList[2] [geoarrow.point] | Binary [geoarrow.wkb] |
| PZ | `pt_z` | Struct<x,y,z> [geoarrow.point] | Binary [geoarrow.wkb] |
| W4326 | `wkb4326` | Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}] | Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}] |

## Same-source controls (a construct over one source with itself)

| source | construct | native | WKB mode |
|---|---|---|---|
| P | UNION ALL | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(1 2) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(1 2) |
| P | CASE | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| P | COALESCE | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| P | VALUES (2 rows) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| P | IN (subquery) | ok: Int64 [-] → 1; 2 | ok: Int64 [-] → 1; 2 |
| P | make_array | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(1 2) |
| P | array_agg (of UNION ALL) | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(1 2) |
| P | JOIN ON = | ok: Int64 [-], Int64 [-] → 1, 1; 1, 2; 2, 1; 2, 2 | ok: Int64 [-], Int64 [-] → 1, 1; 1, 2; 2, 1; 2, 2 |
| P | = (projection) | ok: Int64 [-], Boolean [-] → 1, true; 2, true | ok: Int64 [-], Boolean [-] → 1, true; 2, true |
| G | UNION ALL | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| G | CASE | **FAIL (exec)**: External error: Data not conforming to GeoArrow specification: Only FixedSizeList, Struct, Binary, LargeBinary, BinaryView, String, LargeString, and StringView arrays are unambigously typed for a GeoArrow type and can be... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| G | COALESCE | **FAIL (exec)**: External error: Data not conforming to GeoArrow specification: Only FixedSizeList, Struct, Binary, LargeBinary, BinaryView, String, LargeString, and StringView arrays are unambigously typed for a GeoArrow type and can be... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| G | VALUES (2 rows) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| G | IN (subquery) | ok: Int64 [-] → 1; 2 | ok: Int64 [-] → 1; 2 |
| G | make_array | **FAIL (exec)**: External error: Data not conforming to GeoArrow specification: Only FixedSizeList, Struct, Binary, LargeBinary, BinaryView, String, LargeString, and StringView arrays are unambigously typed for a GeoArrow type and can be... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) |
| G | array_agg (of UNION ALL) | **FAIL (exec)**: External error: Data not conforming to GeoArrow specification: Only FixedSizeList, Struct, Binary, LargeBinary, BinaryView, String, LargeString, and StringView arrays are unambigously typed for a GeoArrow type and can be... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| G | JOIN ON = | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 |
| G | = (projection) | ok: Int64 [-], Boolean [-] → 1, true; 2, true | ok: Int64 [-], Boolean [-] → 1, true; 2, true |
| W | UNION ALL | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| W | CASE | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| W | COALESCE | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| W | VALUES (2 rows) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| W | IN (subquery) | ok: Int64 [-] → 1; 2 | ok: Int64 [-] → 1; 2 |
| W | make_array | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) |
| W | array_agg (of UNION ALL) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| W | JOIN ON = | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 |
| W | = (projection) | ok: Int64 [-], Boolean [-] → 1, true; 2, true | ok: Int64 [-], Boolean [-] → 1, true; 2, true |
| W4326 | UNION ALL | ok: Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| W4326 | CASE | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| W4326 | COALESCE | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| W4326 | VALUES (2 rows) | ok: Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] → POINT(1 2); POINT(1 2) | ok: Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] → POINT(1 2); POINT(1 2) |
| W4326 | IN (subquery) | ok: Int64 [-] → 1; 2 | ok: Int64 [-] → 1; 2 |
| W4326 | make_array | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) |
| W4326 | array_agg (of UNION ALL) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| W4326 | JOIN ON = | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 |
| W4326 | = (projection) | ok: Int64 [-], Boolean [-] → 1, true; 2, true | ok: Int64 [-], Boolean [-] → 1, true; 2, true |

## Matrix

### P vs PS: control: two native XY separated points (centroid vs point column)

| construct | native | WKB mode |
|---|---|---|
| UNION ALL | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(3 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(3 4) |
| CASE | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(3 4) |
| COALESCE | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| VALUES (2 rows) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| IN (subquery) | ok: Int64 [-] → 1; 2 | ok: Int64 [-] → 1; 2 |
| make_array | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(3 4) |
| array_agg (of UNION ALL) | **FAIL (exec)**: External error: Invalid argument error: Extension type name missing | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(3 4) |
| JOIN ON = | ok: Int64 [-], Int64 [-] → 1, 1; 2, 1 | ok: Int64 [-], Int64 [-] → 1, 1; 2, 1 |
| = (projection) | ok: Int64 [-], Boolean [-] → 1, true; 2, false | ok: Int64 [-], Boolean [-] → 1, true; 2, false |

### P vs G: Point (ST_Centroid) vs Geometry (ST_GeomFromText)

| construct | native | WKB mode |
|---|---|---|
| UNION ALL | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type Union(Dense, 1: ("Point": Struct(... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4) |
| CASE | **FAIL (exec)**: type_coercion caused by Error during planning: Failed to coerce then (Struct("x": non-null Float64, "y": non-null Float64)) and else (Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| COALESCE | **FAIL (plan)**: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed with: Execution error: Fail to find the coerced type, errors: Execution error: Expect to get struct but got Union(Dense, 1: ("Point... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| VALUES (2 rows) | **FAIL (plan)**: Error during planning: Inconsistent metadata across values list at row 1 column 0. Was FieldMetadata { inner: {"ARROW:extension:name": "geoarrow.point"} } but found FieldMetadata { inner: {"ARROW:extension:name": "geoarr... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| IN (subquery) | **FAIL (exec)**: type_coercion caused by Error during planning: expr type Struct("x": non-null Float64, "y": non-null Float64) can't cast to Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("LineString... | ok: Int64 [-] → 1; 2 |
| make_array | **FAIL (plan)**: Error during planning: Execution error: Function 'make_array' user-defined coercion failed with: Error during planning: Failed to unify argument types of make_array: [Struct("x": non-null Float64, "y": non-null Float64),... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4) |
| array_agg (of UNION ALL) | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type Union(Dense, 1: ("Point": Struct(... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4) |
| JOIN ON = | **FAIL (exec)**: type_coercion caused by Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y"... | ok: Int64 [-], Int64 [-] → 1, 1; 2, 1 |
| = (projection) | **FAIL (plan)**: Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2:... | ok: Int64 [-], Boolean [-] → 1, true; 2, false |

### P vs W: native Point vs WKB column

| construct | native | WKB mode |
|---|---|---|
| UNION ALL | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type Binary on column 'g' | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4) |
| CASE | **FAIL (exec)**: type_coercion caused by Error during planning: Failed to coerce then (Struct("x": non-null Float64, "y": non-null Float64)) and else (Binary) to common types in CASE WHEN expression | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| COALESCE | **FAIL (plan)**: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed with: Execution error: Fail to find the coerced type, errors: Execution error: Expect to get struct but got Binary. No function mat... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| VALUES (2 rows) | **FAIL (plan)**: Error during planning: Inconsistent metadata across values list at row 1 column 0. Was FieldMetadata { inner: {"ARROW:extension:name": "geoarrow.point"} } but found FieldMetadata { inner: {"ARROW:extension:name": "geoarr... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| IN (subquery) | **FAIL (exec)**: type_coercion caused by Error during planning: expr type Struct("x": non-null Float64, "y": non-null Float64) can't cast to Binary in InSubquery | ok: Int64 [-] → 1; 2 |
| make_array | **FAIL (plan)**: Error during planning: Execution error: Function 'make_array' user-defined coercion failed with: Error during planning: Failed to unify argument types of make_array: [Struct("x": non-null Float64, "y": non-null Float64),... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4) |
| array_agg (of UNION ALL) | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type Binary on column 'g' | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4) |
| JOIN ON = | **FAIL (exec)**: type_coercion caused by Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = Binary | ok: Int64 [-], Int64 [-] → 1, 1; 2, 1 |
| = (projection) | **FAIL (plan)**: Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = Binary | ok: Int64 [-], Boolean [-] → 1, true; 2, false |

### G vs W: native Geometry vs WKB column

| construct | native | WKB mode |
|---|---|---|
| UNION ALL | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("LineString": List(non-null ... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| CASE | **FAIL (exec)**: type_coercion caused by Error during planning: Failed to coerce then (Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("LineString": List(non-null Struct("x": non-null Float64, "y": no... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| COALESCE | **FAIL (plan)**: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed with: Execution error: Fail to find the coerced type, errors: Execution error: Expect to get struct but got Union(Dense, 1: ("Point... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| VALUES (2 rows) | **FAIL (plan)**: Error during planning: Inconsistent metadata across values list at row 1 column 0. Was FieldMetadata { inner: {"ARROW:extension:name": "geoarrow.geometry"} } but found FieldMetadata { inner: {"ARROW:extension:name": "geo... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| IN (subquery) | **FAIL (exec)**: type_coercion caused by Error during planning: expr type Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("LineString": List(non-null Struct("x": non-null Float64, "y": non-null Float6... | ok: Int64 [-] → 1; 2 |
| make_array | **FAIL (plan)**: Error during planning: Execution error: Function 'make_array' user-defined coercion failed with: Error during planning: Failed to unify argument types of make_array: [Union(Dense, 1: ("Point": Struct("x": non-null Float6... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) |
| array_agg (of UNION ALL) | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("LineString": List(non-null ... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| JOIN ON = | **FAIL (exec)**: type_coercion caused by Error during planning: Cannot infer common argument type for comparison operation Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("LineString": List(non-null S... | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 |
| = (projection) | **FAIL (plan)**: Error during planning: Cannot infer common argument type for comparison operation Union(Dense, 1: ("Point": Struct("x": non-null Float64, "y": non-null Float64)), 2: ("LineString": List(non-null Struct("x": non-null Floa... | ok: Int64 [-], Boolean [-] → 1, true; 2, true |

### P vs PI: Point separated vs Point interleaved

| construct | native | WKB mode |
|---|---|---|
| UNION ALL | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type FixedSizeList(2 x non-null Float6... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(3 4) |
| CASE | **FAIL (exec)**: type_coercion caused by Error during planning: Failed to coerce then (Struct("x": non-null Float64, "y": non-null Float64)) and else (FixedSizeList(2 x non-null Float64, field: 'xy')) to common types in CASE WHEN express... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(3 4) |
| COALESCE | **FAIL (plan)**: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed with: Execution error: Fail to find the coerced type, errors: Execution error: Expect to get struct but got FixedSizeList(2 x non-n... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| VALUES (2 rows) | ok: Utf8 [-] → n/a: no constant constructor for this type | ok: Utf8 [-] → n/a: no constant constructor for this type |
| IN (subquery) | **FAIL (exec)**: type_coercion caused by Error during planning: expr type Struct("x": non-null Float64, "y": non-null Float64) can't cast to FixedSizeList(2 x non-null Float64, field: 'xy') in InSubquery | ok: Int64 [-] → 1; 2 |
| make_array | **FAIL (plan)**: Error during planning: Execution error: Function 'make_array' user-defined coercion failed with: Error during planning: Failed to unify argument types of make_array: [Struct("x": non-null Float64, "y": non-null Float64),... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(3 4) |
| array_agg (of UNION ALL) | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type FixedSizeList(2 x non-null Float6... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT(1 2); POINT(3 4) |
| JOIN ON = | **FAIL (exec)**: type_coercion caused by Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = FixedSizeList(2 x non-null Float64, field: 'xy') | ok: Int64 [-], Int64 [-] → 1, 1; 2, 1 |
| = (projection) | **FAIL (plan)**: Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = FixedSizeList(2 x non-null Float64, field: 'xy') | ok: Int64 [-], Boolean [-] → 1, true; 2, false |

### P vs PZ: Point XY vs Point XYZ

| construct | native | WKB mode |
|---|---|---|
| UNION ALL | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type Struct("x": non-null Float64, "y"... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT Z(1 2 3); POINT Z(3 4 5) |
| CASE | **FAIL (exec)**: type_coercion caused by Error during planning: Failed to coerce then (Struct("x": non-null Float64, "y": non-null Float64)) and else (Struct("x": non-null Float64, "y": non-null Float64, "z": non-null Float64)) to common... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT Z(3 4 5) |
| COALESCE | **FAIL (plan)**: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed with: Execution error: Fail to find the coerced type, errors: Execution error: Expect same keys for struct type but got mismatched ... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2) |
| VALUES (2 rows) | **FAIL (plan)**: Error during planning: Inconsistent data type across values list at row 1 column 0. Was Struct("x": non-null Float64, "y": non-null Float64) but found Struct("x": non-null Float64, "y": non-null Float64, "z": non-null Fl... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT Z(1 2 3) |
| IN (subquery) | **FAIL (exec)**: type_coercion caused by Error during planning: expr type Struct("x": non-null Float64, "y": non-null Float64) can't cast to Struct("x": non-null Float64, "y": non-null Float64, "z": non-null Float64) in InSubquery | ok: Int64 [-] →  |
| make_array | **FAIL (plan)**: Error during planning: Execution error: Function 'make_array' user-defined coercion failed with: Error during planning: Failed to unify argument types of make_array: [Struct("x": non-null Float64, "y": non-null Float64),... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT Z(1 2 3); POINT(1 2); POINT Z(3 4 5) |
| array_agg (of UNION ALL) | **FAIL (exec)**: type_coercion caused by Error during planning: Incompatible inputs for Union: Previous inputs were of type Struct("x": non-null Float64, "y": non-null Float64), but got incompatible type Struct("x": non-null Float64, "y"... | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); POINT Z(1 2 3); POINT Z(3 4 5) |
| JOIN ON = | **FAIL (exec)**: type_coercion caused by Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = Struct("x": non-null Float64, "y": non-null Float64, "z": n... | ok: Int64 [-], Int64 [-] →  |
| = (projection) | **FAIL (plan)**: Error during planning: Cannot infer common argument type for comparison operation Struct("x": non-null Float64, "y": non-null Float64) = Struct("x": non-null Float64, "y": non-null Float64, "z": non-null Float64) | ok: Int64 [-], Boolean [-] → 1, false; 2, false |

### W vs W4326: WKB no CRS vs WKB EPSG:4326 (same storage, different CRS)

| construct | native | WKB mode |
|---|---|---|
| UNION ALL | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| CASE | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| COALESCE | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4) |
| VALUES (2 rows) | **FAIL (plan)**: Error during planning: Inconsistent metadata across values list at row 1 column 0. Was FieldMetadata { inner: {"ARROW:extension:name": "geoarrow.wkb"} } but found FieldMetadata { inner: {"ARROW:extension:metadata": "{\"c... | **FAIL (plan)**: Error during planning: Inconsistent metadata across values list at row 1 column 0. Was FieldMetadata { inner: {"ARROW:extension:name": "geoarrow.wkb"} } but found FieldMetadata { inner: {"ARROW:extension:metadata": "{\"c... |
| IN (subquery) | ok: Int64 [-] → 1; 2 | ok: Int64 [-] → 1; 2 |
| make_array | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); POINT(1 2); LINESTRING(0 0,2 4); LINESTRING(0 0,2 4) |
| array_agg (of UNION ALL) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) | ok: Utf8 [geoarrow.wkt] → POINT(1 2); LINESTRING(0 0,2 4); POINT(1 2); LINESTRING(0 0,2 4) |
| JOIN ON = | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 | ok: Int64 [-], Int64 [-] → 1, 1; 2, 2 |
| = (projection) | ok: Int64 [-], Boolean [-] → 1, true; 2, true | ok: Int64 [-], Boolean [-] → 1, true; 2, true |

## Summary (FAIL = error; values are checked by hand in the report)

| pair | UNION ALL | CASE | COALESCE | VALUES (2 rows) | IN (subquery) | make_array | array_agg (of UNION ALL) | JOIN ON = | = (projection) |
|---|---|---|---|---|---|---|---|---|---|
| P-PS | ok / ok | FAIL / ok | FAIL / ok | ok / ok | ok / ok | FAIL / ok | FAIL / ok | ok / ok | ok / ok |
| P-G | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| P-W | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| G-W | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| P-PI | FAIL / ok | FAIL / ok | FAIL / ok | ok / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| P-PZ | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| W-W4326 | ok / ok | ok / ok | ok / ok | FAIL / FAIL | ok / ok | ok / ok | ok / ok | ok / ok | ok / ok |

## Queries used (native mode, pair P vs W)

- UNION ALL: `SELECT ST_AsText(g) FROM (SELECT ST_Centroid(wkb) AS g FROM t UNION ALL SELECT wkb AS g FROM t)`
- CASE: `SELECT ST_AsText(CASE WHEN id = 1 THEN ST_Centroid(wkb) ELSE wkb END) FROM t`
- COALESCE: `SELECT ST_AsText(COALESCE(ST_Centroid(wkb), wkb)) FROM t`
- VALUES (2 rows): `SELECT ST_AsText(column1) FROM (VALUES (ST_Centroid(ST_GeomFromText('LINESTRING(0 0,2 4)'))), (ST_AsBinary(ST_GeomFromText('POINT(1 2)'))))`
- IN (subquery): `SELECT t1.id FROM t AS t1 WHERE ST_Centroid(t1.wkb) IN (SELECT t2.wkb FROM t AS t2) ORDER BY t1.id`
- make_array: `SELECT ST_AsText(x) FROM (SELECT unnest(make_array(ST_Centroid(wkb), wkb)) AS x FROM t)`
- array_agg (of UNION ALL): `SELECT ST_AsText(x) FROM (SELECT unnest(arr) AS x FROM (SELECT array_agg(g) AS arr FROM (SELECT ST_Centroid(wkb) AS g FROM t UNION ALL SELECT wkb AS g FROM t)))`
- JOIN ON =: `SELECT t1.id, t2.id FROM t AS t1 JOIN t AS t2 ON ST_Centroid(t1.wkb) = t2.wkb ORDER BY 1, 2`
- = (projection): `SELECT id, ST_Centroid(wkb) = wkb FROM t ORDER BY id`

WKB mode wraps every source expression in `ST_AsBinary(..)`.
