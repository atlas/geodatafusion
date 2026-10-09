//! Helpers shared by the UDFs of every provider.

pub(crate) mod args;
pub(crate) mod collect;
pub(crate) mod ewkt;
pub(crate) mod field;
pub(crate) mod kernel;
pub(crate) mod ordinates;
#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(
        dead_code,
        reason = "only GEOS-backed UDFs read further geometry arguments so far"
    )
)]
pub(crate) mod owned;
pub(crate) mod signature;
pub(crate) mod srid;
mod srid_authorities;
#[cfg(test)]
pub(crate) mod test;
