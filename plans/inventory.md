# PostGIS function inventory

Every function in the PostGIS 3.6.4 reference docs, except the raster, SFCGAL, version, GUC,
troubleshooting, management and exception chapters. **Group** is the implementation group after
reconciling the group plans and the experiments (see [README.md](README.md)). `G1+G5` means a scalar form in G1 and an
aggregate form in G5; `—` means won't do (see the group plans for why).
**Doc tests** is the `passed/total` count for `slt/postgis_docs/<file>.slt` from `parity.txt`
(— means the docs have no usable examples).
**Usage rank** is the function's rank by use in public code (1 = most used, of 297), from
[experiment E6](experiments/e6-usage.md). Within a group's phase, work in usage order.

| Group | Chapter | Function(s) | Implemented | Doc tests | Usage rank | File |
|---|---|---|---|---|---|---|
| G1 | accessor | GeometryType | yes | 1/3 | 32 | geometrytype |
| G1 | accessor | ST_Boundary |  | 0/6 | 64 | st_boundary |
| G1 | accessor | ST_BoundingDiagonal |  | 0/1 | 211 | st_boundingdiagonal |
| G1 | accessor | ST_CoordDim | yes | 1/2 | 167 | st_coorddim |
| G1 | accessor | ST_CurveN |  | 0/1 | 269 | st_curven |
| G1 | accessor | ST_Dimension |  | 0/1 | 81 | st_dimension |
| G1 | accessor | ST_EndPoint | yes | 1/3 | 24 | st_endpoint |
| G1 | accessor | ST_Envelope |  | 0/6 | 37 | st_envelope |
| G1 | accessor | ST_ExteriorRing |  | — | 66 | st_exteriorring |
| G1 | accessor | ST_GeometryN |  | 0/2 | 60 | st_geometryn |
| G1 | accessor | ST_GeometryType | yes | 1/4 | 25 | st_geometrytype |
| G1 | accessor | ST_HasArc |  | 0/1 | 149 | st_hasarc |
| G1 | accessor | ST_HasM |  | 0/2 | 259 | st_hasm |
| G1 | accessor | ST_HasZ |  | 0/2 | 261 | st_hasz |
| G1 | accessor | ST_InteriorRingN |  | 0/1 | 136 | st_interiorringn |
| G1 | accessor | ST_IsClosed | yes | 0/2 | 98 | st_isclosed |
| G1 | accessor | ST_IsCollection |  | — | 192 | st_iscollection |
| G1 | accessor | ST_IsEmpty | yes | 4/5 | 39 | st_isempty |
| G1 | accessor | ST_IsPolygonCCW |  | — | 222 | st_ispolygonccw |
| G1 | accessor | ST_IsPolygonCW |  | — | 138 | st_ispolygoncw |
| G1 | accessor | ST_M | yes | 0/1 | 36 | st_m |
| G1 | accessor | ST_MemSize |  | — | 207 | st_memsize |
| G1 | accessor | ST_NDims | yes | 0/1 | 77 | st_ndims |
| G1 | accessor | ST_NPoints | yes | — | 47 | st_npoints |
| G1 | accessor | ST_NRings |  | 0/1 | 182 | st_nrings |
| G1 | accessor | ST_NumCurves |  | 0/2 | 277 | st_numcurves |
| G1 | accessor | ST_NumGeometries |  | 0/2 | 74 | st_numgeometries |
| G1 | accessor | ST_NumInteriorRing | yes | — | 195 | st_numinteriorring |
| G1 | accessor | ST_NumInteriorRings | yes | — | 125 | st_numinteriorrings |
| G1 | accessor | ST_NumPatches |  | 0/1 | 268 | st_numpatches |
| G1 | accessor | ST_NumPoints | yes | 1/1 | 73 | st_numpoints |
| G1 | accessor | ST_PatchN |  | 0/1 | 267 | st_patchn |
| G1 | accessor | ST_PointN |  | 0/3 | 55 | st_pointn |
| G1 | accessor | ST_Points |  | — | 116 | st_points |
| G1 | accessor | ST_StartPoint | yes | 1/4 | 31 | st_startpoint |
| G1 | accessor | ST_Summary |  | — | 97 | st_summary |
| G1 | accessor | ST_X | yes | 0/2 | 4 | st_x |
| G1 | accessor | ST_Y | yes | 0/2 | 1 | st_y |
| G1 | accessor | ST_Z | yes | 0/1 | 23 | st_z |
| G1 | accessor | ST_Zmflag |  | 0/4 | 225 | st_zmflag |
| G1 | bbox | ST_3DMakeBox | yes | 1/1 | 120 | st_3dmakebox |
| G1 | bbox | ST_Expand |  | — | 80 | st_expand |
| G1 | bbox | ST_MakeBox2D | yes | — | 110 | st_makebox2d |
| G1 | bbox | ST_XMax | yes | — | 44 | st_xmax |
| G1 | bbox | ST_XMin | yes | — | 41 | st_xmin |
| G1 | bbox | ST_YMax | yes | — | 46 | st_ymax |
| G1 | bbox | ST_YMin | yes | — | 43 | st_ymin |
| G1 | bbox | ST_ZMax | yes | — | 142 | st_zmax |
| G1 | bbox | ST_ZMin | yes | — | 141 | st_zmin |
| G1 | constructor | ST_Hexagon |  | 0/1 | 241 | st_hexagon |
| G1 | constructor | ST_LineFromMultiPoint |  | 0/1 | 150 | st_linefrommultipoint |
| G1 | constructor | ST_MakeEnvelope |  | 0/1 | 33 | st_makeenvelope |
| G1 | constructor | ST_MakePoint | yes | 3/4 | 3 | st_makepoint |
| G1 | constructor | ST_MakePointM | yes | 1/3 | 188 | st_makepointm |
| G1 | constructor | ST_MakePolygon |  | 0/5 | 51 | st_makepolygon |
| G1 | constructor | ST_Point | yes | 2/6 | 13 | st_point |
| G1 | constructor | ST_PointM | yes | 2/3 | 278 | st_pointm |
| G1 | constructor | ST_PointZ | yes | 2/3 | 231 | st_pointz |
| G1 | constructor | ST_PointZM | yes | 2/3 | 279 | st_pointzm |
| G1 | constructor | ST_Polygon |  | 0/2 | 101 | st_polygon |
| G1 | constructor | ST_Square |  | 0/1 | 280 | st_square |
| G1 | constructor | ST_TileEnvelope |  | 0/2 | 75 | st_tileenvelope |
| G1 | editor | ST_AddPoint |  | 0/1 | 96 | st_addpoint |
| G1 | editor | ST_CollectionExtract |  | 0/3 | 42 | st_collectionextract |
| G1 | editor | ST_CollectionHomogenize |  | 0/5 | 175 | st_collectionhomogenize |
| G1 | editor | ST_CurveToLine |  | 0/4 | 104 | st_curvetoline |
| G1 | editor | ST_FlipCoordinates |  | 0/1 | 160 | st_flipcoordinates |
| G1 | editor | ST_Force2D |  | 0/2 | 56 | st_force2d |
| G1 | editor | ST_Force3D |  | 0/2 | 94 | st_force_3d |
| G1 | editor | ST_Force3DM |  | 0/2 | 224 | st_force_3dm |
| G1 | editor | ST_Force3DZ |  | 0/2 | 177 | st_force_3dz |
| G1 | editor | ST_Force4D |  | 0/2 | 208 | st_force_4d |
| G1 | editor | ST_ForceCollection |  | 0/3 | 164 | st_force_collection |
| G1 | editor | ST_ForceCurve |  | 0/1 | 223 | st_forcecurve |
| G1 | editor | ST_ForcePolygonCCW |  | — | 210 | st_forcepolygonccw |
| G1 | editor | ST_ForcePolygonCW |  | — | 185 | st_forcepolygoncw |
| G1 | editor | ST_ForceRHR |  | 0/1 | 131 | st_forcerhr |
| G1 | editor | ST_LineExtend |  | 0/1 | 238 | st_lineextend |
| G1 | editor | ST_LineToCurve |  | 0/2 | 204 | st_linetocurve |
| G1 | editor | ST_Multi |  | 0/1 | 11 | st_multi |
| G1 | editor | ST_Project |  | 0/1 | 35 | st_project |
| G1 | editor | ST_QuantizeCoordinates |  | 0/2 | 229 | st_quantizecoordinates |
| G1 | editor | ST_RemoveIrrelevantPointsForView |  | 0/5 | 252 | st_removeirrelevantpointsforview |
| G1 | editor | ST_RemovePoint |  | — | 156 | st_removepoint |
| G1 | editor | ST_RemoveRepeatedPoints |  | 0/4 | 115 | st_removerepeatedpoints |
| G1 | editor | ST_RemoveSmallParts |  | 0/2 | 271 | st_removesmallparts |
| G1 | editor | ST_Reverse |  | 0/1 | 61 | st_reverse |
| G1 | editor | ST_Scroll |  | 0/1 | 281 | st_scroll |
| G1 | editor | ST_Segmentize |  | 0/3 | 121 | st_segmentize |
| G1 | editor | ST_SetPoint |  | 0/3 | 93 | st_setpoint |
| G1 | editor | ST_ShiftLongitude |  | 0/1 | 196 | st_shiftlongitude |
| G1 | editor | ST_SnapToGrid |  | — | 69 | st_snaptogrid |
| G1 | editor | ST_SwapOrdinates |  | 0/1 | 262 | st_swapordinates |
| G1 | lrs | ST_3DLineInterpolatePoint |  | 0/1 | 219 | st_3dlineinterpolatepoint |
| G1 | lrs | ST_AddMeasure |  | 0/4 | 145 | st_addmeasure |
| G1 | lrs | ST_InterpolatePoint |  | 0/1 | 209 | st_interpolatepoint |
| G1 | lrs | ST_LineInterpolatePoint |  | 0/3 | 57 | st_lineinterpolatepoint |
| G1 | lrs | ST_LineInterpolatePoints |  | — | 189 | st_lineinterpolatepoints |
| G1 | lrs | ST_LineSubstring |  | 0/4 | 82 | st_linesubstring |
| G1 | lrs | ST_LocateAlong |  | 0/1 | 151 | st_locatealong |
| G1 | lrs | ST_LocateBetween |  | 0/2 | 173 | st_locatebetween |
| G1 | lrs | ST_LocateBetweenElevations |  | 0/2 | 228 | st_locatebetweenelevations |
| G1 | measure | ST_3DClosestPoint |  | 0/3 | 233 | st_3dclosestpoint |
| G1 | measure | ST_3DDistance |  | 0/2 | 174 | st_3ddistance |
| G1 | measure | ST_3DLength |  | 0/1 | 178 | st_3dlength |
| G1 | measure | ST_3DLongestLine |  | 0/3 | 253 | st_3dlongestline |
| G1 | measure | ST_3DMaxDistance |  | 0/1 | 240 | st_3dmaxdistance |
| G1 | measure | ST_3DPerimeter |  | 0/1 | 260 | st_3dperimeter |
| G1 | measure | ST_3DShortestLine |  | 0/3 | 242 | st_3dshortestline |
| G1 | measure | ST_Angle |  | 0/3 | 198 | st_angle |
| G1 | measure | ST_Area | yes | 0/3 | 10 | st_area |
| G1 | measure | ST_ClosestPoint |  | 0/2 | 70 | st_closestpoint |
| G1 | measure | ST_Distance | yes | 0/6 | 5 | st_distance |
| G1 | measure | ST_LongestLine |  | 0/3 | 205 | st_longestline |
| G1 | measure | ST_MaxDistance |  | 0/3 | 161 | st_maxdistance |
| G1 | measure | ST_ShortestLine |  | 0/2 | 119 | st_shortestline |
| G1 | processing | ST_ChaikinSmoothing |  | 0/3 | 181 | st_chaikinsmoothing |
| G1 | processing | ST_FilterByM |  | 0/1 | 282 | st_filterbym |
| G1 | processing | ST_GeneratePoints |  | 0/1 | 144 | st_generatepoints |
| G1 | processing | ST_GeometricMedian |  | 0/1 | 214 | st_geometricmedian |
| G1 | processing | ST_MinimumBoundingCircle |  | 0/1 | 168 | st_minimumboundingcircle |
| G1 | processing | ST_MinimumBoundingRadius |  | 0/1 | 190 | st_minimumboundingradius |
| G1 | processing | ST_SetEffectiveArea |  | 0/1 | 254 | st_seteffectivearea |
| G1 | processing | ST_Simplify | yes | 0/1 | 48 | st_simplify |
| G1 | processing | ST_SimplifyVW | yes | 3/3 | 162 | st_simplifyvw |
| G1 | relationship | ST_3DDFullyWithin |  | 0/1 | 247 | st_3ddfullywithin |
| G1 | relationship | ST_3DDWithin |  | 0/1 | 186 | st_3ddwithin |
| G1 | relationship | ST_3DIntersects |  | 0/2 | 194 | st_3dintersects |
| G1 | relationship | ST_DWithin |  | — | 6 | st_dwithin |
| G1 | relationship | ST_LineCrossingDirection |  | 0/3 | 237 | st_linecrossingdirection |
| G1 | relationship | ST_OrderingEquals |  | 0/3 | 137 | st_orderingequals |
| G1 | relationship | ST_PointInsideCircle |  | 0/1 | 283 | st_pointinsidecircle |
| G1 | trajectory | ST_ClosestPointOfApproach |  | 0/1 | 272 | st_closestpointofapproach |
| G1 | trajectory | ST_CPAWithin |  | 0/1 | 284 | st_cpawithin |
| G1 | trajectory | ST_DistanceCPA |  | 0/1 | 232 | st_distancecpa |
| G1 | trajectory | ST_IsValidTrajectory |  | 0/2 | 273 | st_isvalidtrajectory |
| G1 | transformation | ST_Affine |  | 0/2 | 171 | st_affine |
| G1 | transformation | ST_Rotate |  | 0/3 | 78 | st_rotate |
| G1 | transformation | ST_RotateX |  | 0/1 | 193 | st_rotatex |
| G1 | transformation | ST_RotateY |  | 0/1 | 227 | st_rotatey |
| G1 | transformation | ST_RotateZ |  | 0/2 | 218 | st_rotatez |
| G1 | transformation | ST_Scale |  | 0/4 | 123 | st_scale |
| G1 | transformation | ST_Translate |  | 0/4 | 54 | st_translate |
| G1 | transformation | ST_TransScale |  | 0/2 | 212 | st_transscale |
| G1+G5 | constructor | ST_Collect |  | 0/4 | 9 | st_collect |
| G1+G5 | constructor | ST_MakeLine |  | 0/4 | 26 | st_makeline |
| G2 | lrs | ST_LineLocatePoint |  | 0/2 | 52 | st_linelocatepoint |
| G2 | measure | ST_Azimuth |  | 0/1 | 63 | st_azimuth |
| G2 | measure | ST_DistanceSphere |  | 0/1 | 83 | st_distancesphere |
| G2 | measure | ST_DistanceSpheroid |  | 0/1 | 199 | st_distance_spheroid |
| G2 | measure | ST_FrechetDistance |  | 0/1 | 220 | st_frechetdistance |
| G2 | measure | ST_HausdorffDistance |  | 0/2 | 170 | st_hausdorffdistance |
| G2 | measure | ST_Length | yes | 0/3 | 16 | st_length |
| G2 | measure | ST_Length2D | yes | — | 124 | st_length2d |
| G2 | measure | ST_LengthSpheroid |  | — | 200 | st_length_spheroid |
| G2 | measure | ST_Perimeter |  | 0/4 | 79 | st_perimeter |
| G2 | measure | ST_Perimeter2D |  | — | 250 | st_perimeter2d |
| G2 | validation | ST_IsValid | yes | — | 29 | st_isvalid |
| G3 | accessor | ST_IsRing |  | 0/2 | 147 | st_isring |
| G3 | accessor | ST_IsSimple |  | 0/2 | 117 | st_issimple |
| G3 | editor | ST_Normalize |  | 0/1 | 153 | st_normalize |
| G3 | editor | ST_Snap |  | 0/4 | 27 | st_snap |
| G3 | editor | ST_WrapX |  | — | 221 | st_wrapx |
| G3 | input | ST_BdMPolyFromText |  | — | 251 | st_bdmpolyfromtext |
| G3 | input | ST_BdPolyFromText |  | — | 245 | st_bdpolyfromtext |
| G3 | measure | ST_MinimumClearance |  | 0/1 | 274 | st_minimumclearance |
| G3 | measure | ST_MinimumClearanceLine |  | 0/1 | 275 | st_minimumclearanceline |
| G3 | output | ST_AsMVTGeom |  | 0/1 | 68 | st_asmvtgeom |
| G3 | overlay | ST_ClipByBox2D |  | — | 201 | st_clipbybox2d |
| G3 | overlay | ST_Difference |  | 0/2 | 45 | st_difference |
| G3 | overlay | ST_Intersection |  | 0/3 | 20 | st_intersection |
| G3 | overlay | ST_Node |  | 0/2 | 84 | st_node |
| G3 | overlay | ST_Split |  | 0/3 | 59 | st_split |
| G3 | overlay | ST_SymDifference |  | 0/2 | 118 | st_symdifference |
| G3 | overlay | ST_UnaryUnion |  | — | 87 | st_unaryunion |
| G3 | processing | ST_Buffer |  | 0/14 | 21 | st_buffer |
| G3 | processing | ST_BuildArea |  | 0/2 | 108 | st_buildarea |
| G3 | processing | ST_Centroid | yes | 0/3 | 17 | st_centroid |
| G3 | processing | ST_ConcaveHull |  | 0/3 | 127 | st_concavehull |
| G3 | processing | ST_ConvexHull | yes | 0/1 | 65 | st_convexhull |
| G3 | processing | ST_DelaunayTriangles |  | 0/3 | 184 | st_delaunaytriangles |
| G3 | processing | ST_LargestEmptyCircle |  | 0/2 | 285 | st_largestemptycircle |
| G3 | processing | ST_LineMerge | yes | 4/5 | 50 | st_linemerge |
| G3 | processing | ST_MaximumInscribedCircle |  | 0/1 | 183 | st_maximuminscribedcircle |
| G3 | processing | ST_OffsetCurve |  | 0/6 | 128 | st_offsetcurve |
| G3 | processing | ST_OrientedEnvelope | yes | 0/2 | 172 | st_orientedenvelope |
| G3 | processing | ST_PointOnSurface | yes | 3/5 | 49 | st_pointonsurface |
| G3 | processing | ST_ReducePrecision |  | 0/5 | 112 | st_reduceprecision |
| G3 | processing | ST_SharedPaths |  | 0/2 | 246 | st_sharedpaths |
| G3 | processing | ST_SimplifyPolygonHull |  | 0/3 | 263 | st_simplifypolygonhull |
| G3 | processing | ST_SimplifyPreserveTopology | yes | 0/3 | 76 | st_simplifypreservetopology |
| G3 | processing | ST_TriangulatePolygon |  | 0/3 | 276 | st_triangulatepolygon |
| G3 | processing | ST_VoronoiLines |  | 0/1 | 255 | st_voronoilines |
| G3 | processing | ST_VoronoiPolygons |  | 0/2 | 140 | st_voronoipolygons |
| G3 | relationship | ST_Contains | yes | 0/2 | 15 | st_contains |
| G3 | relationship | ST_ContainsProperly |  | 0/2 | 166 | st_containsproperly |
| G3 | relationship | ST_CoveredBy | yes | 0/1 | 109 | st_coveredby |
| G3 | relationship | ST_Covers | yes | 0/2 | 72 | st_covers |
| G3 | relationship | ST_Crosses | yes | — | 103 | st_crosses |
| G3 | relationship | ST_DFullyWithin |  | 0/1 | 249 | st_dfullywithin |
| G3 | relationship | ST_Disjoint | yes | 2/2 | 132 | st_disjoint |
| G3 | relationship | ST_Equals | yes | 1/2 | 38 | st_equals |
| G3 | relationship | ST_Intersects | yes | 0/1 | 7 | st_intersects |
| G3 | relationship | ST_Overlaps | yes | 2/3 | 99 | st_overlaps |
| G3 | relationship | ST_Relate |  | 0/3 | 100 | st_relate |
| G3 | relationship | ST_RelateMatch |  | 0/2 | 217 | st_relatematch |
| G3 | relationship | ST_Touches | yes | 2/2 | 71 | st_touches |
| G3 | relationship | ST_Within | yes | 0/1 | 28 | st_within |
| G3 | srs | ST_InverseTransformPipeline |  | 0/2 | 286 | st_inversetransformpipeline |
| G3 | srs | ST_Transform |  | 0/3 | 8 | st_transform |
| G3 | srs | ST_TransformPipeline |  | 0/2 | 248 | st_transformpipeline |
| G3 | validation | ST_IsValidDetail |  | 0/2 | 130 | st_isvaliddetail |
| G3 | validation | ST_IsValidReason | yes | 1/3 | 126 | st_isvalidreason |
| G3 | validation | ST_MakeValid |  | 0/4 | 40 | st_makevalid |
| G3+G5 | overlay | ST_Union |  | 0/1 | 12 | st_union |
| G3+G5 | processing | ST_Polygonize |  | 0/1 | 106 | st_polygonize |
| G4 | input | ST_Box2dFromGeoHash | yes | 0/3 | 256 | st_box2dfromgeohash |
| G4 | input | ST_GeogFromText |  | — | 53 | st_geogfromtext |
| G4 | input | ST_GeogFromWKB |  | 0/1 | 113 | st_geogfromwkb |
| G4 | input | ST_GeographyFromText |  | — | 114 | st_geographyfromtext |
| G4 | input | ST_GeomCollFromText |  | 0/1 | 179 | st_geomcollfromtext |
| G4 | input | ST_GeometryFromText | yes | — | 95 | st_geometryfromtext |
| G4 | input | ST_GeomFromEWKB |  | 0/1 | 139 | st_geomfromewkb |
| G4 | input | ST_GeomFromEWKT |  | 0/7 | 86 | st_geomfromewkt |
| G4 | input | ST_GeomFromGeoHash |  | 0/3 | 213 | st_geomfromgeohash |
| G4 | input | ST_GeomFromGeoJSON |  | 0/2 | 34 | st_geomfromgeojson |
| G4 | input | ST_GeomFromGML |  | 0/3 | 226 | st_geomfromgml |
| G4 | input | ST_GeomFromKML |  | 0/1 | 235 | st_geomfromkml |
| G4 | input | ST_GeomFromText | yes | 4/7 | 14 | st_geomfromtext |
| G4 | input | ST_GeomFromTWKB |  | 0/2 | 243 | st_geomfromtwkb |
| G4 | input | ST_GeomFromWKB | yes | 0/2 | 88 | st_geomfromwkb |
| G4 | input | ST_GMLToSQL |  | — | 264 | st_gmltosql |
| G4 | input | ST_LineFromEncodedPolyline |  | 0/2 | 215 | st_linefromencodedpolyline |
| G4 | input | ST_LineFromText |  | 0/1 | 133 | st_linefromtext |
| G4 | input | ST_LineFromWKB |  | 0/1 | 191 | st_linefromwkb |
| G4 | input | ST_LinestringFromWKB |  | 0/1 | 202 | st_linestringfromwkb |
| G4 | input | ST_MLineFromText |  | 0/1 | 134 | st_mlinefromtext |
| G4 | input | ST_MPointFromText |  | 0/2 | 154 | st_mpointfromtext |
| G4 | input | ST_MPolyFromText |  | 0/2 | 143 | st_mpolyfromtext |
| G4 | input | ST_PointFromGeoHash | yes | 0/3 | 216 | st_pointfromgeohash |
| G4 | input | ST_PointFromText |  | 0/2 | 89 | st_pointfromtext |
| G4 | input | ST_PointFromWKB |  | 0/2 | 146 | st_pointfromwkb |
| G4 | input | ST_PolygonFromText |  | 0/2 | 129 | st_polygonfromtext |
| G4 | input | ST_WKBToSQL | yes | — | 206 | st_wkbtosql |
| G4 | input | ST_WKTToSQL | yes | — | 187 | st_wkttosql |
| G4 | output | ST_AsBinary | yes | 0/2 | 91 | st_asbinary |
| G4 | output | ST_AsEncodedPolyline |  | 0/2 | 244 | st_asencodedpolyline |
| G4 | output | ST_AsEWKB |  | 0/2 | 111 | st_asewkb |
| G4 | output | ST_AsEWKT |  | — | 85 | st_asewkt |
| G4 | output | ST_AsGeoJSON |  | 0/4 | 22 | st_asgeojson |
| G4 | output | ST_AsGML |  | 0/5 | 152 | st_asgml |
| G4 | output | ST_AsHEXEWKB |  | 0/2 | 236 | st_ashexewkb |
| G4 | output | ST_AsKML |  | 0/2 | 159 | st_askml |
| G4 | output | ST_AsLatLonText |  | 0/6 | 176 | st_aslatlontext |
| G4 | output | ST_AsMARC21 |  | 0/3 | 288 | st_asmarc21 |
| G4 | output | ST_AsSVG |  | 0/4 | 169 | st_assvg |
| G4 | output | ST_AsText | yes | — | 18 | st_astext |
| G4 | output | ST_AsTWKB |  | 0/1 | 180 | st_astwkb |
| G4 | output | ST_AsX3D |  | 0/3 | 122 | st_asx3d |
| G4 | output | ST_GeoHash | yes | 0/3 | 102 | st_geohash |
| G5 | accessor | ST_Dump | yes | 0/2 | 19 | st_dump |
| G5 | accessor | ST_DumpPoints |  | 0/4 | 58 | st_dumppoints |
| G5 | accessor | ST_DumpRings |  | 0/1 | 90 | st_dumprings |
| G5 | accessor | ST_DumpSegments |  | 0/3 | 158 | st_dumpsegments |
| G5 | bbox | ST_3DExtent |  | — | 203 | st_3dextent |
| G5 | bbox | ST_Extent | yes | — | 62 | st_extent |
| G5 | cluster | ST_ClusterDBSCAN |  | — | 92 | st_clusterdbscan |
| G5 | cluster | ST_ClusterIntersecting |  | 0/1 | 197 | st_clusterintersecting |
| G5 | cluster | ST_ClusterIntersectingWin |  | 0/1 | 239 | st_clusterintersectingwin |
| G5 | cluster | ST_ClusterKMeans |  | 0/2 | 165 | st_clusterkmeans |
| G5 | cluster | ST_ClusterWithin |  | 0/1 | 148 | st_clusterwithin |
| G5 | cluster | ST_ClusterWithinWin |  | 0/1 | 265 | st_clusterwithinwin |
| G5 | constructor | ST_HexagonGrid |  | — | 163 | st_hexagongrid |
| G5 | constructor | ST_SquareGrid |  | — | 155 | st_squaregrid |
| G5 | coverage | ST_CoverageClean |  | — | 266 | st_coverageclean |
| G5 | coverage | ST_CoverageInvalidEdges |  | 0/1 | 257 | st_coverageinvalidedges |
| G5 | coverage | ST_CoverageSimplify |  | 0/1 | 234 | st_coveragesimplify |
| G5 | coverage | ST_CoverageUnion |  | 0/1 | 289 | st_coverageunion |
| G5 | input | ST_FromFlatGeobuf |  | — | 290 | st_fromflatgeobuf |
| G5 | input | ST_FromFlatGeobufToTable |  | — | 291 | st_fromflatgeobuftotable |
| G5 | output | ST_AsFlatGeobuf |  | — | 292 | st_asflatgeobuf |
| G5 | output | ST_AsGeobuf |  | 0/1 | 258 | st_asgeobuf |
| G5 | output | ST_AsMVT |  | — | 67 | st_asmvt |
| G5 | overlay | ST_MemUnion |  | — | 230 | st_memunion |
| G5 | overlay | ST_Subdivide |  | 0/2 | 105 | st_subdivide |
| G5 | srs | postgis_srs |  | 0/1 | 293 | postgis_srs |
| G5 | srs | postgis_srs_all |  | 0/1 | 294 | postgis_srs_all |
| G5 | srs | postgis_srs_codes |  | 0/1 | 295 | postgis_srs_codes |
| G5 | srs | postgis_srs_search |  | 0/1 | 296 | postgis_srs_search |
| G6 | bbox | Box2D | yes | 1/2 | 107 | box2d |
| G6 | bbox | Box3D | yes | 0/2 | 157 | box3d |
| G6 | operator | && |  | 0/1 | — | geometry_overlaps |
| G6 | operator | &&& |  | 0/2 | — | geometry_overlaps_nd |
| G6 | operator | &&&(geometry,gidx) |  | 0/1 | — | overlaps_nd_geometry_gidx |
| G6 | operator | &&&(gidx,geometry) |  | 0/1 | — | overlaps_nd_gidx_geometry |
| G6 | operator | &&&(gidx,gidx) |  | 0/1 | — | overlaps_nd_gidx_gidx |
| G6 | operator | &&(box2df,box2df) |  | 0/1 | — | overlaps_box2df_box2df |
| G6 | operator | &&(box2df,geometry) |  | 0/1 | — | overlaps_box2df_geometry |
| G6 | operator | &&(geometry,box2df) |  | 0/1 | — | overlaps_geometry_box2df |
| G6 | operator | &< |  | 0/1 | — | st_geometry_overleft |
| G6 | operator | &<| |  | 0/1 | st_geometry_overbelow |
| G6 | operator | &> |  | 0/1 | — | st_geometry_overright |
| G6 | operator | <#> |  | — | — | geometry_distance_box |
| G6 | operator | <-> |  | — | — | geometry_distance_knn |
| G6 | operator | << |  | 0/1 | — | st_geometry_left |
| G6 | operator | <<->> |  | — | — | geometry_distance_centroid_nd |
| G6 | operator | <<| |  | 0/1 | st_geometry_below |
| G6 | operator | = |  | 4/4 | — | st_geometry_eq |
| G6 | operator | >> |  | 0/1 | — | st_geometry_right |
| G6 | operator | @ |  | 0/1 | — | st_geometry_contained |
| G6 | operator | @(box2df,box2df) |  | 0/1 | — | is_contained_box2df_box2df |
| G6 | operator | @(box2df,geometry) |  | 0/1 | — | is_contained_box2df_geometry |
| G6 | operator | @(geometry,box2df) |  | 0/1 | — | is_contained_geometry_box2df |
| G6 | operator | |&> |  | 0/1 | st_geometry_overabove |
| G6 | operator | |=| |  | — | geometry_distance_cpa |
| G6 | operator | |>> |  | 0/1 | st_geometry_above |
| G6 | operator | ~ |  | 0/1 | — | st_geometry_contain |
| G6 | operator | ~(box2df,box2df) |  | 0/1 | — | contains_box2df_box2df |
| G6 | operator | ~(box2df,geometry) |  | 0/1 | — | contains_box2df_geometry |
| G6 | operator | ~(geometry,box2df) |  | 0/1 | — | contains_geometry_box2df |
| G6 | operator | ~= |  | 0/1 | — | st_geometry_same |
| G6 | srs | ST_SetSRID |  | 0/2 | 2 | st_setsrid |
| G6 | srs | ST_SRID |  | 0/1 | 30 | st_srid |
| G6 | type | box2d | yes | — | 107 | box2d_type |
| G6 | type | box3d | yes | — | 157 | box3d_type |
| G6 | type | geography |  | — | — | geography |
| G6 | type | geometry |  | — | — | geometry |
| G6 | type | geometry_dump |  | — | — | geometry_dump |
| — | bbox | ST_EstimatedExtent |  | — | 135 | st_estimatedextent |
| — | constructor | ST_Letters |  | 0/2 | 297 | st_letters |
| — | editor | ST_ForceSFS |  | — | 270 | st_forcesfs |
| — | input | ST_GeomFromMARC21 |  | 0/3 | 287 | st_geomfrommarc21 |

## Totals

| Group | Functions | Implemented |
|---|---|---|
| G1 | 144 | 34 |
| G1+G5 | 2 | 0 |
| G2 | 12 | 3 |
| G3 | 56 | 17 |
| G3+G5 | 2 | 0 |
| G4 | 44 | 10 |
| G5 | 29 | 2 |
| G6 | 39 | 4 |
| — | 4 | 0 |
