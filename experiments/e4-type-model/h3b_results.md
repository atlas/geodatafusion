| query | tagged: result | tagged: output field(s) | untagged control: result | control: output field(s) |
|---|---|---|---|---|
| output of ST_AsText | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] |
| output of ST_AsBinary | ok → 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040 | Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040 | Binary [-] |
| || literal | ok → POINT(1 2) x; LINESTRING(0 0,2 4) x | Utf8 [-] | ok → POINT(1 2) x; LINESTRING(0 0,2 4) x | Utf8 [-] |
| literal || | ok → x POINT(1 2); x LINESTRING(0 0,2 4) | Utf8 [-] | ok → x POINT(1 2); x LINESTRING(0 0,2 4) | Utf8 [-] |
| || itself | ok → POINT(1 2)POINT(1 2); LINESTRING(0 0,2 4)LINESTRING(0 0,2 4) | Utf8 [-] | ok → POINT(1 2)POINT(1 2); LINESTRING(0 0,2 4)LINESTRING(0 0,2 4) | Utf8 [-] |
| concat | ok → POINT(1 2) x; LINESTRING(0 0,2 4) x | Utf8 [-] | ok → POINT(1 2) x; LINESTRING(0 0,2 4) x | Utf8 [-] |
| concat_ws | ok → POINT(1 2),x; LINESTRING(0 0,2 4),x | Utf8 [-] | ok → POINT(1 2),x; LINESTRING(0 0,2 4),x | Utf8 [-] |
| LIKE | ok → true; false | Boolean [-] | ok → true; false | Boolean [-] |
| ILIKE | ok → true; false | Boolean [-] | ok → true; false | Boolean [-] |
| ~ regex | ok → true; false | Boolean [-] | ok → true; false | Boolean [-] |
| length | ok → 10; 19 | Int32 [-] | ok → 10; 19 | Int32 [-] |
| char_length | ok → 10; 19 | Int32 [-] | ok → 10; 19 | Int32 [-] |
| upper | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] |
| lower | ok → point(1 2); linestring(0 0,2 4) | Utf8 [-] | ok → point(1 2); linestring(0 0,2 4) | Utf8 [-] |
| substr | ok → POINT; LINES | Utf8View [-] | ok → POINT; LINES | Utf8View [-] |
| replace | ok → P(1 2); LINESTRING(0 0,2 4) | Utf8 [-] | ok → P(1 2); LINESTRING(0 0,2 4) | Utf8 [-] |
| split_part | ok → POINT; LINESTRING | Utf8 [-] | ok → POINT; LINESTRING | Utf8 [-] |
| starts_with | ok → true; false | Boolean [-] | ok → true; false | Boolean [-] |
| trim | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] |
| md5(text) | ok → fb7c902e4b915a83a0792bd71289c968; 6f7ebd6c4b818eb82f99d7b3a75f206a | Utf8View [-] | ok → fb7c902e4b915a83a0792bd71289c968; 6f7ebd6c4b818eb82f99d7b3a75f206a | Utf8View [-] |
| md5(binary) | ok → 4ddc678d472071b63dd260ae7d7cd0eb; 6eb1ed2ccb778b33b142fb4f9e11d808 | Utf8View [-] | ok → 4ddc678d472071b63dd260ae7d7cd0eb; 6eb1ed2ccb778b33b142fb4f9e11d808 | Utf8View [-] |
| sha256(binary) | ok → 10a39f36ce6fbfdb4ad86e3d96a0fcd00d2fe4ee71e8c5f889ed2b70932e9d7a; b0410d7984de0f318ac4afdad1ca56e86ad0bd133749496036867a3526592b65 | Binary [-] | ok → 10a39f36ce6fbfdb4ad86e3d96a0fcd00d2fe4ee71e8c5f889ed2b70932e9d7a; b0410d7984de0f318ac4afdad1ca56e86ad0bd133749496036867a3526592b65 | Binary [-] |
| encode(binary,'hex') | ok → 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040 | Utf8 [-] | ok → 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040 | Utf8 [-] |
| encode(binary,'base64') | ok → AQEAAAAAAAAAAADwPwAAAAAAAABA; AQIAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAEEA | Utf8 [-] | ok → AQEAAAAAAAAAAADwPwAAAAAAAABA; AQIAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAEEA | Utf8 [-] |
| to_hex? (n/a) -> octet_length(binary) | **FAIL (plan)**: Error during planning: Function 'octet_length' requires String, but received Binary (DataType: Binary).  Hint: Binary types are not automatically coerced to String. Use CAST(column AS VARCHAR) to convert Binary data to S... |  | **FAIL (plan)**: Error during planning: Function 'octet_length' requires String, but received Binary (DataType: Binary).  Hint: Binary types are not automatically coerced to String. Use CAST(column AS VARCHAR) to convert Binary data to S... |  |
| length(binary) | **FAIL (exec)**: Arrow error: Invalid argument error: Encountered non UTF-8 data: invalid utf-8 sequence of 1 bytes from index 11 | Int32 [-] | **FAIL (exec)**: Arrow error: Invalid argument error: Encountered non UTF-8 data: invalid utf-8 sequence of 1 bytes from index 11 | Int32 [-] |
| substr(binary) | **FAIL (plan)**: Error during planning: Internal error: Function 'substr' failed to match any signature, errors: Error during planning: Function 'substr' expects 2 arguments but received 3,Error during planning: Function 'substr' require... |  | **FAIL (plan)**: Error during planning: Internal error: Function 'substr' failed to match any signature, errors: Error during planning: Function 'substr' expects 2 arguments but received 3,Error during planning: Function 'substr' require... |  |
| CAST binary AS BYTEA | ok → 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040 | Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040 | Binary [-] |
| UNION ALL binary with literal | ok → 00; 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040 | Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → 0101000000000000000000f03f0000000000000040; 0102000000020000000000000000000000000000000000000000000000000000400000000000001040; 00 | Binary [-] |
| = text literal | ok → true; false | Boolean [-] | ok → true; false | Boolean [-] |
| = ST_AsText(other col) | ok → true; true | Boolean [-] | ok → true; true | Boolean [-] |
| < text | ok → true; true | Boolean [-] | ok → true; true | Boolean [-] |
| = binary | ok → true; true | Boolean [-] | ok → true; true | Boolean [-] |
| IN (list) | ok → true; false | Boolean [-] | ok → true; false | Boolean [-] |
| CASE text/literal | ok → POINT(1 2); none | Utf8 [-] | ok → POINT(1 2); none | Utf8 [-] |
| COALESCE text/literal | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] |
| UNION ALL with literal | ok → abc; POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → POINT(1 2); LINESTRING(0 0,2 4); abc | Utf8 [-] |
| GROUP BY text | ok → LINESTRING(0 0,2 4), 1; POINT(1 2), 1 | Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}], Int64 [-] | ok → LINESTRING(0 0,2 4), 1; POINT(1 2), 1 | Utf8 [-], Int64 [-] |
| GROUP BY binary | ok → 0102000000020000000000000000000000000000000000000000000000000000400000000000001040, 1; 0101000000000000000000f03f0000000000000040, 1 | Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}], Int64 [-] | ok → 0102000000020000000000000000000000000000000000000000000000000000400000000000001040, 1; 0101000000000000000000f03f0000000000000040, 1 | Binary [-], Int64 [-] |
| DISTINCT text | ok → LINESTRING(0 0,2 4); POINT(1 2) | Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → LINESTRING(0 0,2 4); POINT(1 2) | Utf8 [-] |
| ORDER BY text | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] |
| string_agg | ok → POINT(1 2);LINESTRING(0 0,2 4) | LargeUtf8 [-] | ok → POINT(1 2);LINESTRING(0 0,2 4) | LargeUtf8 [-] |
| min/max text | ok → LINESTRING(0 0,2 4), POINT(1 2) | Utf8 [-], Utf8 [-] | ok → LINESTRING(0 0,2 4), POINT(1 2) | Utf8 [-], Utf8 [-] |
| array_agg text | ok → [POINT(1 2), LINESTRING(0 0,2 4)] | List<Utf8 [-]> [-] | ok → [POINT(1 2), LINESTRING(0 0,2 4)] | List<Utf8 [-]> [-] |
| CAST AS VARCHAR | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8View [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8View [-] |
| arrow_cast Utf8 | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [-] |
| arrow_cast LargeUtf8 | ok → POINT(1 2); LINESTRING(0 0,2 4) | LargeUtf8 [-] | ok → POINT(1 2); LINESTRING(0 0,2 4) | LargeUtf8 [-] |
| CAST binary AS VARCHAR | **FAIL (exec)**: Arrow error: Invalid argument error: Encountered non UTF-8 data: invalid utf-8 sequence of 1 bytes from index 11 | Utf8View [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}] | **FAIL (exec)**: Arrow error: Invalid argument error: Encountered non UTF-8 data: invalid utf-8 sequence of 1 bytes from index 11 | Utf8View [-] |
| ST_AsText(ST_AsText(g)) (chaining) | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [geoarrow.wkt] |
| ST_GeomFromText(ST_AsText(g)) | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}] | ok → POINT(1 2); LINESTRING(0 0,2 4) | Utf8 [geoarrow.wkt] |
| ST_AsText('garbage' || ...) into geometry fn | **FAIL (exec)**: External error: WKT error: Invalid type encountered | Float64 [-] | **FAIL (exec)**: External error: WKT error: Invalid type encountered | Float64 [-] |
| join on text | ok → 2 | Int64 [-] | ok → 2 | Int64 [-] |

COPY TO parquet: ok=true 2
COPY union-with-literal TO parquet: ok=true 3
COPY CAST(ST_AsBinary AS VARCHAR) (0 rows) TO parquet: ok=true 0
logical schema: id -> Int64 [-]
logical schema: wkt -> Utf8 [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}]
logical schema: wkb -> Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}]
logical schema: wkt_junk -> Utf8 [-]
logical schema: wkt_upper -> Utf8 [-]
logical schema: centroid -> Struct<x,y> [geoarrow.point {"crs":"EPSG:4326","crs_type":"authority_code"}]
DataFrame::write_parquet: Ok(())
read back (DataFusion) fields: 
ST_AsText(wkt_junk) after read-back: ok=true POINT(1 2) junk; LINESTRING(0 0,2 4) junk
