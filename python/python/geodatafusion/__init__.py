from __future__ import annotations

from typing import TYPE_CHECKING

from datafusion import udaf, udf

from ._rust import *
from ._rust import ___version

__version__: str = ___version()

if TYPE_CHECKING:
    from datafusion import SessionContext


def register_all_geo(ctx: SessionContext):
    from . import geo

    # validation
    ctx.register_udf(udf(geo.IsValid()))


def register_all_native(ctx: SessionContext):
    from . import native

    # accessors
    ctx.register_udf(udf(native.CoordDim()))
    ctx.register_udf(udf(native.Dump()))
    ctx.register_udf(udf(native.EndPoint()))
    ctx.register_udf(udf(native.IsClosed()))
    ctx.register_udf(udf(native.IsEmpty()))
    ctx.register_udf(udf(native.GeometryType()))
    ctx.register_udf(udf(native.M()))
    ctx.register_udf(udf(native.NDims()))
    ctx.register_udf(udf(native.NPoints()))
    ctx.register_udf(udf(native.NumInteriorRings()))
    ctx.register_udf(udf(native.NumPoints()))
    ctx.register_udf(udf(native.StartPoint()))
    ctx.register_udf(udf(native.STGeometryType()))
    ctx.register_udf(udf(native.X()))
    ctx.register_udf(udf(native.Y()))
    ctx.register_udf(udf(native.Z()))
    ctx.register_udf(udf(native.Dimension()))
    ctx.register_udf(udf(native.Zmflag()))
    ctx.register_udf(udf(native.HasZ()))
    ctx.register_udf(udf(native.HasM()))

    # bounding box
    ctx.register_udf(udf(native.Box2D()))
    ctx.register_udf(udf(native.Box3D()))
    ctx.register_udf(udf(native.XMin()))
    ctx.register_udf(udf(native.YMin()))
    ctx.register_udf(udf(native.ZMin()))
    ctx.register_udf(udf(native.XMax()))
    ctx.register_udf(udf(native.YMax()))
    ctx.register_udf(udf(native.ZMax()))
    ctx.register_udf(udf(native.MakeBox2D()))
    ctx.register_udf(udf(native.MakeBox3D()))
    # https://github.com/apache/datafusion-python/issues/1237
    ctx.register_udaf(udaf(native.Extent()))  # type: ignore
    ctx.register_udaf(udaf(native.Extent3D()))  # type: ignore

    # constructors
    ctx.register_udf(udf(native.Collect()))
    ctx.register_udaf(udaf(native.CollectAgg()))  # type: ignore
    ctx.register_udf(udf(native.MakeLine()))
    ctx.register_udaf(udaf(native.MakeLineAgg()))  # type: ignore
    ctx.register_udf(udf(native.Point()))
    ctx.register_udf(udf(native.PointZ()))
    ctx.register_udf(udf(native.PointM()))
    ctx.register_udf(udf(native.PointZM()))
    ctx.register_udf(udf(native.MakePoint()))
    ctx.register_udf(udf(native.MakePointM()))

    # io
    ctx.register_udf(udf(native.AsText()))
    ctx.register_udf(udf(native.AsBinary()))
    ctx.register_udf(udf(native.AsEWKB()))
    ctx.register_udf(udf(native.AsHEXEWKB()))
    ctx.register_udf(udf(native.AsEWKT()))
    ctx.register_udf(udf(native.GeomFromEWKB()))
    ctx.register_udf(udf(native.GeomFromEWKT()))
    ctx.register_udf(udf(native.GeomFromText()))
    ctx.register_udf(udf(native.PointFromText()))
    ctx.register_udf(udf(native.LineFromText()))
    ctx.register_udf(udf(native.PolygonFromText()))
    ctx.register_udf(udf(native.MPointFromText()))
    ctx.register_udf(udf(native.MLineFromText()))
    ctx.register_udf(udf(native.MPolyFromText()))
    ctx.register_udf(udf(native.GeomCollFromText()))
    ctx.register_udf(udf(native.GeomFromWKB()))
    ctx.register_udf(udf(native.PointFromWKB()))
    ctx.register_udf(udf(native.LineFromWKB()))
    ctx.register_udf(udf(native.PolyFromWKB()))
    ctx.register_udf(udf(native.MPointFromWKB()))
    ctx.register_udf(udf(native.MLineFromWKB()))
    ctx.register_udf(udf(native.MPolyFromWKB()))
    ctx.register_udf(udf(native.GeomCollFromWKB()))
    ctx.register_udf(udf(native.GeoHash()))
    ctx.register_udf(udf(native.PointFromGeoHash()))
    ctx.register_udf(udf(native.GeomFromGeoHash()))
    ctx.register_udf(udf(native.Box2DFromGeoHash()))

    # measurement
    ctx.register_udf(udf(native.Area()))
    ctx.register_udf(udf(native.Distance()))
    ctx.register_udf(udf(native.Length()))

    # operators
    ctx.register_udf(udf(native.GeometryAbove()))
    ctx.register_udf(udf(native.GeometryBelow()))
    ctx.register_udf(udf(native.GeometryContains()))
    ctx.register_udf(udf(native.GeometryDistanceBox()))
    ctx.register_udf(udf(native.GeometryLeft()))
    ctx.register_udf(udf(native.GeometryOverAbove()))
    ctx.register_udf(udf(native.GeometryOverBelow()))
    ctx.register_udf(udf(native.GeometryOverLeft()))
    ctx.register_udf(udf(native.GeometryOverRight()))
    ctx.register_udf(udf(native.GeometryOverlaps()))
    ctx.register_udf(udf(native.GeometryOverlapsNd()))
    ctx.register_udf(udf(native.GeometryRight()))
    ctx.register_udf(udf(native.GeometrySame()))
    ctx.register_udf(udf(native.GeometryWithin()))

    # processing
    ctx.register_udf(udf(native.Simplify()))
    ctx.register_udf(udf(native.SimplifyVW()))

    # relationships
    ctx.register_udf(udf(native.DWithin()))
    ctx.register_udf(udf(native.RelateMatch()))

    # srs
    ctx.register_udf(udf(native.SetSRID()))
    ctx.register_udf(udf(native.SRID()))

    # types
    ctx.register_udf(udf(native.Geometry()))


def register_all(ctx: SessionContext):
    register_all_geo(ctx)
    register_all_native(ctx)
