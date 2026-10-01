//! Fixed declarations shared by original expression rows whose expressions
//! name context that the source CSV does not declare, such as a record
//! constant or a helper function.
//!
//! The fixtures file lists each fixture once and maps original case IDs to the
//! fixtures they use. Its definitions are explicit data: nothing here derives a
//! fixture from a row's recorded Python output, and the source CSV is never
//! rewritten. RoboChart library names (for example `size`) must come from
//! the library imports, so fixtures cannot declare them.

use crate::rejection::{Category, Rejection};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const FILE_NAME: &str = "shared_fixtures.json";

/// Names that RoboTool's libraries already declare; a fixture must not shadow them.
const LIBRARY_NAMES: &[&str] = &[
    "size", "head", "tail", "last", "front", "reverse", "cat", "dom", "ran", "card",
    "union", "inter", "diff", "transpose", "inverse", "pi",
];

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedFixtures {
    #[serde(default)]
    #[allow(dead_code)]
    description: Vec<String>,
    #[serde(default)]
    fixtures: BTreeMap<String, Fixture>,
    #[serde(default)]
    cases: BTreeMap<String, TestCaseFixtures>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    /// Why the fixture exists and where its values come from.
    #[allow(dead_code)]
    note: String,
    #[serde(default)]
    datatypes: Map<String, Value>,
    #[serde(default)]
    constants: BTreeMap<String, Constant>,
    /// RoboChart function declarations, one per entry.
    #[serde(default)]
    functions: Vec<String>,
    /// The CSP body of each declared function, as used by the statement rows.
    #[serde(default)]
    function_definitions: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constant {
    #[serde(rename = "type")]
    pub ty: String,
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestCaseFixtures {
    fixtures: Vec<String>,
    output_type: String,
}

/// The context columns a row receives from its shared fixtures.
#[derive(Debug, Default)]
pub struct TestCaseContext {
    pub datatypes: String,
    pub constants: BTreeMap<String, Constant>,
    pub functions: String,
    pub function_definitions: String,
    pub output_type: String,
}

impl SharedFixtures {
    /// Load the fixtures stored beside a source CSV, or none if absent.
    pub fn load(directory: &Path) -> Result<Self> {
        let path = directory.join(FILE_NAME);
        if !path.is_file() {
            return Ok(Self::default());
        }
        let fixtures: Self = serde_json::from_str(&fs::read_to_string(&path)?)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        fixtures.validate()?;
        Ok(fixtures)
    }

    pub fn test_case_ids(&self) -> impl Iterator<Item = &String> {
        self.cases.keys()
    }

    fn validate(&self) -> Result<()> {
        let mut used = BTreeSet::new();
        for (id, case) in &self.cases {
            if case.fixtures.is_empty() {
                return Err(format!("shared fixture use {id} names no fixture").into());
            }
            for name in &case.fixtures {
                if !self.fixtures.contains_key(name) {
                    return Err(format!("case {id} uses unknown shared fixture {name}").into());
                }
                used.insert(name);
            }
            self.context(id)?;
        }
        if let Some(unused) = self.fixtures.keys().find(|name| !used.contains(name)) {
            return Err(format!("shared fixture {unused} is not used by any case").into());
        }
        Ok(())
    }

    /// The merged context for an original case, if the case uses fixtures.
    /// `load` has already checked every case, so generation never meets
    /// these rejections for a loaded file.
    pub fn context(&self, id: &str) -> std::result::Result<Option<TestCaseContext>, Rejection> {
        let Some(case) = self.cases.get(id) else {
            return Ok(None);
        };
        let mut datatypes = Map::new();
        let mut constants = BTreeMap::new();
        let mut functions = Vec::new();
        let mut definitions = Map::new();
        let mut declared = BTreeSet::new();
        for name in &case.fixtures {
            let fixture = &self.fixtures[name];
            let names = fixture
                .datatypes
                .keys()
                .chain(fixture.constants.keys())
                .chain(fixture.function_definitions.keys());
            for declared_name in names {
                if LIBRARY_NAMES.contains(&declared_name.as_str()) {
                    return Err(Category::FixtureGenerationError.reject(format!(
                        "shared fixture {name} must not redeclare library name {declared_name}"
                    )));
                }
                if !declared.insert(declared_name.clone()) {
                    return Err(Category::FixtureGenerationError
                        .reject(format!("case {id} declares {declared_name} twice")));
                }
            }
            datatypes.extend(fixture.datatypes.clone());
            constants.extend(fixture.constants.clone());
            functions.extend(fixture.functions.iter().cloned());
            definitions.extend(fixture.function_definitions.clone());
        }
        let json = |map: Map<String, Value>| {
            if map.is_empty() { String::new() } else { Value::Object(map).to_string() }
        };
        Ok(Some(TestCaseContext {
            datatypes: json(datatypes),
            constants,
            functions: functions.join("\n"),
            function_definitions: json(definitions),
            output_type: case.output_type.clone(),
        }))
    }
}

#[cfg(test)]
#[path = "tests/shared_fixtures.rs"]
mod tests;
