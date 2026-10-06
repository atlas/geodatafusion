# G4: Serialization and encodings

Text and binary geometry formats: parsing them into GeoArrow geometries (PostGIS "Geometry
Input") and writing GeoArrow geometries out (PostGIS "Geometry Output"). Every function in the
group is a scalar UDF that loops over one geometry or string column and calls a format codec.
The codecs are the part that needs care: PostGIS output is compared verbatim, so number
formatting, dimension tags and EMPTY forms must match byte for byte.

Findings in this plan were checked against PostGIS 3.6.4 (`psql` on the dev server) and the
crate sources in `~/.cargo/registry`.

> **Reconciled.** This plan was written in parallel with the other group plans. Where it
> conflicts with the cross-group decisions in [README.md](README.md#cross-group-decisions)
> (shared helpers, row access, signatures, argument readers, error and documentation
> conventions, function assignments, output encoding), the README and
> [STYLE_GUIDE.md](../STYLE_GUIDE.md) win. Several of those decisions come from the experiments in
> [hypotheses.md](hypotheses.md), which overturned parts of this plan.

## 1. Scope

### Final function list

| Function(s) | Direction | Status | Notes |
|---|---|---|---|
| ST_GeomFromText (aliases ST_GeometryFromText, ST_WKTToSQL) | in | exists, rewrite | Gains `srid`. Accepts EWKT, as PostGIS does. |
| ST_GeomFromEWKT (alias GeomFromEWKT) | in | new | Same parser as ST_GeomFromText. The legacy alias is used by `st_asencodedpolyline` docs. |
| ST_PointFromText, ST_LineFromText, ST_PolygonFromText (ST_PolyFromText), ST_MPointFromText (ST_MultiPointFromText), ST_MLineFromText (ST_MultiLineStringFromText), ST_MPolyFromText (ST_MultiPolygonFromText), ST_GeomCollFromText | in | new | Type-checked: return NULL for the wrong type. |
| ST_GeomFromWKB (alias ST_WKBToSQL) | in | exists, rewrite | Gains `srid`. Accepts EWKB and keeps its SRID. |
| ST_GeomFromEWKB (alias GeomFromEWKB) | in | new | Unblocks the harness's hex literals. |
| ST_PointFromWKB, ST_LineFromWKB (ST_LinestringFromWKB), ST_PolyFromWKB (ST_PolygonFromWKB), ST_MPointFromWKB (ST_MultiPointFromWKB), ST_MLineFromWKB (ST_MultiLineFromWKB), ST_MPolyFromWKB (ST_MultiPolyFromWKB), ST_GeomCollFromWKB | in | new | Type-checked, NULL for the wrong type. Only PointFromWKB/LineFromWKB/LinestringFromWKB are in the inventory, the rest exist in PostGIS without doc pages. |
| ST_GeomFromGeoJSON | in | new | |
| ST_GeomFromTWKB | in | new | |
| ST_LineFromEncodedPolyline | in | new | |
| ST_PointFromGeoHash, ST_Box2dFromGeoHash | in | exist, rewrite | Native decoder, `precision` argument. |
| ST_GeomFromGeoHash | in | new | |
| ST_GeogFromText (ST_GeographyFromText), ST_GeogFromWKB | in | blocked on G6 | Thin wrappers once the `geography` representation exists. |
| ST_AsText | out | exists, rewrite | Plain `Utf8`, PostGIS writer, `maxdecimaldigits`. |
| ST_AsEWKT | out | new | |
| ST_AsBinary | out | exists, rewrite | Plain `Binary`, endianness argument. |
| ST_AsEWKB, ST_AsHEXEWKB | out | new | |
| ST_AsGeoJSON (geometry form) | out | new | The `record` form is skipped (see below). |
| ST_AsSVG | out | new | |
| ST_AsKML | out | new | SRID 4326 input only until G3 has ST_Transform. |
| ST_AsGML | out | new | |
| ST_AsTWKB (single geometry form) | out | new | |
| ST_AsEncodedPolyline | out | new | |
| ST_GeoHash | out | exists, rewrite | Native encoder, any geometry, `maxchars`. |
| ST_AsLatLonText | out | new | |

### Reassigned

| Function | To | Why |
|---|---|---|
| ST_AsMVT, ST_AsGeobuf, ST_AsFlatGeobuf | G5 | Aggregates in PostGIS. The G5 plan already took them; G4 provides the encoders when they are scheduled. |
| ST_FromFlatGeobuf, ST_FromFlatGeobufToTable | G5 (skip) | Set-returning / DDL with a row type taken from a table. `geodatafusion-flatgeobuf` already covers reading FlatGeobuf as a table. |
| ST_AsMVTGeom | G3 | Clips to a box, snaps to a grid and makes the result valid. That's GEOS work, the "MVT" part is only a coordinate transform. |
| ST_BdPolyFromText, ST_BdMPolyFromText | G3 | They polygonize a MULTILINESTRING (ST_BuildArea). The WKT parsing is the trivial part. |

### Skipped or deferred

| Function | Decision | Why |
|---|---|---|
| ST_GeomFromGML, ST_GMLToSQL, ST_GeomFromKML | defer | Need an XML parser. No XML crate in the workspace; see open question 4. |
| ST_AsX3D | defer (last) | Its doc tests are about POLYHEDRALSURFACE and TIN, which GeoArrow can't represent. |
| ST_AsMARC21, ST_GeomFromMARC21 | skip | Library-catalogue format, XML, almost no users. |
| ST_AsGeoJSON(record, ...) | skip | Takes a whole row (`t.*`). Same blocker as ST_AsMVT in G5. |
| ST_AsTWKB(geometry[], bigint[], ...) | skip | Array form; rarely used. |
| Curves, TRIANGLE, TIN, POLYHEDRALSURFACE in any format | NotImplemented | No GeoArrow representation. Parsers return a `NotImplemented` error naming the type. |

## 2. Existing basis

### How the current functions work

- `native/io/wkt.rs` and `native/io/wkb.rs` hold one input and one output function each.
  - **ST_AsText** (`wkt.rs:18-84`) decodes any GeoArrow array and calls
    `geoarrow_array::cast::to_wkt`, which writes with the `wkt` crate. The result is a
    `geoarrow.wkt` extension column carrying the input CRS.
  - **ST_AsBinary** (`wkb.rs:18-80`) does the same with `to_wkb`, returning `geoarrow.wkb`.
  - **ST_GeomFromText** (`wkt.rs:86-180`) and **ST_GeomFromWKB** (`wkb.rs:82-175`) match on
    the string/binary data type, wrap the array as `WktArray`/`WkbArray` and call
    `geoarrow_array::cast::from_wkt`/`from_wkb`, which parse with the `wkt`/`wkb` crates and
    push into a `GeometryBuilder`.
- `udf/geohash/` wraps the `geohash` crate (0.13.1).
  - **ST_GeoHash** (`geohash.rs`) accepts only native XY `PointType` and encodes 12 characters.
  - **ST_PointFromGeoHash** (`point_from_geohash.rs`) and **ST_Box2dFromGeoHash**
    (`box2d_from_geohash.rs`) decode with `geohash::decode`/`decode_bbox`.
- Python bindings exist for all six (`python/src/udf/native/io.rs`, `python/src/udf/geohash.rs`).

### Parity today

All G4 doc-test files are 0/n except `st_geomfromtext` (4/7). In a full `cargo slt -v` run,
the first error of 58 records is "Invalid function 'st_asewkt'" and of 40 records "Invalid
function 'st_geomfromewkt'". These two functions block more records than any other missing
function in the suite. The doc-test SQL also has 92 lines with implicit-dimension coordinates
(`'LINESTRING(1 2 3, 4 5 6)'`), which the current parser rejects.

### Bugs found while probing

| Where | Bug | PostGIS |
|---|---|---|
| ST_AsText (`wkt` crate writer) | `POINT Z(1 2 3)`, shortest round-trip digits (`2.3076923076923075`), `1e15` printed as `1000000000000000` | `POINT Z (1 2 3)`, `2.307692307692308`, `1e+15` |
| ST_AsText | panics on a MULTIPOINT with an EMPTY member (`wkt/src/to_wkt/geo_trait_impl.rs:161,165`, `unwrap()`) | `MULTIPOINT((1 2),EMPTY)` |
| ST_AsBinary | Returns `geoarrow.wkb`, so the harness renders it as EWKT instead of bytes | `bytea` |
| ST_GeomFromText (`wkt` crate parser) | Rejects `POINT(1 2 3)`, `SRID=4326;POINT(1 2)`, `MULTIPOINT(1 2, EMPTY)`; accepts `POINT(1 2)x`, `LINESTRING(0 0)`, unclosed rings | 3 coords = Z, 4 = ZM; EWKT accepted; trailing text, 1-point lines and unclosed rings are errors |
| ST_GeomFromText | `GEOMETRYCOLLECTION(POINT Z (1 2 3), POINT(1 2))` panics in `geoarrow-array` (`builder/point.rs:99`) | Error "Dimensions mismatch" |
| ST_GeomFromText/WKB | `GEOMETRYCOLLECTION(POINT(1 2))` becomes `POINT(1 2)` (`geoarrow-array/src/builder/geometry.rs:642`, still in 0.9.0) | Kept as a collection |
| ST_GeomFromText/WKB | No `srid` argument; EWKB SRIDs silently dropped | `srid` overrides, EWKB SRID kept |
| ST_GeoHash | 12 characters max (crate limit), only native XY points, panics on POINT EMPTY (`geohash.rs:86`) | 20 characters for points, any geometry (bbox-derived precision), NULL for EMPTY |
| ST_PointFromGeoHash, ST_Box2dFromGeoHash | Error on hashes longer than 12 characters; no `precision` argument | Any length, `precision` truncates |

### Inconsistencies between the existing functions

| Topic | Variants found |
|---|---|
| Logic placement | Inherent `invoke_with_args` method on the struct, shadowing the trait method (`wkt.rs:26`, `wkt.rs:106`, `wkb.rs:106`); inline in the trait method (`wkb.rs:61-67`); free `<name>_impl` function as the style guide asks (`geohash.rs:59`, `point_from_geohash.rs:63`). The box2d one is named `box_from_geohash_impl` (`box2d_from_geohash.rs:83`), not after its file. |
| Signature storage | Struct field (`wkt.rs:96`, `wkb.rs:92`, `point_from_geohash.rs:28`) vs `static LazyLock` (`box2d_from_geohash.rs:35`, `geohash.rs:34`). |
| Static names | `AS_TEXT_DOC`, `GEOM_FROM_TEXT_DOC`, `AS_BINARY_DOC`, `GEOM_FROM_WKB_DOC`, `GEOHASH_DOC`, `GEOHASH_SIGNATURE` vs `DOCUMENTATION`/`SIGNATURE`. |
| Return field | `Metadata::try_from(..)?` (`wkt.rs:58`, `wkt.rs:157`, `wkb.rs:156`) vs `.unwrap_or_default()` (`wkb.rs:50`, `box2d_from_geohash.rs:78`, `point_from_geohash.rs:84`). Field named after the input field (`wkt.rs:60`, `wkt.rs:160`, `wkb.rs:52`, `wkb.rs:159`) vs `""` elsewhere in the crate. |
| Text output type | `geoarrow.wkt` on `Utf8` (ST_AsText) vs plain `Utf8View` (ST_GeoHash, `geohash.rs:56`). |
| Error conversion | `.map_err(GeoDataFusionError::GeoArrow)` (`wkb.rs:64-65`) vs `?` everywhere else. |
| Panics on input paths | `.unwrap()` on the first argument (`geohash.rs:80`, `box2d_from_geohash.rs:87`, `point_from_geohash.rs:93`), on `coord()` (`geohash.rs:86`); `unreachable!()` on the data type (`wkt.rs:123`, `wkb.rs:123`, `box2d_from_geohash.rs:95`, `point_from_geohash.rs:100`). |
| Struct doc comments | None of the six structs has one. |
| Documentation | Argument names don't follow PostGIS: `g1` (`wkt.rs:80`, `wkb.rs:76`), `("g1", "geometry")` for a text input (`wkt.rs:175`), `("geom", "WKB buffers")` (`wkb.rs:170`), name and type swapped in `("text", "geohash")` (`box2d_from_geohash.rs:71`, `point_from_geohash.rs:74`). ST_GeomFromWKB's description mentions an SRID argument that doesn't exist (`wkb.rs:169`). |
| Fully qualified paths | `datafusion::error::Result<DataType>` (`wkt.rs:52`, `wkt.rs:151`, `geohash.rs:55`) next to an imported `Result`. |
| Naming slips | `wkb_type` for a WKT type (`wkt.rs:59`), `rect_arr` for points (`point_from_geohash.rs:96`), `mod tests` (`box2d_from_geohash.rs:113`, `point_from_geohash.rs:123`) vs `mod test`, `geohash/geohash.rs` needing `#[allow(clippy::module_inception)]` (`geohash/mod.rs:2`). |
| Tests | `test_from_text` asserts nothing, it only calls `show()` (`wkt.rs:225-237`). |
| Struct name casing | `Box2DFromGeoHash` vs PostGIS `ST_Box2dFromGeoHash`. |

## 3. Refactoring assessment

### R1. Text and binary outputs return plain `Utf8` and `Binary` (do)

PostGIS returns `text` and `bytea`. Today ST_AsText returns `geoarrow.wkt` and ST_AsBinary
`geoarrow.wkb`, both carrying the input CRS.

- For: matches PostGIS and `datafusion-functions` (`to_hex`, `to_char`, `encode` return `Utf8`).
  The harness renders `geoarrow.wkb` as geometry, so ST_AsBinary can never pass its doc tests
  with the extension type. ST_AsEWKT (`SRID=...;` prefix), ST_AsGeoJSON etc. aren't valid
  `geoarrow.wkt` anyway, so the group can only be consistent with plain types. A
  `maxdecimaldigits`-rounded WKT tagged as a geometry column would silently lose precision when
  written to GeoParquet.
- Against: breaking change; users who wrote `ST_AsBinary(geom)` to GeoParquet lose the
  geometry tag. Plain `Utf8`/`Binary` input is still accepted as WKT/WKB by every geometry
  function (`any_geometry_type()` includes them), so chaining `ST_AsText` into another
  function keeps working.
- `Utf8`, not `Utf8View`, for every text output, including ST_GeoHash.

Effort S. Risk low. Update the comment in `tests/sqllogictests/datafusion_engine.rs:94-96`,
which cites ST_AsText as a `geoarrow.wkt` producer.

### R2. Shared codec and driver module `native/io/util/` (do)

Every output function is "for each geometry, write text or bytes, or NULL"; every input
function is "for each string or byte buffer, parse a geometry, or NULL". Today each file
re-implements the data-type match. Proposed shared pieces (signatures in section 4):

- `write_text_array` / `write_binary_array`: loop over any GeoArrow array through
  `downcast_geoarrow_array!`, so WKB and WKT inputs are read directly without converting to
  native arrays first.
- `parse_text_array` / `parse_binary_array`: loop over `Utf8`/`LargeUtf8`/`Utf8View` or
  `Binary`/`LargeBinary`/`BinaryView`, build a `GeometryArray` with `push_geometry(..)?` (no
  `from_nullable_geometries`, which `unwrap()`s on errors).
- `write_number`: the PostGIS coordinate formatter, used by every text format.
- `write_wkt` (ISO and EWKT flavours), `parse_ewkt`, `write_ewkb`, `ewkb_srid`.
- `ExpectedType` for the type-checked constructors; `IntArg`/`TextArg` for optional arguments.
- `geohash::{encode, decode_bbox, precision}`.

Effort M. Risk low; it's new code behind the existing functions.

### R3. A PostGIS-compatible (E)WKT parser instead of `from_wkt` (do)

The `wkt` crate (0.14) can't parse implicit dimensions (`POINT(1 2 3)`), SRID prefixes or
`EMPTY` multipoint members, and accepts input PostGIS rejects (section 2). Options:

1. **Preprocess and call `wkt::Wkt::from_str`.** Stripping `SRID=n;` is trivial, but implicit
   dimensions need a full tokenizer pass to know the coordinate count. Not simpler than option 2.
2. **Own parser producing `wkt::Wkt<f64>` (recommended).** The `wkt::types` constructors are
   public (`Point::new(Option<Coord>, Dimension)`, `LineString::new(Vec<Coord>, Dimension)`,
   ...) and implement `geo-traits`, so the result feeds `GeometryBuilder::push_geometry`
   unchanged. About 400 lines with tests: tokenizer, recursive descent, dimension inference
   from the first coordinate, consistency checks (mixed dimensions, `LINESTRING` needs 2
   points, rings need 4 points and must be closed, nothing after the geometry).
3. **Upstream to `wkt`.** Implicit dimensions and `EMPTY` multipoint members are reasonable PRs,
   but SRID prefixes and PostGIS's validation rules aren't the crate's job. Worth filing in
   addition, not instead.

```rust
// Before (wkt.rs:106-127)
let geom_arr = match field.data_type() {
    DataType::Utf8 => from_wkt(&WktArray::try_from((array.as_ref(), field.as_ref()))?, to_type),
    ...
    _ => unreachable!(),
}?;

// After
let result = parse_text_array(&arrays[0], typ, |text| {
    let (srid, geom) = parse_ewkt(text, "ST_GeomFromText")?;
    check_row_srid(srid, planned_srid, srid_argument_given, "ST_GeomFromText")?;
    Ok(Some(geom))
})?;
```

Effort M. Risk medium: the harness rewrites every `'...'::geometry` literal into
ST_GeomFromText/ST_GeomFromEWKT, so a parser bug fails tests across all groups. Mitigation: a
hand-written `geodatafusion/st_geomfromtext.slt` covering the grammar, round-trip tests, and a
full `cargo slt` before merging. Upside of the same coupling: implicit-Z literals start working
for every group at once.

The binary side keeps the `wkb` crate reader (`wkb::reader::read_wkb`), which already handles
EWKB Z/M/SRID flags. Only the SRID needs reading separately (`ewkb_srid`, a 9-byte header read,
as in `tests/sqllogictests/render.rs:84-101`).

### R4. Own writers, not `wkt`/`geozero`/`geoarrow-geojson` (do)

None of the available writers produce PostGIS text:

- `wkt` 0.14 formats numbers with `Display`, writes `POINT Z(`, and `unwrap()`s on empty
  multipoint members. Its writer functions are generic over `T: WktNum + Display`, so a
  formatting newtype would need the whole `num_traits::Num` surface. Not worth it.
- `geozero` 0.14/0.15 (pulled in transitively by `geoarrow-flatgeobuf`, not by the core crate)
  has WKT, GeoJSON and SVG writers, all with `Display` formatting and no `maxdecimaldigits`.
  Its MVT feature needs `prost-build`.
- `geoarrow-geojson` 0.8 encoders `expect()` on POINT EMPTY and use `write!(out, "{}", ..)`.

Writing to PostGIS's format directly over `geo-traits` is 100-200 lines per format and the only
way to get verbatim parity. `wkb::writer::write_geometry` *is* suitable for ISO WKB (it takes
`WriteOptions { endianness }`); EWKB needs its own small writer because the `wkb` crate only
writes ISO type codes.

### R5. Native GeoHash, moved to `native/io`, `geohash` crate dropped (do)

The `geohash` crate interleaves bits in a `u64`, so it's capped at 12 characters. PostGIS
encodes points with 20 characters by default and the doc tests decode 22-character hashes. The
PostGIS algorithm is a few dozen lines of double-precision bisection (`lwgeom_geohash.c`), and
reproducing it exactly also reproduces PostGIS's floating-point results
(`POINT(-115.17281600000001 36.11464599999999)`), which ST_AsText compares verbatim.

With no crate left to wrap, the `geohash/` provider no longer fits the layout rule (providers
are implementation mechanisms). Move the three functions to `native/io/`, remove the `geohash`
dependency and `GeoDataFusionError::GeoHash`, and move the Python classes from
`geodatafusion.geohash` to `geodatafusion.native`. Rename `Box2DFromGeoHash` to
`Box2dFromGeoHash` in the same breaking change (open question 5).

```rust
// Before (geohash.rs:84-89)
let coord = point?.coord().unwrap();
// TODO: make arg
let s = geohash::encode(coord.to_coord(), 12)?;

// After (native/io/geo_hash.rs, through write_text_array)
impl TextWriter for GeoHashWriter<'_> {
    fn write(&self, row: usize, geom: &impl GeometryTrait<T = f64>, out: &mut String) -> GeoDataFusionResult<bool> {
        let Some(bounds) = bounding_rect(geom) else {
            // PostGIS returns NULL for EMPTY input.
            return Ok(false);
        };
        let Some(max_chars) = self.max_chars.get(row) else { return Ok(false) };
        geohash::encode_bounds(out, &bounds, max_chars, "ST_GeoHash")?;
        Ok(true)
    }
}
```

Effort S. Risk low (Rust and Python import paths change).

### R6. Standard anatomy for the six existing functions (do)

Apply the style guide: free `<file>_impl` functions, `DOCUMENTATION`/`SIGNATURE` statics, struct
doc comments, PostGIS argument names and SQL types in the documentation, no `unwrap()`/
`unreachable!()` on input paths (use `take_function_args` as G1's R6 proposes), `""` field
names, `mod test` with assertions. This happens naturally when R1-R5 rewrite the files. Effort
S. Risk none.

### R7. Harness: render `bytea` the same on both engines (do, prerequisite)

The PostGIS engine renders `bytea` through `render::text` (`postgis.rs:108`), which escapes the
backslash, so recorded expectations read `\\x0103...`. The geodatafusion engine renders
`Binary` with `render::bytes` (`datafusion_engine.rs:168-170`), giving `\x0103...`. Even a
correct ST_AsBinary can't pass. Fix on the geodatafusion side, `render::text(&render::bytes(..))`,
so no expectations need re-recording. Effort S. Risk none. Coordinate with G6 if they own the
harness.

### R8. Upstream fixes in geoarrow-rs (file now, work around meanwhile)

- `GeometryBuilder::push_geometry` replaces a one-element GEOMETRYCOLLECTION with its element
  (`builder/geometry.rs:642`, 0.8.0 and 0.9.0). Propose making it opt-in like
  `with_prefer_multi`. G1 hits the same bug.
- Mixed-dimension collections panic in `PointBuilder::push_point` (`builder/point.rs:99`)
  instead of returning an error. Our parser rejects them first, but other paths (WKB input) can
  still reach it.

There's no local workaround for the first: `push_geometry_collection` is private. Until fixed,
it's a documented parity gap. Effort S upstream; timeline external.

### Not recommended

- **A macro for the 14 type-checked constructors.** They'd be near-identical ~60-line structs.
  A declarative macro would hide the shape every UDF is meant to share, and DataFusion doesn't
  macro-generate UDF impls either (G1 R12 reaches the same conclusion). Keep explicit structs
  that delegate to one shared `geom_from_text_impl`/`geom_from_wkb_impl`.
- **A single parameterised struct for all `*FromText` variants.** Breaks the "one struct per
  PostGIS function, one Python class per struct" rule.

### Summary

| # | Proposal | Effort | Risk | Recommendation |
|---|---|---|---|---|
| R1 | Plain `Utf8`/`Binary` outputs | S | low | do |
| R2 | `native/io/util/` codecs and drivers | M | low | do |
| R3 | Own (E)WKT parser | M | medium | do |
| R4 | Own writers for every text format, EWKB | M | low | do |
| R5 | Native GeoHash in `native/io`, drop crate | S | low | do |
| R6 | Standard anatomy for existing functions | S | none | do |
| R7 | Harness `bytea` rendering | S | none | do first |
| R8 | geoarrow-rs fixes | S | external | file issues |

## 4. Canonical templates

### Layout

```
rust/geodatafusion/src/udf/native/io/
├── mod.rs
├── as_text.rs               ST_AsText, ST_AsEWKT
├── as_binary.rs             ST_AsBinary, ST_AsEWKB, ST_AsHEXEWKB
├── geom_from_text.rs        ST_GeomFromText, ST_GeomFromEWKT and the 7 type-checked *FromText
├── geom_from_wkb.rs         ST_GeomFromWKB, ST_GeomFromEWKB and the 7 type-checked *FromWKB
├── geo_hash.rs              ST_GeoHash
├── geom_from_geo_hash.rs    ST_PointFromGeoHash, ST_GeomFromGeoHash, ST_Box2dFromGeoHash
├── as_geojson.rs            geom_from_geojson.rs
├── as_svg.rs  as_kml.rs  as_gml.rs  as_lat_lon_text.rs
├── as_twkb.rs               geom_from_twkb.rs
├── as_encoded_polyline.rs   line_from_encoded_polyline.rs
└── util/
    ├── mod.rs
    ├── args.rs              IntArg, TextArg, Endianness parsing
    ├── driver.rs            write_text_array, write_binary_array, parse_text_array,
    │                        parse_binary_array, ExpectedType
    ├── number.rs            write_number, write_fixed, DEFAULT_MAX_DECIMAL_DIGITS
    ├── wkt.rs               write_wkt, WktFlavor, parse_ewkt, split_srid_prefix
    ├── wkb.rs               write_ewkb, ewkb_srid
    └── geohash.rs           encode, decode_bbox, precision
```

Files named after the main function; WKT/EWKT and the type-checked families share a file as
"closely related variants", with a `//!` module doc. `wkt.rs` and `wkb.rs` become `as_text.rs`,
`as_binary.rs`, `geom_from_text.rs` and `geom_from_wkb.rs`.

### Shared helpers (`native/io/util/`, all `pub(crate)`)

```rust
// number.rs

/// PostGIS's default `maxdecimaldigits` for WKT, KML, GML and SVG.
pub(crate) const DEFAULT_MAX_DECIMAL_DIGITS: i64 = 15;

/// Writes `value` the way PostGIS's `lwprint_double` does.
///
/// Takes the shortest round-trip decimal digits, rounds them half-to-even to at most
/// `max_decimal_digits` decimals (negative counts as 0), and drops trailing zeros. Values with
/// `1e-8 < |value| < 1e15` use fixed notation, others exponential (`1e+15`, `1.5e-9`). Zero, and
/// anything that rounds to zero, is `0` without a sign. NaN and infinities are `NaN`,
/// `Infinity` and `-Infinity`.
pub(crate) fn write_number(out: &mut String, value: f64, max_decimal_digits: i64);

/// Writes `value` with exactly `decimals` decimals (C's `%.*f`). ST_AsGeoJSON's `bbox` uses it.
pub(crate) fn write_fixed(out: &mut String, value: f64, decimals: usize);
```

> **Correction (phase 2):** the rule below misses two cases. A carry out of the mantissa in
> exponential notation keeps the exponent (`9.99e-9` with 0 decimals is `10e-9`), and of several
> shortest round-trip digit strings PostGIS picks the closest, ties to even. The implementation
> (`native/io/util/number.rs`) is checked against 3,471 cases recorded from PostGIS.

The `write_number` rule was reverse-engineered and checked: a prototype matched PostGIS on all
5,514 cases tried (random magnitudes 1e-12 to 1e17, ties, `maxdecimaldigits` -1 to 20). Notable
cases: `0.45` at 1 decimal is `0.4` and `2.675` at 2 is `2.68` (rounding the *shortest* digits,
not the exact binary value); `2.3076923076923075` is `2.307692307692308`; `0.30000000000000004`
at 17 decimals stays as is; `999999999999999.9` at 0 decimals is `1000000000000000` (the
notation is picked before rounding). Rust's `format!("{:e}", v)` yields the shortest digits; the
rounding is done on the digit string, not with `{:.N}` (exact rounding, which differs).

```rust
// wkt.rs

pub(crate) enum WktFlavor {
    /// ISO WKT as ST_AsText writes it: `POINT Z (1 2 3)`, `MULTIPOINT((1 2),EMPTY)`,
    /// `POINT Z EMPTY`, every collection member tagged.
    Iso,
    /// PostGIS EWKT as ST_AsEWKT writes it: no Z tag, glued M tag (`POINTM(1 2 3)`,
    /// `GEOMETRYCOLLECTIONM(POINTM(1 2 3))`), `MULTIPOINT(1 2,EMPTY)`, `POINT EMPTY` for Z/ZM,
    /// `POINTM EMPTY` for M. The `SRID=n;` prefix is written by the caller.
    Extended,
}

pub(crate) fn write_wkt(
    out: &mut String,
    geom: &impl GeometryTrait<T = f64>,
    flavor: WktFlavor,
    max_decimal_digits: i64,
) -> GeoDataFusionResult<()>;

/// Parses (E)WKT the way PostGIS does. Returns the SRID from a `SRID=n;` prefix, if any.
/// `function` names the SQL function in error messages.
pub(crate) fn parse_ewkt(text: &str, function: &str) -> GeoDataFusionResult<(Option<i32>, Wkt<f64>)>;

/// Splits off a `SRID=n;` prefix without parsing the geometry. Used at planning time on
/// literal arguments.
pub(crate) fn split_srid_prefix(text: &str) -> (Option<i32>, &str);

// wkb.rs

/// Writes EWKB: Z/M/SRID flags (0x80000000, 0x40000000, 0x20000000) in the type code instead
/// of ISO's +1000/2000/3000. Only the top level carries the SRID; members carry Z/M flags.
pub(crate) fn write_ewkb(
    out: &mut Vec<u8>,
    geom: &impl GeometryTrait<T = f64>,
    srid: Option<i32>,
    endianness: wkb::Endianness,
) -> GeoDataFusionResult<()>;

/// The SRID in an EWKB header, if the SRID flag is set.
pub(crate) fn ewkb_srid(buf: &[u8]) -> Option<i32>;

// args.rs

/// An optional `integer` argument: the broadcast array if the call has it, else the PostGIS
/// default. `get` returns `None` for SQL NULL.
pub(crate) struct IntArg<'a> { values: Option<&'a Int64Array>, default: i64 }
impl<'a> IntArg<'a> {
    pub(crate) fn new(arrays: &'a [ArrayRef], index: usize, default: i64) -> GeoDataFusionResult<Self>;
    pub(crate) fn get(&self, row: usize) -> Option<i64>;
}
// TextArg is the same for `text` arguments (endianness, nprefix, tmpl).

/// PostGIS's endianness argument: `'XDR'`/`'xdr'` is big-endian, anything else NDR.
pub(crate) fn parse_endianness(value: &str) -> wkb::Endianness;

// driver.rs

/// Writes one geometry as text. Implemented by each output function.
pub(crate) trait TextWriter {
    /// Writes row `row`. Returns `Ok(false)` if the result is SQL NULL (a NULL argument, or a
    /// geometry PostGIS maps to NULL).
    fn write(&self, row: usize, geom: &impl GeometryTrait<T = f64>, out: &mut String) -> GeoDataFusionResult<bool>;
}

/// The same for binary formats.
pub(crate) trait BinaryWriter {
    fn write(&self, row: usize, geom: &impl GeometryTrait<T = f64>, out: &mut Vec<u8>) -> GeoDataFusionResult<bool>;
}

/// Applies `writer` to every geometry in `array`, whatever its GeoArrow encoding.
pub(crate) fn write_text_array(array: &dyn GeoArrowArray, writer: &impl TextWriter) -> GeoDataFusionResult<StringArray> {
    downcast_geoarrow_array!(array, impl_write_text_array, writer)
}

fn impl_write_text_array<'a>(
    array: &'a impl GeoArrowArrayAccessor<'a>,
    writer: &impl TextWriter,
) -> GeoDataFusionResult<StringArray> {
    let mut builder = StringBuilder::with_capacity(array.len(), 0);
    let mut buffer = String::new();
    for (row, item) in array.iter().enumerate() {
        match item {
            Some(geom) => {
                buffer.clear();
                if writer.write(row, &geom?, &mut buffer)? {
                    builder.append_value(&buffer);
                } else {
                    builder.append_null();
                }
            }
            // SQL NULL in, SQL NULL out.
            None => builder.append_null(),
        }
    }
    Ok(builder.finish())
}

pub(crate) fn write_binary_array(array: &dyn GeoArrowArray, writer: &impl BinaryWriter) -> GeoDataFusionResult<BinaryArray>;

/// Parses every value of a string array. `parse` returns `Ok(None)` for SQL NULL, e.g. a
/// type-checked constructor given another geometry type.
pub(crate) fn parse_text_array(
    array: &ArrayRef,
    typ: GeometryType,
    parse: impl FnMut(&str) -> GeoDataFusionResult<Option<Wkt<f64>>>,
) -> GeoDataFusionResult<GeometryArray>;

pub(crate) fn parse_binary_array<'a>(
    array: &'a ArrayRef,
    typ: GeometryType,
    parse: impl FnMut(&'a [u8]) -> GeoDataFusionResult<Option<Wkb<'a>>>,
) -> GeoDataFusionResult<GeometryArray>;

// Both collect the parsed geometries, then build without `from_nullable_geometries`
// (it unwraps):
//     let capacity = GeometryCapacity::from_geometries(geoms.iter().map(Option::as_ref))?;
//     let mut builder = GeometryBuilder::with_capacity(typ, capacity);
//     for geom in &geoms { builder.push_geometry(geom.as_ref())?; }

/// The geometry type a type-checked constructor requires.
pub(crate) enum ExpectedType { Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon, GeometryCollection }
impl ExpectedType {
    pub(crate) fn matches(&self, geom: &impl GeometryTrait<T = f64>) -> bool; // via geom.as_type()
}
```

Writers append with `push_str` and `write_number`, never `write!`, so there's no
`fmt::Error` to handle.

Helpers expected from G6 (names are placeholders until the G6 plan settles them):
`geometry_and(overloads)` signatures in `data_types.rs` (as G1 R4 proposes),
`metadata_from_srid(i32) -> Metadata`, `srid_from_metadata(&Metadata) -> Option<i32>` and
`scalar_srid(args: &ReturnFieldArgs, index, function) -> GeoDataFusionResult<Option<i32>>`.
If they aren't there when batch 1 starts, put minimal versions in `io/util/` and move them.

### Output function (ST_AsEWKT)

```rust
use std::sync::{Arc, LazyLock, OnceLock};

use arrow_schema::DataType;
use datafusion::common::utils::take_function_args;
use datafusion::error::Result;
use datafusion::logical_expr::scalar_doc_sections::DOC_SECTION_OTHER;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::from_arrow_array;

use crate::data_types::geometry_and;
use crate::error::GeoDataFusionResult;
use crate::srid::srid_from_metadata;
use crate::udf::native::io::util::{
    DEFAULT_MAX_DECIMAL_DIGITS, IntArg, TextWriter, WktFlavor, write_number, write_text_array,
    write_wkt,
};

/// Returns the Well-Known Text (WKT) representation of the geometry with SRID metadata.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsEWKT;

impl AsEWKT {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for AsEWKT {
    fn default() -> Self {
        Self::new()
    }
}

static DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();
static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    // st_asewkt(geometry), st_asewkt(geometry, integer).
    geometry_and(&[&[], &[DataType::Int64]])
});

impl ScalarUDFImpl for AsEWKT {
    fn name(&self) -> &str {
        "st_asewkt"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_ewkt_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        Some(DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Returns the Well-Known Text (WKT) representation of the geometry with SRID meta data.",
                "ST_AsEWKT(geom, maxdecimaldigits)",
            )
            .with_argument("geom", "geometry")
            .with_argument("maxdecimaldigits", "integer, default 15")
            .with_related_udf("st_astext")
            .with_related_udf("st_geomfromewkt")
            .build()
        }))
    }
}

fn as_ewkt_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let arrays = ColumnarValue::values_to_arrays(&args.args)?;
    let geo_array = from_arrow_array(&arrays[0], &args.arg_fields[0])?;
    let writer = EwktWriter {
        srid: srid_from_metadata(geo_array.data_type().metadata()),
        max_decimal_digits: IntArg::new(&arrays, 1, DEFAULT_MAX_DECIMAL_DIGITS)?,
    };
    let result = write_text_array(geo_array.as_ref(), &writer)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct EwktWriter<'a> {
    srid: Option<i32>,
    max_decimal_digits: IntArg<'a>,
}

impl TextWriter for EwktWriter<'_> {
    fn write(
        &self,
        row: usize,
        geom: &impl GeometryTrait<T = f64>,
        out: &mut String,
    ) -> GeoDataFusionResult<bool> {
        let Some(max_decimal_digits) = self.max_decimal_digits.get(row) else {
            return Ok(false);
        };
        // PostGIS omits the prefix for SRID 0 (unknown).
        if let Some(srid) = self.srid {
            out.push_str("SRID=");
            out.push_str(&srid.to_string());
            out.push(';');
        }
        write_wkt(out, geom, WktFlavor::Extended, max_decimal_digits)?;
        Ok(true)
    }
}
```

Binary outputs are identical with `BinaryWriter`, `write_binary_array` and
`return_type → DataType::Binary`.

### Input function (ST_GeomFromText and a type-checked variant)

```rust
//! Constructors from Well-Known Text: ST_GeomFromText, ST_GeomFromEWKT and the type-checked
//! ST_PointFromText family, which return NULL when the text holds another geometry type.

use std::sync::{Arc, LazyLock, OnceLock};

use arrow_schema::{DataType, FieldRef};
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::scalar_doc_sections::DOC_SECTION_OTHER;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    TypeSignature, Volatility,
};
use geoarrow_array::GeoArrowArray;
use geoarrow_schema::{CoordType, GeometryType};

use crate::error::GeoDataFusionResult;
use crate::srid::{metadata_from_srid, scalar_srid, srid_from_metadata};
use crate::udf::native::io::util::{ExpectedType, parse_ewkt, parse_text_array, split_srid_prefix};

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    let mut variants = vec![];
    for text in [DataType::Utf8, DataType::LargeUtf8, DataType::Utf8View] {
        variants.push(TypeSignature::Exact(vec![text.clone()]));
        variants.push(TypeSignature::Exact(vec![text, DataType::Int64]));
    }
    Signature::one_of(variants, Volatility::Immutable)
});

/// Returns a geometry from Well-Known Text (WKT).
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromText {
    coord_type: CoordType,
    aliases: Vec<String>,
}

impl GeomFromText {
    pub fn new(coord_type: CoordType) -> Self {
        Self {
            coord_type,
            aliases: vec!["st_geometryfromtext".to_string(), "st_wkttosql".to_string()],
        }
    }
}

impl Default for GeomFromText {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

static GEOM_FROM_TEXT_DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();

impl ScalarUDFImpl for GeomFromText {
    fn name(&self) -> &str {
        "st_geomfromtext"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("return_type".to_string()))
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(return_field_impl(args, self.coord_type, "ST_GeomFromText")?)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_text_impl(args, None, "ST_GeomFromText")?)
    }

    fn documentation(&self) -> Option<&Documentation> { /* .with_argument("WKT", "text")
        .with_argument("srid", "integer") */ }
}

// PointFromText is the same struct shape with name "st_pointfromtext", no aliases, its own
// documentation static, and
//     geom_from_text_impl(args, Some(ExpectedType::Point), "ST_PointFromText")

/// The output field: a geometry whose CRS comes from the `srid` argument, else from a literal
/// EWKT argument's `SRID=n;` prefix, else is unknown. GeoArrow keeps the CRS per column, so a
/// per-row SRID can't be represented.
fn return_field_impl(
    args: ReturnFieldArgs,
    coord_type: CoordType,
    function: &str,
) -> GeoDataFusionResult<FieldRef> {
    let srid = match scalar_srid(&args, 1, function)? {
        Some(srid) => Some(srid),
        None => literal_text(args.scalar_arguments[0]).and_then(|text| split_srid_prefix(text).0),
    };
    let metadata = Arc::new(srid.map(metadata_from_srid).unwrap_or_default());
    let output_type = GeometryType::new(metadata).with_coord_type(coord_type);
    Ok(Arc::new(output_type.to_field("", true)))
}

fn geom_from_text_impl(
    args: ScalarFunctionArgs,
    expected: Option<ExpectedType>,
    function: &str,
) -> GeoDataFusionResult<ColumnarValue> {
    let arrays = ColumnarValue::values_to_arrays(&args.args)?;
    let output_type = args.return_field.try_extension_type::<GeometryType>()?;
    let output_srid = srid_from_metadata(output_type.metadata());
    let srid_argument_given = arrays.len() > 1;
    let result = parse_text_array(&arrays[0], output_type, |text| {
        let (srid, geom) = parse_ewkt(text, function)?;
        // The srid argument overrides an embedded SRID, as in PostGIS. Otherwise an embedded
        // SRID must match the column's, which planning took from a literal argument.
        if !srid_argument_given && srid.filter(|s| *s != 0) != output_srid {
            return Err(DataFusionError::Execution(format!(
                "{function} only supports a SRID prefix in a literal argument, got SRID={} in a column",
                srid.unwrap_or(0),
            ))
            .into());
        }
        Ok(expected.is_none_or(|e| e.matches(&geom)).then_some(geom))
    })?;
    Ok(ColumnarValue::Array(result.into_array_ref()))
}
```

Every input function in the group has this shape: a `return_field_impl` that fixes the CRS
(always 4326 for ST_GeomFromGeoJSON without a `crs` member and for
ST_LineFromEncodedPolyline; unknown for TWKB and GeoHash), and an `_impl` that calls
`parse_text_array` or `parse_binary_array` with a format-specific closure.

### Python bindings and docs, per function

`impl_udf!` (fixed return type) or `impl_udf_coord_type_arg!` (geometry return type) in
`python/src/udf/native/io.rs`, `m.add_class` under `// io` in `python/src/udf/native/mod.rs`, a
stub in `python/python/geodatafusion/native/_io.pyi`, and the README row.

## 5. Dependencies

### Crate APIs relied on (verified)

| Crate (locked version) | API | Use |
|---|---|---|
| geoarrow-array 0.8.0 | `array::from_arrow_array`, `downcast_geoarrow_array!`, `GeoArrowArrayAccessor::iter`, `builder::GeometryBuilder::{with_capacity, push_geometry, finish}`, `capacity::GeometryCapacity::from_geometries` | Drivers. `push_geometry` returns `GeoArrowResult`; `from_nullable_geometries`/`extend_from_iter` unwrap, so avoid them. Scalar `Point::coord()` is `None` for an all-NaN (EMPTY) point. |
| wkt 0.14.0 | `Wkt<f64>`, `types::{Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon, GeometryCollection}::new(.., Dimension)`, `types::{Coord, Dimension}` | Parser output; implements `geo-traits`. Its parser and writer aren't used. |
| wkb 0.9.1 | `reader::read_wkb` (handles EWKB Z/M/SRID flags), `writer::write_geometry(&mut impl Write, &impl GeometryTrait, &WriteOptions)`, `writer::WriteOptions { endianness }`, `Endianness::{BigEndian, LittleEndian}`, `error::WkbError` | WKB in/out. Move from `[dev-dependencies]` to `[dependencies]` (workspace dep, already in the tree through geoarrow-array). Add `GeoDataFusionError::Wkb(#[from] WkbError)`. |
| geojson 0.24.2 | `GeoJson::from_str`, `Geometry { value: Value, .. }`, `Value::{Point(Vec<f64>), ..}` | ST_GeomFromGeoJSON. Workspace dep, new to the core crate; use `default-features = false` (drops geo-types). Alternative: parse with `serde_json` directly (also a workspace dep). |
| datafusion 54.1 | `Signature::one_of`, `TypeSignature::Exact`, `Signature::with_parameter_names` (must match the longest overload), `ReturnFieldArgs::scalar_arguments`, `common::utils::take_function_args` | Signatures, SRID planning. `Exact` coerces narrower integers to `Int64` but not `Int64` literals to `Int32`, so `integer` parameters are `Int64`. |
| arrow 58 | `StringBuilder`, `BinaryBuilder`, `AsArray::{as_string, as_string_view, as_binary, as_binary_view, as_primitive}` | Drivers. |

Not used, and why: `geohash` (12-character cap, removed); `geozero` 0.14/0.15 (number
formatting, `prost-build` for MVT; would be a new core dependency); `geoarrow-geojson` 0.8
(panics on EMPTY, no precision); `polyline` 0.11, `rapidgeo-polyline` (not dependencies, and
encoded polyline is 40 lines); no TWKB crate exists on crates.io; `quick-xml` 0.42 only if GML/KML
input is wanted (open question 4).

### Other groups

| Group | Needed from them | For |
|---|---|---|
| G6 | SRID model: SRID ↔ CRS helpers. **Assumption:** SRID is column-level (GeoArrow CRS metadata), SRID n ↔ `Crs::from_authority_code("EPSG:n")` (as `ST_Point` does today) or `CrsType::Srid`, 0 ↔ no CRS. A per-row SRID in a column is an `Execution` error. | EWKT/EWKB in and out, every `srid` argument, GeoJSON `crs`, GML `srsName`, KML/EncodedPolyline 4326 checks |
| G6 | `geometry_and(...)` signature helper (proposed by G1) | Every output function with optional arguments |
| G6 | Decision on the input functions' output encoding (native `Geometry` union today; see open question 3) | All input functions |
| G6 | If plain `Utf8` stays a valid geometry input, route it through G4's `parse_ewkt` instead of the `wkt` crate, so `ST_AsText('POINT(1 2 3)')` behaves like PostGIS | Text → geometry coercion |
| G6 | `geography` type | ST_GeogFromText, ST_GeographyFromText, ST_GeogFromWKB |
| G6 | Casts `geometry::text` (HEXEWKB) and `box2d` text reuse G4's `write_ewkb` and `write_number` | `st_ashexewkb` doc test 2 |
| G3 | ST_Transform | ST_AsKML for SRIDs other than 4326 |
| G5 | Row-valued arguments | ST_AsGeoJSON(record), ST_AsMVT etc. |
| G1, G2, G3, G5 | Nothing, but they all depend on G4: ST_AsText/ST_AsEWKT wrap most doc-test outputs and the harness turns every geometry literal into ST_GeomFromText/EWKT/EWKB | — |
| upstream | geoarrow-rs fixes (R8) | GEOMETRYCOLLECTION fidelity |

## 6. Phasing

Doc-test counts are `0/n` today unless noted; "unlocks" means the G4 function no longer blocks
the record, not that every such record passes (many also need other groups).

| Batch | Contents | Doc tests | Depends on |
|---|---|---|---|
| 1. Refactor | R7 harness fix. `io/util` skeleton: drivers, `write_number`, `write_wkt` (ISO and EWKT), `args`. R1/R6: ST_AsText (`maxdecimaldigits`, Utf8), ST_AsBinary (endianness, Binary), ST_GeomFromText/ST_GeomFromWKB (standard anatomy, `srid`, still on `from_wkt`/`from_wkb` internally). R5: native GeoHash in `native/io` with `maxchars`/`precision`, ST_GeomFromGeoHash. | st_asbinary 2, st_geomfromtext +2 (9 records suite-wide fail on the missing `srid` overload), st_geohash 3, st_pointfromgeohash 3, st_box2dfromgeohash 3, st_geomfromgeohash 3, plus ST_AsText text diffs suite-wide | G6 SRID helper (or local stopgap) |
| 2. EWKT | R3 parser (`parse_ewkt`), switch ST_GeomFromText to it, ST_GeomFromEWKT (+ `geomfromewkt`), ST_AsEWKT. | First failure of 98 records (58 st_asewkt, 40 st_geomfromewkt); st_geomfromewkt 0/7 (2 are curves/polyhedral, unattainable); implicit-dimension literals in 92 doc SQL lines; G1 counts ~45 of its records | Batch 1 |
| 3. EWKB and typed constructors | ST_GeomFromEWKB (+ `geomfromewkb`), ST_AsEWKB, ST_AsHEXEWKB, ST_GeomFromWKB on `parse_binary_array` with EWKB SRIDs; the 7 `*FromText` and 7 `*FromWKB` variants. | st_asewkb 2, st_ashexewkb 1 of 2, st_geomfromwkb 1-2, st_pointfromtext 2, st_linefromtext 1, st_polygonfromtext 2, st_mpointfromtext 2, st_mlinefromtext 1, st_mpolyfromtext 2, st_geomcollfromtext 1, st_pointfromwkb 2, st_linefromwkb 1, st_linestringfromwkb 1; the harness's hex literals | Batch 2 |
| 4. Common web formats | ST_AsGeoJSON, ST_GeomFromGeoJSON, ST_AsEncodedPolyline, ST_LineFromEncodedPolyline, ST_AsTWKB, ST_GeomFromTWKB, ST_AsSVG, ST_AsLatLonText | st_asgeojson 2 of 4 (2 need the record form), st_geomfromgeojson 2, st_asencodedpolyline 1-2, st_linefromencodedpolyline 2, st_astwkb 1, st_geomfromtwkb 2, st_assvg 1 of 4 (3 are curves), st_aslatlontext 6 | Batch 1 |
| 5. OGC XML outputs | ST_AsKML (4326 only), ST_AsGML (versions 2 and 3, options bitmask) | st_askml 2, st_asgml 3 of 5 (1 polyhedral, 1 needs SRID lookup to match) | Batch 1; G3 for KML reprojection |
| 6. Blocked or deferred | ST_GeogFromText/ST_GeographyFromText/ST_GeogFromWKB; GML/KML input if an XML crate is accepted; ST_AsX3D last | st_geogfromwkb 1, st_geomfromgml 2 of 3, st_geomfromkml 1, st_asx3d 1 of 3 | G6 geography; open question 4 |

Batch 2 is the most valuable single change in the group and arguably in the project; it can
start as soon as `write_number`/`write_wkt` from batch 1 exist.

## 7. Per-function notes

| Function | Implementation | PostGIS format gotchas | Size | Doc tests |
|---|---|---|---|---|
| ST_AsText | `write_wkt(Iso)` | `maxdecimaldigits` default 15; `POINT Z (` with a space; members tagged (`GEOMETRYCOLLECTION M (POINT M (1 2 3))`); `MULTIPOINT((1 2),EMPTY)`; `POINT Z EMPTY`; NaN prints `NaN`; SRID dropped; `text` overload works through the Utf8 input | S | — |
| ST_AsEWKT | `write_wkt(Extended)` | `SRID=n;` unless 0; no Z tag, glued M (`POINTM(1 2 3)`); `MULTIPOINT(1 2,3 4)`; EMPTY loses Z (`POINT EMPTY`) but keeps M (`POINTM EMPTY`) | S | — (hand-written) |
| ST_AsBinary | `wkb::writer::write_geometry` | ISO type codes (Z = +1000); POINT EMPTY is NaN coordinates; `'XDR'`/`'xdr'` big-endian, anything else NDR | S | 0/2 |
| ST_AsEWKB | `write_ewkb` | Flags 0x80000000 Z, 0x40000000 M, 0x20000000 SRID; SRID only at top level | S | 0/2 |
| ST_AsHEXEWKB | `write_ewkb` + hex | Uppercase hex, `text` result; only exact `'XDR'`/`'xdr'` select big-endian (`'xDr'` is NDR) | S | 0/2 (one is a `::text` cast, G6) |
| ST_GeomFromText, ST_GeomFromEWKT | `parse_ewkt` | 3 coordinates = Z, 4 = ZM, `POINTM`/`POINT M` = M; EWKT accepted by both; `srid` overrides embedded SRID; errors: mixed dimensions, `LINESTRING` < 2 points, rings < 4 points or not closed, trailing text; NaN accepted; curves NotImplemented; one-member GEOMETRYCOLLECTION collapses until R8 | M | 4/7, 0/7 |
| *FromText (7) | `parse_ewkt` + `ExpectedType` | Wrong type returns NULL, not an error; invalid text still errors; ST_GeomCollFromText accepts only collections; return type stays `Geometry` (dimension unknown at planning) | S each | 0/1-0/2 each |
| ST_GeomFromWKB, ST_GeomFromEWKB | `read_wkb` + `ewkb_srid` | EWKB accepted by both; embedded SRID kept unless `srid` given; truncated input errors. The st_geomfromwkb doc test passes an `E'\\001...'` escape string where PostGIS casts to `bytea`; DataFusion can't do that cast | S | 0/2, 0/1 |
| *FromWKB (7) | `read_wkb` + `ExpectedType` | Wrong type returns NULL | S each | 0/1-0/2 |
| ST_GeomFromGeoJSON | geojson crate | Default SRID 4326; `crs` member sets SRID (literal arguments only); 3 positions = Z, a 4th is dropped; Feature objects are an error; `"coordinates":[]` is EMPTY | M | 0/2 |
| ST_AsGeoJSON | from scratch | `maxdecimaldigits` default 9, `options` default 8; compact (`{"type":"Point","coordinates":[1,2]}`); M dropped; EMPTY is `"coordinates":[]` / `"geometries":[]`; `bbox` (option 1) uses fixed decimals, not trimmed (`1.000000000`); `crs` short `EPSG:n` (2), long `urn:ogc:def:crs:EPSG::n` (4), short only when SRID ≠ 4326 (8) | M | 0/4 |
| ST_AsSVG | from scratch | Y negated; points `cx="1" cy="-2"` (rel 0) or `x="1" y="-2"` (rel 1); multipoint members comma-separated; lines `M 1 -2 L 3 -4`, relative `l dx dy`; polygons end in ` Z`; collection members `;`-separated; EMPTY is `''` | M | 0/4 (3 curves) |
| ST_AsKML | from scratch | Errors for SRID 0; transforms to 4326 (G3); `x,y[,z]` tuples space-separated; `nprefix` | S | 0/2 |
| ST_AsGML | from scratch | Version 2 `<gml:coordinates>1,2 3,4` vs 3 `<gml:pos>`/`<gml:posList srsDimension="2">`; `srsName="EPSG:n"` or long form (option 1); option 2 drops `srsDimension`, 4 writes `<LineString>` instead of `<Curve>`, 16 lat/lon order, 32 envelope; `nprefix`, `id` | L | 0/5 |
| ST_AsTWKB | from scratch | Zigzag varints, delta-encoded, `prec` scales by 10^prec; header byte has type and precision, metadata byte has size/bbox/idlist/extended-dim/empty bits; `POINT EMPTY` is `\x0110` | M | 0/1 |
| ST_GeomFromTWKB | from scratch | SRID unknown | M | 0/2 |
| ST_AsEncodedPolyline | from scratch | LINESTRING (and MULTIPOINT) only; requires SRID 4326; lat before lon; `nprecision` default 5 | S | 0/2 |
| ST_LineFromEncodedPolyline | from scratch | Result SRID 4326; `nprecision` default 5 | S | 0/2 |
| ST_GeoHash | `util::geohash` | `maxchars` default 0: 20 characters for a point, otherwise the precision where the bbox still fits one cell, which can be `''`; error "Geohash requires inputs in decimal degrees" outside lon/lat range; NULL for EMPTY; no upper limit on `maxchars` | S | 0/3 |
| ST_PointFromGeoHash | `util::geohash` | Centre of the decoded box, by double bisection; any length; `precision` truncates; invalid character errors; SRID unknown | S | 0/3 |
| ST_GeomFromGeoHash | `util::geohash` | Ring `(xmin ymin, xmin ymax, xmax ymax, xmax ymin, xmin ymin)` | S | 0/3 |
| ST_Box2dFromGeoHash | `util::geohash` | `precision` 0 is `BOX(-180 -90,180 90)`; NULL means full length | S | 0/3 |
| ST_AsLatLonText | from scratch | Points only; latitude first; out-of-range input is normalised (`-302.23` → `57.76 E`); template tokens `D`, `M`, `S`, `C` with decimal places from repeated `D.DDD`; rounding carries seconds into minutes | M | 0/6 |
| ST_GeogFromText, ST_GeographyFromText, ST_GeogFromWKB | wrappers | Default SRID 4326; depend on G6 geography | S | —, 0/1 |

## 8. Testing

- **Text is compared verbatim.** Every output function needs a hand-written
  `slt/geodatafusion/<function>.slt` recorded from PostGIS with `cargo slt --complete`. Never
  type expected output.
- **Number formatting** gets its own matrix in `geodatafusion/st_astext.slt`, using
  `ST_AsText(ST_MakePoint(x, 0), n)`: the 1e-8 and 1e15 notation thresholds, half-even ties
  on shortest digits (`0.45`, `2.675`, `0.125`), values that round to zero, `-0`, NaN,
  `Infinity`, `maxdecimaldigits` of -1, 0 and 20, and exponents of one to three digits. The
  other text formats then only need a few numeric cases, since they share `write_number`.
- **Dimensions and EMPTY**: every geometry type in XY, Z, M and ZM, EMPTY at the top level and
  as a collection member, nested collections. For both ST_AsText and ST_AsEWKT.
- **Parser grammar** (`geodatafusion/st_geomfromtext.slt`): case-insensitive keywords, `POINTM`
  vs `POINT M`, implicit dimensions, exponents and leading dots (`-.5`, `1e3`), whitespace and
  newlines, `MULTIPOINT` with and without inner parentheses, and the error cases as
  `query error` records (PostGIS errors, `--complete` records them).
- **Round trips**, recorded from PostGIS like everything else:
  `ST_AsEWKT(ST_GeomFromEWKT(ST_AsEWKT(g)))`, `ST_AsEWKT(ST_GeomFromEWKB(ST_AsEWKB(g)))`,
  `ST_AsText(ST_GeomFromTWKB(ST_AsTWKB(g, 3)))`, GeoJSON and encoded polyline likewise.
- **SRID**: literal EWKT/EWKB with and without SRID, `srid` argument overriding an embedded one,
  `SRID=0`. A column with per-row SRIDs is a Rust unit test (the harness only has literals).
- **Unit tests** cover what SQL can't show: return types (`Utf8`, `Binary`, geometry with the
  CRS from a literal SRID), `coord_type` on input functions, identical output for native, WKB
  and WKT inputs, NULL arguments giving NULL, and the per-row SRID error.
- Run the full `cargo slt` after any parser change: every group's literals go through it.

## 9. Style guide amendments

1. **Return types** (add to *Outputs*): "Functions returning PostGIS `text` return `Utf8`, and
   `bytea` returns `Binary`, without GeoArrow extension metadata, even when the content is WKT
   or WKB. Not `Utf8View`." Rationale: R1; one rule for the whole group and for G1's
   `GeometryType` (G1 R5 proposes the same).
2. **Text formatting** (add to *Outputs*): "Coordinates in text output are written with
   `write_number` (`native/io/util/number.rs`). Never format an `f64` with `{}` or
   `to_string()` in user-visible text." Rationale: verbatim parity; the rule is subtle enough
   that it must live in one place.
3. **Optional arguments** (extend *Inputs*): "PostGIS `DEFAULT` parameters become shorter
   overloads in a `Signature::one_of`. `integer` parameters are `Int64` (DataFusion's integer
   literal type). Formatting and option arguments are read per row from the broadcast arrays;
   only arguments that change the return type (SRID) are scalar-only, read in
   `return_field_from_args`." This replaces the current blanket "accept `ColumnarValue::Scalar`
   and return `NotImplemented` for arrays" with G1's R9 rule.
4. **Shared format code** (add to *Layout*): "Format codecs (parsers, writers, encoders) live in
   `native/io/util/`, one module per format, and are `pub(crate)` so other groups (casts,
   aggregates) can reuse them."
5. **Multi-UDF files** (extend *Layout*): add "type-checked constructor families (`*FromText`,
   `*FromWKB`)" and "a format's plain and extended variants (`ST_AsText`/`ST_AsEWKT`)" as
   examples of closely related variants. Such files start with a `//!` doc listing the UDFs.
6. **Naming** (extend the table): "Keep PostGIS's capitalisation inside the name:
   `GeomFromWKB`, `AsEWKT`, `AsHEXEWKB`, `AsGeoJSON`, `Box2dFromGeoHash`." Rationale: the rule
   "PostGIS name without `ST_`, PascalCase" is ambiguous for acronyms.
7. **Errors** (extend): "Invalid text or binary input is an `Execution` error naming the
   function and the problem, PostGIS-style (`ST_GeomFromText: geometry requires more
   points`)."
8. **Tests** (extend *Unit tests*): "Every unit test asserts something; `show()` is not a test."
   And the module is always `mod test`.

## 10. Open questions for the maintainer

1. **Plain `Utf8`/`Binary` for ST_AsText and ST_AsBinary** (R1). It's a breaking change for
   anyone relying on the `geoarrow.wkt`/`geoarrow.wkb` tag. OK?
2. **Per-row SRIDs in columns.** GeoArrow has one CRS per column. Proposal: a SRID prefix in a
   literal sets the column CRS; in a column, it's an `Execution` error unless a `srid`
   argument overrides it. Alternatives: silently drop it (lossy), or a per-row SRID model in
   G6. Which?
3. **Encoding of parsed geometries.** Input functions return a native `Geometry` union today,
   which is subject to the GEOMETRYCOLLECTION collapse (R8) and needs `GeometryCapacity`
   pre-counting. Returning `geoarrow.wkb` would be cheaper and lossless, but every consumer
   would decode it again and the `coord_type` option would become meaningless. Keep native
   (recommended, decided with G6)?
4. **XML input** (ST_GeomFromGML, ST_GMLToSQL, ST_GeomFromKML; MARC21). They need an XML
   parser: add `quick-xml` (behind a feature?) or skip them?
5. **GeoHash move.** Drop the `geohash` crate, move the functions to `native/io`, the Python
   classes to `geodatafusion.native`, and rename `Box2DFromGeoHash` to `Box2dFromGeoHash`
   (R5). Breaking in Rust and Python. OK, or keep re-exports for a release?
6. **SRID → authority names.** ST_AsGML `srsName`, ST_AsGeoJSON `crs` and ST_AsKML need
   PostGIS's `spatial_ref_sys` auth names. Assume `EPSG:<srid>` for every SRID?
7. **Formats to skip**: ST_AsX3D (deferred), MARC21, the ST_AsGeoJSON record form, the ST_AsTWKB
   array form, and curves/TIN/polyhedral surfaces everywhere (NotImplemented). Agreed?
8. **Upstream work.** File the geoarrow-rs issues (GEOMETRYCOLLECTION collapse, mixed-dimension
   panic) and offer implicit-dimension parsing to the `wkt` crate. Who, and should batch 2 wait
   for any of it? (Recommendation: don't wait.)
