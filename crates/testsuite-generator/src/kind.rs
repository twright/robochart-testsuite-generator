//! Test case kinds and the one generation pipeline that they share.
//!
//! Every kind of case row (expression or statement) is planned, then
//! rendered. A row that cannot be generated is a [`Rejection`] whose
//! category was decided where it was raised.

use crate::facts::CspFacts;
use crate::rejection::{Category, Rejection};
use crate::render::Machine;
use crate::shared_fixtures::SharedFixtures;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A kind of case row, read from its own CSV.
pub trait TestCaseKind {
    /// The kind as written to `case-index.csv` and `generation-failures.csv`.
    const KIND: &'static str;
    /// One row of the kind's CSV.
    type Row: DeserializeOwned;
    /// The decisions taken for a row.
    type Plan;
    /// The identity of a row, known before it is planned.
    fn test_case_id(row: &Self::Row) -> TestCaseId;
    /// Decide everything about a row's case, or reject it.
    fn plan(row: &Self::Row, shared: &SharedFixtures) -> Result<Self::Plan, Rejection>;
    /// Render a planned case; rendering takes no decisions and cannot fail.
    fn render(id: &TestCaseId, plan: &Self::Plan) -> GeneratedTestCase;
}

/// The identity of a case row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestCaseId {
    pub feature: String,
    pub id: String,
}

/// A rendered case: its model, and what its CSP check needs.
#[derive(Clone)]
pub struct GeneratedTestCase {
    pub timed: bool,
    pub id: String,
    pub kind: &'static str,
    pub feature: String,
    pub path: PathBuf,
    pub model: String,
    pub expected_csp: String,
    pub fixture_arguments: Vec<String>,
    pub expected_status: String,
    pub expected_events: Vec<String>,
    pub driver: Option<(String, Vec<String>)>,
    /// A complete check script, for cases whose check is not a single
    /// expected output (lambda rows compare a graph).
    pub custom_check: Option<String>,
}

/// The decisions taken for a row of any kind: its model, as a [`Machine`],
/// and what its CSP check needs. Each shape writes a check that is not a
/// single expected output itself (`custom_check`).
#[derive(Clone, Debug)]
pub struct TestCasePlan {
    pub timed: bool,
    pub path: PathBuf,
    pub machine: Machine,
    pub expected_csp: String,
    pub fixture_arguments: Vec<String>,
    pub expected_status: String,
    pub expected_events: Vec<String>,
    pub driver: Option<(String, Vec<String>)>,
    pub custom_check: Option<String>,
    /// What the FDR preparation needs to know about the case's CSP.
    pub facts: CspFacts,
}

impl TestCasePlan {
    /// Render the plan of a row of kind `kind`; this takes no decisions.
    pub fn render(&self, kind: &'static str, id: &TestCaseId) -> GeneratedTestCase {
        GeneratedTestCase {
            timed: self.timed,
            id: id.id.clone(),
            kind,
            feature: id.feature.clone(),
            path: self.path.clone(),
            model: self.machine.render(),
            expected_csp: self.expected_csp.clone(),
            fixture_arguments: self.fixture_arguments.clone(),
            expected_status: self.expected_status.clone(),
            expected_events: self.expected_events.clone(),
            driver: self.driver.clone(),
            custom_check: self.custom_check.clone(),
        }
    }
}

/// A row that could not be generated.
pub(crate) struct Failure {
    pub kind: &'static str,
    pub test_case: TestCaseId,
    pub rejection: Rejection,
}

/// The cases and failures of one generation run, over all kinds.
#[derive(Default)]
pub(crate) struct Generation {
    pub cases: Vec<GeneratedTestCase>,
    pub failures: Vec<Failure>,
    /// IDs of the selected rows, in reading order.
    pub found: Vec<String>,
}

impl Generation {
    /// Generate the rows of one kind's CSV for which `selected` holds, in
    /// file order. Gives the IDs of every row in the file.
    pub fn run<K: TestCaseKind>(
        &mut self,
        source: &Path,
        shared: &SharedFixtures,
        selected: impl Fn(&str) -> bool,
    ) -> crate::Result<BTreeSet<String>> {
        let mut ids = BTreeSet::new();
        for result in csv::Reader::from_path(source)?.deserialize() {
            let row: K::Row = result?;
            let case = K::test_case_id(&row);
            ids.insert(case.id.clone());
            if !selected(&case.id) {
                continue;
            }
            self.found.push(case.id.clone());
            match K::plan(&row, shared) {
                Ok(plan) => self.cases.push(K::render(&case, &plan)),
                Err(rejection) => {
                    let rejection = feature_rejection(&case.feature, rejection);
                    self.failures.push(Failure { kind: K::KIND, test_case: case, rejection });
                }
            }
        }
        Ok(ids)
    }
}

/// A row of feature `record_empty` records an empty record literal, which is
/// not RoboChart. Whatever rejects such a row, the failure is reported as
/// unsupported syntax, unless the row is rejected for an oracle limit or
/// missing context. This is the one category rule that depends on a row
/// rather than on the rejection site; it applies to every kind.
pub(crate) fn feature_rejection(feature: &str, rejection: Rejection) -> Rejection {
    match rejection.category {
        Category::OracleRepresentationLimit | Category::MissingFixtureContext => rejection,
        _ if feature == "record_empty" => Rejection { category: Category::UnsupportedRobochartSyntax, ..rejection },
        _ => rejection,
    }
}
