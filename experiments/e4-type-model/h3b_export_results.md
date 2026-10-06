## pyarrow without geoarrow-pyarrow

- astext_asbinary.parquet: id: int64 [-]; wkt: string [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}]; wkb: binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}]; wkt_junk: string [-]; wkt_upper: string [-]; centroid: struct<x: double not null, y: double not null> [geoarrow.point {"crs":"EPSG:4326","crs_type":"authority_code"}]
  schema metadata keys: []
  pc.utf8_upper(wkt): ['POINT(1 2)', 'LINESTRING(0 0,2 4)']
- astext_asbinary_df.parquet: id: int64 [-]; wkt: string [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}]; wkb: binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}]; wkt_junk: string [-]; wkt_upper: string [-]; centroid: struct<x: double not null, y: double not null> [geoarrow.point {"crs":"EPSG:4326","crs_type":"authority_code"}]
  schema metadata keys: []
  pc.utf8_upper(wkt): ['POINT(1 2)', 'LINESTRING(0 0,2 4)']
- cast_varchar.parquet: s: string_view [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"authority_code"}]
  schema metadata keys: []
- union_literal.parquet: s: string [geoarrow.wkt {"crs":"EPSG:4326","crs_type":"authority_code"}]
  schema metadata keys: []

## pyarrow with geoarrow-pyarrow imported (extension types registered)

- astext_asbinary.parquet: read ok; types: id: int64; wkt: extension<geoarrow.wkt<WktType>>; wkb: extension<geoarrow.wkb<WkbType>>; wkt_junk: string; wkt_upper: string; centroid: extension<geoarrow.point<PointType>>
  wkt: ga.as_wkt -> ['POINT (1 2)', 'LINESTRING (0 0, 2 4)']
  wkt: pc.utf8_upper FAIL: ArrowNotImplementedError: Function 'utf8_upper' has no kernel matching input types (extension<geoarrow.wkt<WktType>>)
  wkt: pc.utf8_length FAIL: ArrowNotImplementedError: Function 'utf8_length' has no kernel matching input types (extension<geoarrow.wkt<WktType>>)
  wkt: to_pandas dtype -> extension<geoarrow.wkt<WktType>>[pyarrow]
  wkb: ga.as_wkt -> ['POINT (1 2)', 'LINESTRING (0 0, 2 4)']
  centroid: ga.as_wkt -> ['POINT (1 2)', 'POINT (1 2)']
- astext_asbinary_df.parquet: read ok; types: id: int64; wkt: extension<geoarrow.wkt<WktType>>; wkb: extension<geoarrow.wkb<WkbType>>; wkt_junk: string; wkt_upper: string; centroid: extension<geoarrow.point<PointType>>
  wkt: ga.as_wkt -> ['POINT (1 2)', 'LINESTRING (0 0, 2 4)']
  wkt: pc.utf8_upper FAIL: ArrowNotImplementedError: Function 'utf8_upper' has no kernel matching input types (extension<geoarrow.wkt<WktType>>)
  wkt: pc.utf8_length FAIL: ArrowNotImplementedError: Function 'utf8_length' has no kernel matching input types (extension<geoarrow.wkt<WktType>>)
  wkt: to_pandas dtype -> extension<geoarrow.wkt<WktType>>[pyarrow]
  wkb: ga.as_wkt -> ['POINT (1 2)', 'LINESTRING (0 0, 2 4)']
  centroid: ga.as_wkt -> ['POINT (1 2)', 'POINT (1 2)']
- cast_varchar.parquet: read FAIL: ValueError: Can't interpret string_view as geoarrow.wkb
- union_literal.parquet: read ok; types: s: extension<geoarrow.wkt<WktType>>
  s: ga.as_wkt FAIL: GeoArrowCException: GeoArrowKernel<as_geoarrow>::push_batch() failed (22): Expected geometry type at byte 0
  s: pc.utf8_upper FAIL: ArrowNotImplementedError: Function 'utf8_upper' has no kernel matching input types (extension<geoarrow.wkt<WktType>>)
  s: pc.utf8_length FAIL: ArrowNotImplementedError: Function 'utf8_length' has no kernel matching input types (extension<geoarrow.wkt<WktType>>)
  s: to_pandas dtype -> extension<geoarrow.wkt<WktType>>[pyarrow]

## geopandas

- read_parquet astext_asbinary.parquet: FAIL: ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
  from_arrow: FAIL: TypeError: Unknown GeoArrow extension type: geoarrow.wkt
- read_parquet astext_asbinary_df.parquet: FAIL: ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
  from_arrow: FAIL: TypeError: Unknown GeoArrow extension type: geoarrow.wkt
- read_parquet cast_varchar.parquet: FAIL: ValueError: Can't interpret string_view as geoarrow.wkb
  from_arrow: FAIL: ValueError: Can't interpret string_view as geoarrow.wkb
- read_parquet union_literal.parquet: FAIL: ValueError: Missing geo metadata in Parquet/Feather file.             Use pandas.read_parquet/read_feather() instead.
  from_arrow: FAIL: TypeError: Unknown GeoArrow extension type: geoarrow.wkt

## DuckDB

- astext_asbinary.parquet: id: BIGINT; wkt: VARCHAR; wkb: BLOB; wkt_junk: VARCHAR; wkt_upper: VARCHAR; centroid: STRUCT(x DOUBLE, y DOUBLE)
  rows: [(1, 'POINT(1 2)', b'\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\xf0?\x00\x00\x00\x00\x00\x00\x00@', 'POINT(1 2) junk', 'POINT(1 2)', {'x': 1.0, 'y': 2.0}), (2, 'LINESTRING(0 0,2 4)', b'\x01\x02\x00\
  from arrow table: id: BIGINT; wkt: VARCHAR; wkb: GEOMETRY('EPSG:4326'); wkt_junk: VARCHAR; wkt_upper: VARCHAR; centroid: STRUCT(x DOUBLE, y DOUBLE)
  ST_AsText(wkb): [('POINT (1 2)',), ('LINESTRING (0 0, 2 4)',)]
- astext_asbinary_df.parquet: id: BIGINT; wkt: VARCHAR; wkb: BLOB; wkt_junk: VARCHAR; wkt_upper: VARCHAR; centroid: STRUCT(x DOUBLE, y DOUBLE)
  rows: [(1, 'POINT(1 2)', b'\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\xf0?\x00\x00\x00\x00\x00\x00\x00@', 'POINT(1 2) junk', 'POINT(1 2)', {'x': 1.0, 'y': 2.0}), (2, 'LINESTRING(0 0,2 4)', b'\x01\x02\x00\
  from arrow table: id: BIGINT; wkt: VARCHAR; wkb: GEOMETRY('EPSG:4326'); wkt_junk: VARCHAR; wkt_upper: VARCHAR; centroid: STRUCT(x DOUBLE, y DOUBLE)
  ST_AsText(wkb): [('POINT (1 2)',), ('LINESTRING (0 0, 2 4)',)]
- cast_varchar.parquet: s: VARCHAR
  rows: []
  from arrow table: FAIL: ValueError: Can't interpret string_view as geoarrow.wkb
- union_literal.parquet: s: VARCHAR
  rows: [('abc',), ('POINT(1 2)',), ('LINESTRING(0 0,2 4)',)]
  from arrow table: s: VARCHAR

## Published Python packages (geodatafusion + datafusion)

- `SELECT ST_AsText(ST_GeomFromText('POINT(1 2)')) AS t`: ok, type=extension<geoarrow.wkt<WktType>>, value=['POINT(1 2)'], field metadata={}
    to_pandas -> 'POINT(1 2)'
- `SELECT ST_AsBinary(ST_GeomFromText('POINT(1 2)')) AS b`: ok, type=extension<geoarrow.wkb<WkbType>>, value=[b'\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\xf0?\x00\x00\x00\x00\x00\x00\x00@'], field metadata={}
    to_pandas -> b'\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\xf0?\x00\x00\x00\x00\x00\x00\x00@'
- `SELECT upper(ST_AsText(ST_GeomFromText('POINT(1 2)'))) AS t`: ok, type=string, value=['POINT(1 2)'], field metadata=None
    to_pandas -> 'POINT(1 2)'
- `SELECT ST_AsText(ST_GeomFromText('POINT(1 2)')) || 'x' AS t`: ok, type=string, value=['POINT(1 2)x'], field metadata=None
    to_pandas -> 'POINT(1 2)x'
- `SELECT md5(ST_AsBinary(ST_GeomFromText('POINT(1 2)'))) AS t`: ok, type=string_view, value=['4ddc678d472071b63dd260ae7d7cd0eb'], field metadata=None
    to_pandas -> '4ddc678d472071b63dd260ae7d7cd0eb'
- `SELECT encode(ST_AsBinary(ST_GeomFromText('POINT(1 2)')), 'hex') AS t`: ok, type=string, value=['0101000000000000000000f03f0000000000000040'], field metadata=None
    to_pandas -> '0101000000000000000000f03f0000000000000040'
- `SELECT ST_AsText(ST_GeomFromText('POINT(1 2)')) LIKE 'POINT%' AS t`: ok, type=bool, value=[True], field metadata=None
    to_pandas -> np.True_

## Controls

- from_arrow without the geoarrow.wkt column: ok, geometry=wkb, crs=EPSG:4326
- pandas .str.upper() on geoarrow.wkt column: FAIL: ArrowNotImplementedError: Function 'utf8_upper' has no kernel matching input types (extension<geoarrow.wkt<WktType>>)
- pandas .str.upper() on plain string column: ['POINT(1 2)', 'LINESTRING(0 0,2 4)']
