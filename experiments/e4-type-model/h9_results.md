
## GeoPandas (to_parquet / to_arrow)

- to_parquet EPSG:4326 encoding=WKB: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84' id=EPSG:4326
- to_parquet EPSG:4326 encoding=geoarrow: geo v1.1.0 encoding=point crs PROJJSON name='WGS 84' id=EPSG:4326
- to_arrow EPSG:4326 encoding=WKB: geometry: geoarrow.wkb crs_type=None crs=PROJJSON name='WGS 84' id=EPSG:4326
- to_arrow EPSG:4326 encoding=geoarrow: geometry: geoarrow.point crs_type=None crs=PROJJSON name='WGS 84' id=EPSG:4326
- to_parquet OGC:CRS84 encoding=WKB: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- to_parquet OGC:CRS84 encoding=geoarrow: geo v1.1.0 encoding=point crs PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- to_arrow OGC:CRS84 encoding=WKB: geometry: geoarrow.wkb crs_type=None crs=PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- to_arrow OGC:CRS84 encoding=geoarrow: geometry: geoarrow.point crs_type=None crs=PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- to_parquet EPSG:3857 encoding=WKB: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857
- to_parquet EPSG:3857 encoding=geoarrow: geo v1.1.0 encoding=point crs PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857
- to_arrow EPSG:3857 encoding=WKB: geometry: geoarrow.wkb crs_type=None crs=PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857
- to_arrow EPSG:3857 encoding=geoarrow: geometry: geoarrow.point crs_type=None crs=PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857
- to_parquet EPSG:2263 encoding=WKB: geo v1.1.0 encoding=WKB crs PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263
- to_parquet EPSG:2263 encoding=geoarrow: geo v1.1.0 encoding=point crs PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263
- to_arrow EPSG:2263 encoding=WKB: geometry: geoarrow.wkb crs_type=None crs=PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263
- to_arrow EPSG:2263 encoding=geoarrow: geometry: geoarrow.point crs_type=None crs=PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263

## GDAL ogr2ogr (system GDAL)

GDAL 3.13.3 "Iowa City", released 2026/08/13
- Parquet EPSG:4326 encoding=WKB: geo v1.1.0 encoding=WKB crs KEY OMITTED (= OGC:CRS84 by spec); arrow field: no extension field
- Parquet EPSG:4326 encoding=GEOARROW: geo v1.1.0 encoding=point crs KEY OMITTED (= OGC:CRS84 by spec); arrow field: no extension field
- Arrow IPC EPSG:4326: field: geom: geoarrow.point crs_type=None crs=None; schema-level geo metadata crs: WKT2 string 'GEOGCRS["WGS 84",ENSEMBLE["World Geodeti'... ID ID["EPSG",4326]]
- Parquet OGC:CRS84 encoding=WKB: geo v1.1.0 encoding=WKB crs KEY OMITTED (= OGC:CRS84 by spec); arrow field: no extension field
- Parquet OGC:CRS84 encoding=GEOARROW: geo v1.1.0 encoding=point crs KEY OMITTED (= OGC:CRS84 by spec); arrow field: no extension field
- Arrow IPC OGC:CRS84: field: geom: geoarrow.point crs_type=None crs=None; schema-level geo metadata crs: WKT2 string 'GEOGCRS["WGS 84 (CRS84)",DATUM["World Ge'... ID ID["OGC","CRS84"]]
- Parquet EPSG:3857 encoding=WKB: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857; arrow field: no extension field
- Parquet EPSG:3857 encoding=GEOARROW: geo v1.1.0 encoding=point crs PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857; arrow field: no extension field
- Arrow IPC EPSG:3857: field: geom: geoarrow.point crs_type=None crs=None; schema-level geo metadata crs: WKT2 string 'PROJCRS["WGS 84 / Pseudo-Mercator",BASEG'... ID ID["EPSG",3857]]
- Parquet EPSG:2263 encoding=WKB: geo v1.1.0 encoding=WKB crs PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263; arrow field: no extension field
- Parquet EPSG:2263 encoding=GEOARROW: geo v1.1.0 encoding=point crs PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263; arrow field: no extension field
- Arrow IPC EPSG:2263: field: geom: geoarrow.point crs_type=None crs=None; schema-level geo metadata crs: WKT2 string 'PROJCRS["NAD83 / New York Long Island (f'... ID ID["EPSG",2263]]

## pyogrio read_arrow (GDAL ArrowStream export, bundled GDAL)

pyogrio 0.13.0 GDAL 3.12.4
- read_arrow EPSG:4326: meta crs='EPSG:4326'; field: geom: geoarrow.wkb crs_type=None crs=PROJJSON name='WGS 84' id=EPSG:4326
- read_arrow OGC:CRS84: meta crs='GEOGCS["WGS 84 (CRS84)",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AXIS["Longitude",EAST],AXIS["Latitude",NORTH],AUTHORITY["OGC","CRS84"]]'; field: geom: geoarrow.wkb crs_type=None crs=PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- read_arrow EPSG:3857: meta crs='EPSG:3857'; field: geom: geoarrow.wkb crs_type=None crs=PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857
- read_arrow EPSG:2263: meta crs='EPSG:2263'; field: geom: geoarrow.wkb crs_type=None crs=PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263

## DuckDB spatial

duckdb 1.5.6
- `SELECT ST_SetCRS(ST_Point(10, 59), 'EPSG:4326') AS geometry`: type GEOMETRY('EPSG:4326'); COPY parquet: geo v1.0.0 encoding=WKB crs PROJJSON name='WGS 84' id=EPSG:4326; arrow field: no extension field; .arrow(): geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84' id=EPSG:4326
- `SELECT ST_SetCRS(ST_Point(10, 59), 'OGC:CRS84') AS geometry`: type GEOMETRY('OGC:CRS84'); COPY parquet: geo v1.0.0 encoding=WKB crs KEY OMITTED (= OGC:CRS84 by spec); arrow field: no extension field; .arrow(): geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- `SELECT ST_SetCRS(ST_Point(10, 59), 'EPSG:3857') AS geometry`: type GEOMETRY('EPSG:3857'); COPY parquet: geo v1.0.0 encoding=WKB crs PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857; arrow field: no extension field; .arrow(): geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857
- `SELECT ST_SetCRS(ST_Point(10, 59), 'EPSG:2263') AS geometry`: type GEOMETRY('EPSG:2263'); COPY parquet: geo v1.0.0 encoding=WKB crs PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263; arrow field: no extension field; .arrow(): geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263
- DuckDB reads geopandas_EPSG_4326_WKB.parquet: GEOMETRY('EPSG:4326')
- DuckDB reads geopandas_OGC_CRS84_WKB.parquet: GEOMETRY('OGC:CRS84')
- DuckDB reads geopandas_EPSG_3857_WKB.parquet: GEOMETRY('EPSG:3857')

## geoarrow-pyarrow

- with_crs(str 'EPSG:4326'): {"crs": "EPSG:4326"}
- with_crs(pyproj.CRS('EPSG:4326')): PROJJSON name='WGS 84' id=EPSG:4326 crs_type=projjson
- with_crs(str 'OGC:CRS84'): {"crs": "OGC:CRS84"}
- with_crs(pyproj.CRS('OGC:CRS84')): PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84 crs_type=projjson
- with_crs(str 'EPSG:3857'): {"crs": "EPSG:3857"}
- with_crs(pyproj.CRS('EPSG:3857')): PROJJSON name='WGS 84 / Pseudo-Mercator' id=EPSG:3857 crs_type=projjson
- with_crs(str 'EPSG:2263'): {"crs": "EPSG:2263"}
- with_crs(pyproj.CRS('EPSG:2263')): PROJJSON name='NAD83 / New York Long Island (ftUS)' id=EPSG:2263 crs_type=projjson

## Round trips of a geoarrow.wkb column with a given CRS tag

### authority_code EPSG:4326

- gpd.from_arrow crs: EPSG:4326
- gpd.to_arrow back: geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84' id=EPSG:4326
- gpd.to_parquet geo: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84' id=EPSG:4326
- gpd.read_parquet crs: EPSG:4326 (to_epsg=4326)
- gpd.read_parquet(raw, no geo md): FAIL ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
- duckdb.from_arrow type: GEOMETRY('EPSG:4326')
- duckdb .arrow() back: {'ARROW:extension:metadata': '{"crs_type":"projjson","crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","type":"GeographicCRS","name":"WGS 84","datum_ensemble":{"name":"World Geodetic System 1984 ensemble","members":[{"name":"World Geodetic System 1984 (Transit)","id":{"authority"
- duckdb COPY parquet geo crs: PROJJSON name='WGS 84' id=EPSG:4326
- duckdb read_parquet(raw) type: BLOB
- ogrinfo raw parquet: geometry recognised, SRS (unknown)
- ogrinfo arrow ipc: SRS ID["EPSG",4326]]
- ogr2ogr ipc->parquet geo: geo v1.1.0 encoding=WKB crs KEY OMITTED (= OGC:CRS84 by spec)

### authority_code OGC:CRS84

- gpd.from_arrow crs: OGC:CRS84
- gpd.to_arrow back: geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- gpd.to_parquet geo: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84 (CRS84)' id=OGC:CRS84
- gpd.read_parquet crs: OGC:CRS84 (to_epsg=None)
- gpd.read_parquet(raw, no geo md): FAIL ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
- duckdb.from_arrow type: GEOMETRY('OGC:CRS84')
- duckdb .arrow() back: {'ARROW:extension:metadata': '{"crs_type":"projjson","crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","type":"GeographicCRS","name":"WGS 84 (CRS84)","datum_ensemble":{"name":"World Geodetic System 1984 ensemble","members":[{"name":"World Geodetic System 1984 (Transit)","id":{"au
- duckdb COPY parquet geo crs: "KEY OMITTED"
- duckdb read_parquet(raw) type: BLOB
- ogrinfo raw parquet: geometry recognised, SRS (unknown)
- ogrinfo arrow ipc: SRS ID["EPSG",4326]]
- ogr2ogr ipc->parquet geo: geo v1.1.0 encoding=WKB crs KEY OMITTED (= OGC:CRS84 by spec)

### projjson EPSG:4326

- gpd.from_arrow crs: EPSG:4326
- gpd.to_arrow back: geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84' id=EPSG:4326
- gpd.to_parquet geo: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84' id=EPSG:4326
- gpd.read_parquet crs: EPSG:4326 (to_epsg=4326)
- gpd.read_parquet(raw, no geo md): FAIL ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
- duckdb.from_arrow type: GEOMETRY('EPSG:4326')
- duckdb .arrow() back: {'ARROW:extension:metadata': '{"crs_type":"projjson","crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","type":"GeographicCRS","name":"WGS 84","datum_ensemble":{"name":"World Geodetic System 1984 ensemble","members":[{"name":"World Geodetic System 1984 (Transit)","id":{"authority"
- duckdb COPY parquet geo crs: PROJJSON name='WGS 84' id=EPSG:4326
- duckdb read_parquet(raw) type: BLOB
- ogrinfo raw parquet: geometry recognised, SRS (unknown)
- ogrinfo arrow ipc: SRS ID["EPSG",4326]]
- ogr2ogr ipc->parquet geo: geo v1.1.0 encoding=WKB crs KEY OMITTED (= OGC:CRS84 by spec)

### srid 4326

- gpd.from_arrow crs: EPSG:4326
- gpd.to_arrow back: geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84' id=EPSG:4326
- gpd.to_parquet geo: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84' id=EPSG:4326
- gpd.read_parquet crs: EPSG:4326 (to_epsg=4326)
- gpd.read_parquet(raw, no geo md): FAIL ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
- duckdb.from_arrow type: GEOMETRY
- duckdb .arrow() back: {'ARROW:extension:metadata': '{}', 'ARROW:extension:name': 'geoarrow.wkb'}
- duckdb COPY parquet geo crs: "KEY OMITTED"
- duckdb read_parquet(raw) type: BLOB
- ogrinfo raw parquet: geometry recognised, SRS (unknown)
- ogrinfo arrow ipc: geometry recognised, SRS (unknown)
- ogr2ogr ipc->parquet geo: geo v1.1.0 encoding=WKB crs None

### no crs_type, EPSG:4326

- gpd.from_arrow crs: EPSG:4326
- gpd.to_arrow back: geometry: geoarrow.wkb crs_type=projjson crs=PROJJSON name='WGS 84' id=EPSG:4326
- gpd.to_parquet geo: geo v1.1.0 encoding=WKB crs PROJJSON name='WGS 84' id=EPSG:4326
- gpd.read_parquet crs: EPSG:4326 (to_epsg=4326)
- gpd.read_parquet(raw, no geo md): FAIL ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
- duckdb.from_arrow type: GEOMETRY('EPSG:4326')
- duckdb .arrow() back: {'ARROW:extension:metadata': '{"crs_type":"projjson","crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","type":"GeographicCRS","name":"WGS 84","datum_ensemble":{"name":"World Geodetic System 1984 ensemble","members":[{"name":"World Geodetic System 1984 (Transit)","id":{"authority"
- duckdb COPY parquet geo crs: PROJJSON name='WGS 84' id=EPSG:4326
- duckdb read_parquet(raw) type: BLOB
- ogrinfo raw parquet: geometry recognised, SRS (unknown)
- ogrinfo arrow ipc: SRS ID["EPSG",4326]]
- ogr2ogr ipc->parquet geo: geo v1.1.0 encoding=WKB crs KEY OMITTED (= OGC:CRS84 by spec)

