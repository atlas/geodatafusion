//! Codecs shared by the geometry input and output functions.

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used by the GeoHash UDFs once they move off the geohash crate"
    )
)]
pub(crate) mod geohash;
pub(crate) mod number;
pub(crate) mod wkt;
