//! Statement rows: a RoboChart statement, the variables it starts from and
//! the state (and events) Python recorded after it. Every part of a
//! statement's context comes from a column; there are no implicit rules.

mod plan;

use crate::kind::{GeneratedTestCase, TestCaseId, TestCaseKind, TestCasePlan};
use crate::rejection::Rejection;
use crate::shared_fixtures::SharedFixtures;
use serde::Deserialize;

type Result<T> = std::result::Result<T, Rejection>;

#[derive(Clone, Deserialize)]
pub struct StatementTestCase {
    pub id: String,
    pub feature: String,
    pub robochart: String,
    pub inputs: String,
    pub python_output: String,
    pub types: String,
    pub observe: String,
    pub expected_status: String,
    #[serde(default)]
    pub datatypes: String,
    #[serde(default)]
    pub functions: String,
    #[serde(default)]
    pub events: String,
    #[serde(default)]
    pub input_events: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub iteration: String,
    #[serde(default)]
    pub function_definitions: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub domains: String,
    #[serde(default)]
    pub constants: String,
}

/// The statement kind of case row.
pub struct Statement;

impl TestCaseKind for Statement {
    const KIND: &'static str = "statement";
    type Row = StatementTestCase;
    type Plan = TestCasePlan;

    fn test_case_id(row: &StatementTestCase) -> TestCaseId {
        TestCaseId { feature: row.feature.clone(), id: row.id.clone() }
    }

    /// Statement rows do not use shared fixtures.
    fn plan(row: &StatementTestCase, _: &SharedFixtures) -> Result<TestCasePlan> {
        plan::plan(row)
    }

    fn render(id: &TestCaseId, plan: &TestCasePlan) -> GeneratedTestCase {
        plan.render(Self::KIND, id)
    }
}

/// Plan and render a statement row.
#[cfg(test)]
pub(crate) fn statement_model(case: StatementTestCase) -> Result<GeneratedTestCase> {
    let plan = plan::plan(&case)?;
    Ok(Statement::render(&Statement::test_case_id(&case), &plan))
}
