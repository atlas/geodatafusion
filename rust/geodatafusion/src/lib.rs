// Temporary
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![doc(
    html_logo_url = "https://github.com/geoarrow.png",
    html_favicon_url = "https://github.com/geoarrow.png?size=32"
)]

pub(crate) mod error;
pub mod udf;
pub(crate) mod util;

/// Register all UDFs defined in geodatafusion
pub fn register(session_context: &datafusion::prelude::SessionContext) {
    crate::udf::geo::measurement::register(session_context);

    crate::udf::geo::processing::register(session_context);

    crate::udf::geo::relationships::register(session_context);

    crate::udf::geo::validation::register(session_context);

    #[cfg(feature = "geos-3_11")]
    crate::udf::geos::processing::register(session_context);

    crate::udf::geohash::register(session_context);

    crate::udf::native::accessors::register(session_context);

    crate::udf::native::bounding_box::register(session_context);

    crate::udf::native::constructors::register(session_context);

    crate::udf::native::io::register(session_context);
}

#[cfg(test)]
mod test {
    use datafusion::logical_expr::Documentation;
    use datafusion::prelude::SessionContext;

    use super::*;

    /// The PostGIS reference chapters, which are the documentation sections.
    const DOC_SECTIONS: &[&str] = &[
        "Data Types",
        "Geometry Constructors",
        "Geometry Accessors",
        "Geometry Editors",
        "Geometry Validation",
        "Spatial Reference System Functions",
        "Geometry Input",
        "Geometry Output",
        "Operators",
        "Spatial Relationships",
        "Measurement Functions",
        "Overlay Functions",
        "Geometry Processing",
        "Coverages",
        "Affine Transformations",
        "Clustering Functions",
        "Bounding Box Functions",
        "Linear Referencing",
        "Trajectory Functions",
    ];

    fn check_documentation(name: &str, aliases: &[String], doc: Option<&Documentation>) {
        let doc = doc.unwrap_or_else(|| panic!("{name} has no documentation"));
        assert!(
            DOC_SECTIONS.contains(&doc.doc_section.label),
            "{name}: section {:?} isn't a PostGIS reference chapter",
            doc.doc_section.label
        );
        let syntax = doc.syntax_example.to_lowercase();
        assert!(
            std::iter::once(name)
                .chain(aliases.iter().map(String::as_str))
                .any(|n| syntax.starts_with(&format!("{n}("))),
            "{name}: syntax example {:?} doesn't start with the function name",
            doc.syntax_example
        );
        assert!(
            !doc.description.is_empty(),
            "{name} has an empty description"
        );
    }

    #[test]
    fn test_every_udf_is_documented() {
        let builtin = SessionContext::new().state();
        let ctx = SessionContext::new();
        register(&ctx);
        let state = ctx.state();

        let scalar = state
            .scalar_functions()
            .values()
            .filter(|udf| !builtin.scalar_functions().contains_key(udf.name()));
        for udf in scalar {
            check_documentation(udf.name(), udf.aliases(), udf.documentation());
        }
        let aggregate = state
            .aggregate_functions()
            .values()
            .filter(|udf| !builtin.aggregate_functions().contains_key(udf.name()));
        for udf in aggregate {
            check_documentation(udf.name(), udf.aliases(), udf.documentation());
        }
    }
}
