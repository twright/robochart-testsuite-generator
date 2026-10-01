//! Expression rows: a RoboChart expression and the value Python recorded for it.
//!
//! [`Expression::plan`] first merges a row's shared fixtures into it (so a
//! row with a shared fixture is a typed row), then gives the row the first
//! shape in [`shapes::SHAPE_RULES`] that applies to it.

pub mod context;
mod lambda;
mod scalar;
pub mod shapes;
mod timed;

use crate::fixtures::Fixtures;
use crate::kind::{GeneratedTestCase, TestCaseId, TestCaseKind, TestCasePlan};
use crate::rejection::{Category, Rejection, from_json};
use crate::shared_fixtures::{self, SharedFixtures};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize)]
pub struct ExpressionTestCase {
    pub id: String,
    pub feature: String,
    pub robochart: String,
    pub inputs: String,
    pub python_output: String,
    #[serde(default)]
    pub types: String,
    #[serde(default)]
    pub datatypes: String,
    #[serde(default)]
    pub output_type: String,
    #[serde(default)]
    pub functions: String,
    #[serde(default)]
    pub function_definitions: String,
    #[serde(default)]
    pub timing: String,
    /// Constants from shared fixtures; never read from the source CSV.
    #[serde(skip)]
    pub fixture_constants: BTreeMap<String, shared_fixtures::Constant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingContext {
    pub ticks: u32,
    pub max_ticks: u32,
}

/// The expression kind of case row.
pub struct Expression;

impl TestCaseKind for Expression {
    const KIND: &'static str = "expression";
    type Row = ExpressionTestCase;
    type Plan = TestCasePlan;

    fn test_case_id(row: &ExpressionTestCase) -> TestCaseId {
        TestCaseId { feature: row.feature.clone(), id: row.id.clone() }
    }

    /// Shared fixtures merge before the shape is chosen.
    fn plan(row: &ExpressionTestCase, shared: &SharedFixtures) -> Result<TestCasePlan, Rejection> {
        let mut case = row.clone();
        apply_shared_fixtures(&mut case, shared)?;
        expression_plan(&case)
    }

    fn render(id: &TestCaseId, plan: &TestCasePlan) -> GeneratedTestCase {
        plan.render(Self::KIND, id)
    }
}

/// Give an original row the context declared by its shared fixtures. The row
/// must not declare context of its own, so a fixture never overrides source data.
pub(crate) fn apply_shared_fixtures(case: &mut ExpressionTestCase, shared: &SharedFixtures) -> Result<(), Rejection> {
    let Some(context) = shared.context(&case.id)? else { return Ok(()) };
    if [&case.types, &case.datatypes, &case.output_type, &case.functions, &case.function_definitions, &case.timing]
        .iter()
        .any(|column| !column.trim().is_empty())
    {
        return Err(Category::FixtureGenerationError.reject("shared fixtures conflict with the row's own context columns"));
    }
    if !from_json::<BTreeMap<String, Value>>(&case.inputs)?.is_empty() {
        return Err(Category::FixtureGenerationError.reject("shared fixtures support rows without recorded inputs"));
    }
    case.types = "{}".into();
    case.datatypes = context.datatypes;
    case.output_type = context.output_type;
    case.functions = context.functions;
    case.function_definitions = context.function_definitions;
    case.fixture_constants = context.constants;
    Ok(())
}

/// Plan a row (with its shared fixtures already merged) in the first shape
/// that applies to it.
pub(crate) fn expression_plan(case: &ExpressionTestCase) -> Result<TestCasePlan, Rejection> {
    (shapes::shape_of(case).plan)(case)
}

/// Plan and render a row (with its shared fixtures already merged).
#[cfg(test)]
pub(crate) fn expression_model(case: ExpressionTestCase) -> Result<GeneratedTestCase, Rejection> {
    let plan = expression_plan(&case)?;
    Ok(Expression::render(&Expression::test_case_id(&case), &plan))
}

// Limit bound variables to the evaluation domain recorded in the expression
// fixture. Arithmetic and output types continue to use the wider core types.
pub fn validate_quantifier_domains(value: &Value) -> Result<(), Rejection> {
    let domains = value
        .as_object()
        .ok_or_else(|| Category::FixtureGenerationError.reject("quantifier domains must be an object"))?;
    for (ty, choices) in domains {
        if !matches!(ty.as_str(), "int" | "nat" | "boolean") {
            return Err(Category::FixtureGenerationError.reject(format!("unsupported quantifier domain type {ty}")));
        }
        let choices = choices
            .as_array()
            .ok_or_else(|| Category::FixtureGenerationError.reject("quantifier domain must be an array"))?;
        for item in choices {
            Fixtures::default().csp_literal(ty, item)?;
        }
    }
    Ok(())
}
