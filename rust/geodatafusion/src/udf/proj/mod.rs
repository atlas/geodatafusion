//! User-defined functions that wrap PROJ, behind the `proj` feature.

pub mod srs;
pub(crate) mod util;

use self::util::crs::proj_definition;
use self::util::operation::{Definition, with_operation};
use crate::util::srid::srid_to_crs;

/// Whether PROJ can transform WGS 84 longitude/latitude to SRID `srid`, read as `ST_Transform`
/// reads a target SRID: a statement's transform to it would otherwise fail where it is
/// computed. The operation is created once per thread, as a transform's is.
pub fn transforms_to(srid: i32) -> bool {
    let Some(to) = proj_definition(&srid_to_crs(srid)) else {
        return false;
    };
    let definition = Definition::CrsToCrs {
        from: "EPSG:4326".to_owned(),
        to,
    };
    with_operation("st_transform", &definition, |_| ()).is_ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_transforms_to_the_srids_proj_knows() {
        for srid in [4326, 25833, 27700, 4269, 3857] {
            assert!(transforms_to(srid), "{srid}");
        }
        for srid in [0, 999_999, -1] {
            assert!(!transforms_to(srid), "{srid}");
        }
    }
}
