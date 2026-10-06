## 1. geodatafusion-geoparquet reading fixtures/geoparquet/nybb_wkb.parquet

geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"United States (USA) - New York - counties of Bronx; Kings; Nassau; New York; Queens; Richmond; Suffolk.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic latitude","unit":"degree"},{"abbreviation":"Lon","direction":"east","name":"Geodetic longitude","unit":"degree"}],"subtype":"ellipsoidal"},"datum":{"ellipsoid":{"inverse_flattening":298.257222101,"name":"GRS 1980","semi_major_axis":6378137},"name":"North American Datum 1983","type":"G... (2307 chars)

## 2. geoparquet 0.8 encoder (default options) writing a WKB column

- authority_code EPSG:4326: geo.columns.geometry.crs = null  (key present: false)
  read back via geodatafusion-geoparquet: geometry: Binary [geoarrow.wkb]
- authority_code OGC:CRS84: geo.columns.geometry.crs = null  (key present: false)
  read back via geodatafusion-geoparquet: geometry: Binary [geoarrow.wkb]
- srid 4326: geo.columns.geometry.crs = null  (key present: false)
  read back via geodatafusion-geoparquet: geometry: Binary [geoarrow.wkb]
- projjson (abbreviated): geo.columns.geometry.crs = {"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","id":{"authority":"EPSG","code":4326},"name":"WGS 84","type":"GeographicCRS"}  (key present: true)
  read back via geodatafusion-geoparquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","id":{"authority":"EPSG","code":4326},"name":"WGS 84","type":"GeographicCRS"},"crs_type":"projjson"}]
- unknown string EPSG:4326: geo.columns.geometry.crs = null  (key present: false)
  read back via geodatafusion-geoparquet: geometry: Binary [geoarrow.wkb]
- none: geo.columns.geometry.crs = null  (key present: false)
  read back via geodatafusion-geoparquet: geometry: Binary [geoarrow.wkb]

## 3. geodatafusion-geoparquet reading files written by other tools

- duckdb_EPSG_2263.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","area":"United States (USA) - New York - counties of Bronx; Kings; Nassau; New York; Queens; Richmond; Suffolk.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","n... (2307 chars)
- duckdb_EPSG_3857.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","area":"World between 85.06°S and 85.06°N.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic latitude","unit":"degree"},{"abbreviation":"Lon","di... (2264 chars)
- duckdb_EPSG_4326.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1394 chars)
- duckdb_OGC_CRS84.parquet: geometry: Binary [geoarrow.wkb]
- gdal_EPSG_2263_GEOARROW.parquet: geom: Struct<x,y> [geoarrow.point {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"United States (USA) - New York - counties of Bronx; Kings; Nassau; New York; Queens; Richmond; Suffolk.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north"... (2596 chars)
- gdal_EPSG_2263_WKB.parquet: geom: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"United States (USA) - New York - counties of Bronx; Kings; Nassau; New York; Queens; Richmond; Suffolk.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name"... (2589 chars)
- gdal_EPSG_3857_GEOARROW.parquet: geom: Struct<x,y> [geoarrow.point {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World between 85.06°S and 85.06°N.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic latitude","unit":"degree"},{"abbreviation":"Lon",... (2070 chars)
- gdal_EPSG_3857_WKB.parquet: geom: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World between 85.06°S and 85.06°N.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic latitude","unit":"degree"},{"abbreviation":"Lon","direct... (2063 chars)
- gdal_EPSG_4326_GEOARROW.parquet: geom: Struct<x,y> [geoarrow.point]
- gdal_EPSG_4326_WKB.parquet: geom: Binary [geoarrow.wkb]
- gdal_OGC_CRS84_GEOARROW.parquet: geom: Struct<x,y> [geoarrow.point]
- gdal_OGC_CRS84_WKB.parquet: geom: Binary [geoarrow.wkb]
- geo_string_crs_EPSG_3857.parquet: geometry: Binary [geoarrow.wkb {"crs":"EPSG:3857","crs_type":"projjson"}]
- geo_string_crs_EPSG_4326.parquet: geometry: Binary [geoarrow.wkb {"crs":"EPSG:4326","crs_type":"projjson"}]
- geo_string_crs_OGC_CRS84.parquet: geometry: Binary [geoarrow.wkb {"crs":"OGC:CRS84","crs_type":"projjson"}]
- geopandas_EPSG_2263_WKB.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"United States (USA) - New York - counties of Bronx; Kings; Nassau; New York; Queens; Richmond; Suffolk.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","n... (2593 chars)
- geopandas_EPSG_2263_geoarrow.parquet: geometry: Struct<x,y> [geoarrow.point {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"United States (USA) - New York - counties of Bronx; Kings; Nassau; New York; Queens; Richmond; Suffolk.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"no... (2600 chars)
- geopandas_EPSG_3857_WKB.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World between 85.06°S and 85.06°N.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic latitude","unit":"degree"},{"abbreviation":"Lon","di... (2067 chars)
- geopandas_EPSG_3857_geoarrow.parquet: geometry: Struct<x,y> [geoarrow.point {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World between 85.06°S and 85.06°N.","base_crs":{"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic latitude","unit":"degree"},{"abbreviation":"L... (2074 chars)
- geopandas_EPSG_4326_WKB.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1174 chars)
- geopandas_EPSG_4326_geoarrow.parquet: geometry: Struct<x,y> [geoarrow.point {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geo... (1181 chars)
- geopandas_OGC_CRS84_WKB.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lon","direction":"east","name":"Geodetic lo... (1160 chars)
- geopandas_OGC_CRS84_geoarrow.parquet: geometry: Struct<x,y> [geoarrow.point {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lon","direction":"east","name":"Geod... (1167 chars)
- rt_duckdb_0.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1394 chars)
- rt_duckdb_1.parquet: geometry: Binary [geoarrow.wkb]
- rt_duckdb_2.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1394 chars)
- rt_duckdb_3.parquet: geometry: Binary [geoarrow.wkb]
- rt_duckdb_4.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.5/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1394 chars)
- rt_gpd_0.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1174 chars)
- rt_gpd_1.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lon","direction":"east","name":"Geodetic lo... (1160 chars)
- rt_gpd_2.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1174 chars)
- rt_gpd_3.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1174 chars)
- rt_gpd_4.parquet: geometry: Binary [geoarrow.wkb {"crs":{"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","area":"World.","bbox":{"east_longitude":180,"north_latitude":90,"south_latitude":-90,"west_longitude":-180},"coordinate_system":{"axis":[{"abbreviation":"Lat","direction":"north","name":"Geodetic l... (1174 chars)
- tag_authority_code_EPSG_4326.parquet: geometry: BinaryView [geoarrow.wkb {"crs": "EPSG:4326", "crs_type": "authority_code"}]
- tag_authority_code_EPSG_4326_gdal.parquet: geometry: Binary [geoarrow.wkb]
- tag_authority_code_OGC_CRS84.parquet: geometry: BinaryView [geoarrow.wkb {"crs": "OGC:CRS84", "crs_type": "authority_code"}]
- tag_authority_code_OGC_CRS84_gdal.parquet: geometry: Binary [geoarrow.wkb]
- tag_no_crs_type_EPSG_4326.parquet: geometry: BinaryView [geoarrow.wkb {"crs": "EPSG:4326"}]
- tag_no_crs_type_EPSG_4326_gdal.parquet: geometry: Binary [geoarrow.wkb]
- tag_projjson_EPSG_4326.parquet: geometry: BinaryView [geoarrow.wkb {"crs": {"$schema": "https://proj.org/schemas/v0.7/projjson.schema.json", "type": "GeographicCRS", "name": "WGS 84", "datum_ensemble": {"name": "World Geodetic System 1984 ensemble", "members": [{"name": "World Geodetic System 1984 (Transit)", "id": {"authority": "... (1602 chars)
- tag_projjson_EPSG_4326_gdal.parquet: geometry: Binary [geoarrow.wkb]
- tag_srid_4326.parquet: geometry: BinaryView [geoarrow.wkb {"crs": "4326", "crs_type": "srid"}]
- tag_srid_4326_gdal.parquet: geometry: Binary [geoarrow.wkb]

## 4. DataFusion COPY of tagged columns (no `geo` metadata)

- out/h9_rust/datafusion_copy_4326.parquet: Ok("ok")
- out/h9_rust/datafusion_copy_3857.parquet: Ok("ok")
