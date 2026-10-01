//! Why a case could not be generated.
//!
//! A [`Rejection`] carries its failure [`Category`] from the place it is
//! raised. `generation-failures.csv` records that category as given; it is
//! never inferred from the message text. The category strings are read by
//! `testsuite-runner` (failure attribution) and by the mutation tool
//! (stillborn mutants), so they must not change; which category a rejection
//! site gives is part of the generator's behaviour, and is changed only
//! deliberately.

use serde::de::DeserializeOwned;
use std::fmt;

/// The failure category written to `generation-failures.csv`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// FDR's `core_real` holds integers, so a fractional real cannot be observed.
    OracleRepresentationLimit,
    /// The row lacks context it needs, such as timing metadata for a clock.
    MissingFixtureContext,
    /// The case expression is not valid RoboChart.
    UnsupportedRobochartSyntax,
    /// A value or type that the fixture encoders do not support.
    UnsupportedFixtureType,
    /// A statement construct that the statement harness does not support.
    UnsupportedStatement,
    /// A recursive record or helper function.
    UnsupportedRecursion,
    /// A fixture value (or JSON column) that does not match what is expected of it.
    InvalidFixtureValue,
    /// Custom function declarations and their CSP definitions do not agree.
    InvalidFunctionFixture,
    /// The recorded Python observation lacks what the check needs.
    InvalidOracle,
    /// A name that is not a RoboChart identifier, or a malformed case ID.
    InvalidIdentifier,
    /// Any other inconsistency in a row's fixture columns.
    FixtureGenerationError,
}

impl Category {
    /// Every category, in declaration order.
    pub const ALL: [Category; 11] = [
        Category::OracleRepresentationLimit,
        Category::MissingFixtureContext,
        Category::UnsupportedRobochartSyntax,
        Category::UnsupportedFixtureType,
        Category::UnsupportedStatement,
        Category::UnsupportedRecursion,
        Category::InvalidFixtureValue,
        Category::InvalidFunctionFixture,
        Category::InvalidOracle,
        Category::InvalidIdentifier,
        Category::FixtureGenerationError,
    ];

    /// The category as written to `generation-failures.csv`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::OracleRepresentationLimit => "oracle_representation_limit",
            Category::MissingFixtureContext => "missing_fixture_context",
            Category::UnsupportedRobochartSyntax => "unsupported_robochart_syntax",
            Category::UnsupportedFixtureType => "unsupported_fixture_type",
            Category::UnsupportedStatement => "unsupported_statement",
            Category::UnsupportedRecursion => "unsupported_recursion",
            Category::InvalidFixtureValue => "invalid_fixture_value",
            Category::InvalidFunctionFixture => "invalid_function_fixture",
            Category::InvalidOracle => "invalid_oracle",
            Category::InvalidIdentifier => "invalid_identifier",
            Category::FixtureGenerationError => "fixture_generation_error",
        }
    }

    /// A rejection in this category.
    pub fn reject(self, reason: impl Into<String>) -> Rejection {
        Rejection { category: self, reason: reason.into() }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A case that cannot be generated: its category and a reason for the reviewer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub category: Category,
    pub reason: String,
}

impl fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for Rejection {}

/// Parse a JSON column. A parse error is rejected in the category of
/// [`json_error_category`], with serde_json's message as the reason.
pub(crate) fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, Rejection> {
    serde_json::from_str(text).map_err(|error| json_error_category(&error).reject(error.to_string()))
}

/// The category of a JSON parse error, from serde_json's classification of
/// it alone. A column that is valid JSON but does not have the expected
/// shape (a wrong type, a missing, unknown or duplicate field, a value out of
/// range) is an invalid fixture value; a column that is not JSON at all
/// (a syntax error, or input that ends early) is a generation error.
pub(crate) fn json_error_category(error: &serde_json::Error) -> Category {
    use serde_json::error::Category as Json;
    match error.classify() {
        Json::Data => Category::InvalidFixtureValue,
        Json::Syntax | Json::Eof | Json::Io => Category::FixtureGenerationError,
    }
}
