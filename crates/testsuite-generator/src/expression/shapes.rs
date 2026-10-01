//! Which shape an expression row takes.
//!
//! [`SHAPE_RULES`] is an ordered table: the first rule that applies to a row
//! decides its shape, so the list order is the precedence. A row reaches
//! the table with its shared fixtures already merged.

use super::{ExpressionTestCase, lambda, scalar, timed};
use crate::fixtures::Fixtures;
use crate::kind::TestCasePlan;
use crate::rejection::{Category, Rejection, from_json};
use serde_json::Value;

/// One expression shape: when it applies, and how it plans a row.
pub struct ShapeRule {
    /// Names the rule for reviewers and tests; generation does not read it.
    #[allow(dead_code)]
    pub name: &'static str,
    pub applies: fn(&ExpressionTestCase) -> bool,
    pub plan: fn(&ExpressionTestCase) -> Result<TestCasePlan, Rejection>,
}

/// Expression shapes, in precedence order. The last rule applies to every row.
pub const SHAPE_RULES: &[ShapeRule] = &[
    // A clock observed through guards, with timing metadata (`timed.rs`).
    ShapeRule { name: "timed_clock", applies: has_timing, plan: timed::timed_expression_model },
    // A clock without timing metadata is rejected, after any blocker that no
    // timing context could remove.
    ShapeRule { name: "since_without_timing", applies: observes_clock, plan: since_without_timing },
    // Features whose expressions are not RoboChart values.
    ShapeRule { name: "unsupported_feature", applies: is_unsupported_feature, plan: reject_unsupported_feature },
    // A function-valued expression, observed through its graph (`lambda.rs`).
    ShapeRule { name: "lambda_graph", applies: observes_function, plan: lambda::lambda_model },
    // Any other row, typed or untyped, including every row with shared
    // fixtures: its context comes from `context.rs`.
    ShapeRule { name: "scalar", applies: every_row, plan: scalar::scalar_plan },
];

/// The shape of a row: the first rule in [`SHAPE_RULES`] that applies.
pub fn shape_of(case: &ExpressionTestCase) -> &'static ShapeRule {
    SHAPE_RULES
        .iter()
        .find(|rule| (rule.applies)(case))
        .expect("the last shape rule applies to every row")
}

/// Features whose recorded expression cannot be a RoboChart value, and why.
const UNSUPPORTED_FEATURES: &[(&str, &str)] = &[
    ("special_real", "RoboChart type names cannot be used as value expressions"),
    ("record_empty", "RoboChart record literals require at least one field"),
];

fn has_timing(case: &ExpressionTestCase) -> bool {
    !case.timing.trim().is_empty()
}

fn observes_clock(case: &ExpressionTestCase) -> bool {
    case.robochart.trim().starts_with("since(") || case.robochart.trim().starts_with("sinceEntry(")
}

fn is_unsupported_feature(case: &ExpressionTestCase) -> bool {
    UNSUPPORTED_FEATURES.iter().any(|(feature, _)| case.feature == *feature)
}

fn observes_function(case: &ExpressionTestCase) -> bool {
    case.robochart.trim_start().starts_with("lambda ")
        || serde_json::from_str::<Value>(&case.python_output)
            .is_ok_and(|output| output.get("graph").is_some())
}

fn every_row(_: &ExpressionTestCase) -> bool {
    true
}

fn since_without_timing(case: &ExpressionTestCase) -> Result<TestCasePlan, Rejection> {
    // Report a blocker that no timing context could remove before the
    // missing context itself.
    if case.robochart.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|word| word == "state") {
        return Err(Category::UnsupportedRobochartSyntax.reject("RoboChart keyword state cannot name a state in sinceEntry"));
    }
    let output: Value = from_json(&case.python_output)?;
    let value = output
        .get("value")
        .ok_or_else(|| Category::InvalidOracle.reject("expression output lacks value"))?;
    Fixtures::default().csp_literal("real", value)?;
    Err(Category::MissingFixtureContext.reject("missing timing context: supply independent ticks and max_ticks metadata"))
}

fn reject_unsupported_feature(case: &ExpressionTestCase) -> Result<TestCasePlan, Rejection> {
    let (_, reason) = UNSUPPORTED_FEATURES
        .iter()
        .find(|(feature, _)| case.feature == *feature)
        .expect("applies only to an unsupported feature");
    Err(Category::UnsupportedRobochartSyntax.reject(*reason))
}
