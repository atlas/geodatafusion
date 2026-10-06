-- PostGIS behaviour for the H2b/H2c constructs (reference only).
SELECT ST_AsText(g) FROM (SELECT ST_Centroid('LINESTRING(0 0,2 4)'::geometry) g UNION ALL SELECT 'LINESTRING(0 0,2 4)'::geometry) s;
SELECT ST_AsText(CASE WHEN true THEN ST_Centroid('LINESTRING(0 0,2 4)'::geometry) ELSE 'POINT Z(1 2 3)'::geometry END);
SELECT ST_AsText(COALESCE(NULL::geometry, ST_MakePoint(1,2,3)));
SELECT ST_Centroid('LINESTRING(0 0,2 4)'::geometry) = 'POINT(1 2)'::geometry;
SELECT 'SRID=4326;POINT(1 2)'::geometry = 'POINT(1 2)'::geometry;
SELECT ST_AsEWKT(g) FROM (SELECT 'SRID=4326;POINT(1 2)'::geometry g UNION ALL SELECT 'POINT(1 2)'::geometry) s;
SELECT ST_AsText(unnest(ARRAY[ST_Centroid('LINESTRING(0 0,2 4)'::geometry), 'LINESTRING(0 0,1 1)'::geometry]));
SELECT ST_AsText('GEOMETRYCOLLECTION(POINT(1 2))'::geometry);
SELECT postgis_full_version();
