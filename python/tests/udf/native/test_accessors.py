from __future__ import annotations

from arro3.core import Table
from datafusion import SessionContext
import geodatafusion
from geodatafusion import register_all


def test_st_is_closed_geoarrow():
    ctx = SessionContext()
    register_all(ctx)
    sql = "SELECT ST_IsClosed(ST_GeomFromText('POLYGON((0 0, 0 1, 1 1, 1 0, 0 0))')) as geom"
    df = ctx.sql(sql)
    table = df.to_arrow_table()
    assert table.column("geom")[0].as_py() is True


def test_st_is_empty_num_points_and_dump():
    ctx = SessionContext()
    register_all(ctx)
    sql = """
        SELECT
            ST_IsEmpty(ST_GeomFromText('POINT EMPTY')) AS is_empty,
            ST_NumPoints(ST_GeomFromText('LINESTRING(0 0, 1 1)')) AS num_points,
            ST_Dump(ST_GeomFromText('MULTIPOINT(1 2, 3 4)')) AS dump
    """
    row = ctx.sql(sql).to_pylist()[0]
    assert row["is_empty"] is True
    assert row["num_points"] == 2
    assert [part["path"] for part in row["dump"]] == [[1], [2]]
