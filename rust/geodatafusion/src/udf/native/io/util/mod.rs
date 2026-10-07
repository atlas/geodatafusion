//! Codecs shared by the geometry input and output functions.

pub(crate) mod geohash;
pub(crate) mod number;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the (E)WKB input and output UDFs")
)]
pub(crate) mod wkb;
pub(crate) mod wkt;
