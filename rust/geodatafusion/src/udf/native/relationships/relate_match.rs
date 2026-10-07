//! ST_RelateMatch.

use std::sync::{Arc, LazyLock};

use arrow_array::BooleanArray;
use arrow_array::cast::AsArray;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;

use crate::error::GeoDataFusionResult;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_RelateMatch(text intersectionMatrix, text intersectionMatrixPattern).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Text, Arg::Text]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["intersectionMatrix", "intersectionMatrixPattern"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Tests whether a DE-9IM matrix matches a pattern.
#[user_doc(
    doc_section(label = "Spatial Relationships"),
    description = "Tests whether a DE-9IM intersection matrix matches an intersection matrix pattern. A pattern holds nine characters: T (any non-empty intersection: 0, 1 or 2), F (empty), * (anything), or a dimension 0, 1 or 2. Both strings must have nine characters.",
    syntax_example = "ST_RelateMatch(intersectionMatrix, intersectionMatrixPattern)",
    argument(name = "intersectionMatrix", description = "text"),
    argument(name = "intersectionMatrixPattern", description = "text"),
    related_udf(name = "st_relate")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RelateMatch;

impl RelateMatch {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RelateMatch {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RelateMatch {
    fn name(&self) -> &str {
        "st_relatematch"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(relate_match_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn relate_match_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let [matrices, patterns] = [0, 1].map(|index| {
        args.args[index]
            .cast_to(&DataType::Utf8, None)
            .and_then(|value| value.to_array(args.number_rows))
    });
    let (matrices, patterns) = (matrices?, patterns?);
    let result = matrices
        .as_string::<i32>()
        .iter()
        .zip(patterns.as_string::<i32>())
        .map(|(matrix, pattern)| match (matrix, pattern) {
            // SQL NULL in, SQL NULL out.
            (Some(matrix), Some(pattern)) => relate_match(matrix, pattern).map(Some),
            _ => Ok(None),
        })
        .collect::<GeoDataFusionResult<BooleanArray>>()?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// Whether a DE-9IM matrix matches a pattern. Both must have nine characters, as in GEOS.
pub(crate) fn relate_match(matrix: &str, pattern: &str) -> GeoDataFusionResult<bool> {
    for text in [matrix, pattern] {
        if text.chars().count() != 9 {
            return Err(exec_datafusion_err!(
                "st_relatematch: Should be length 9, is [{text}] instead"
            )
            .into());
        }
    }
    Ok(matrix.chars().zip(pattern.chars()).all(|(entry, wanted)| {
        match wanted.to_ascii_uppercase() {
            '*' => true,
            'T' => matches!(entry, '0' | '1' | '2'),
            wanted => entry.to_ascii_uppercase() == wanted,
        }
    }))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_relate_match() {
        assert!(relate_match("101202FFF", "TTTTTTFFF").unwrap());
        assert!(relate_match("101202FFF", "1*1***FF*").unwrap());
        assert!(!relate_match("101202FFF", "F********").unwrap());
        assert!(relate_match("101202FFF", "TTTX").is_err());
    }
}
