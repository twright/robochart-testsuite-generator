//! The testsuite generator: reads recorded translator observations (case
//! rows and shared fixtures) and writes a review project of RoboChart models
//! and CSP checks. It runs no subprocesses; RoboTool and FDR are driven by
//! `testsuite-runner`.
//!
//! A reviewer who wants to know what surrounds a case reads, in order:
//! `kind.rs` (the pipeline every kind of row shares), `expression/shapes.rs`
//! (which shape an expression row takes), `expression/context.rs` (the
//! context rules of scalar expressions) and the shape modules beside it,
//! `statement/plan.rs`, `fixtures.rs` (literals and types) and `render.rs`
//! (the one model renderer). `facts.rs` states what the FDR preparation of
//! a project needs to know about its cases.

mod expression;
pub mod facts;
pub mod fixtures;
mod kind;
mod robotool_project;
pub mod rejection;
mod render;
pub mod shared_fixtures;
mod statement;
#[cfg(any(test, feature = "testing"))]
#[path = "tests/test_rows.rs"]
pub mod test_rows;
#[cfg(test)]
#[path = "tests/generator_properties.rs"]
mod generator_properties;
#[cfg(test)]
#[path = "tests/testsuite_generator.rs"]
mod tests;

pub use expression::{ExpressionTestCase, TimingContext, validate_quantifier_domains};
pub use robotool_project::{IndexRow, test_case_csp_dir, copy_selected_csv, generate, project_file};
pub use rejection::{Category, Rejection};
pub use statement::StatementTestCase;

use rejection::from_json;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
/// The project directory used when none is given: `testsuite-project`,
/// relative to the current directory.
pub const DEFAULT_PROJECT: &str = "testsuite-project";

#[derive(Clone, Debug, Deserialize)]
pub struct FunctionDefinition {
    pub parameters: Vec<String>,
    pub body: String,
}

/// Custom functions (expression and statement rows) need one explicit CSP
/// definition each, and may not call one another. Gives the definitions of
/// the declared functions, by name.
fn validate_function_fixtures(
    declarations: &str,
    definition_json: &str,
) -> std::result::Result<BTreeMap<String, FunctionDefinition>, Rejection> {
    let definitions: BTreeMap<String, FunctionDefinition> = if definition_json.trim().is_empty() {
        BTreeMap::new()
    } else {
        from_json(definition_json)?
    };
    let tokens: Vec<_> = declarations
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|s| !s.is_empty())
        .collect();
    let declared_functions: BTreeSet<_> = tokens
        .windows(2)
        .filter(|pair| pair[0] == "function")
        .map(|pair| pair[1].to_owned())
        .collect();
    if declared_functions != definitions.keys().cloned().collect() {
        return Err(Category::InvalidFunctionFixture
            .reject("custom functions require matching explicit CSP fixture definitions"));
    }
    for (name, definition) in &definitions {
        validate_identifier(name)?;
        let mut names = BTreeSet::new();
        for parameter in &definition.parameters {
            validate_identifier(parameter)?;
            if !names.insert(parameter) {
                return Err(Category::InvalidFunctionFixture.reject("duplicate function fixture parameter"));
            }
        }
        if definition.body.trim().is_empty() || definition.body.contains(['\n', '\r']) {
            return Err(Category::InvalidFunctionFixture
                .reject("function fixture body must be one non-empty CSP expression"));
        }
        let body_tokens: Vec<_> = definition
            .body
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .collect();
        if body_tokens
            .iter()
            .any(|token| declared_functions.contains(*token))
        {
            return Err(Category::UnsupportedRecursion.reject("unsupported recursive or dependent helper fixture"));
        }
    }
    Ok(definitions)
}

fn test_case_path(kind: &str, feature: &str, id: &str) -> std::result::Result<PathBuf, Rejection> {
    validate_identifier(feature)?;
    let mut parts = id.split('/');
    let first = parts.next().ok_or_else(|| Category::InvalidIdentifier.reject("empty case ID"))?;
    let second = parts.next().ok_or_else(|| Category::InvalidIdentifier.reject("case ID needs two parts"))?;
    if parts.next().is_some() || first != feature {
        return Err(Category::InvalidIdentifier.reject(format!("unsupported case ID {id}")));
    }
    if second.is_empty()
        || !second
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(Category::InvalidIdentifier.reject(format!("invalid case suffix: {second}")));
    }
    Ok(Path::new("cases")
        .join(kind)
        .join(feature)
        .join(format!("{second}.rct")))
}

pub fn package_name(id: &str, kind: &str) -> std::result::Result<String, Rejection> {
    let normalised = id.replace('/', "_");
    validate_identifier(&normalised)?;
    Ok(format!("{kind}_{normalised}"))
}

pub fn validate_identifier(value: &str) -> std::result::Result<(), Rejection> {
    if value.is_empty()
        || !value.chars().next().unwrap().is_ascii_alphabetic()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(Category::InvalidIdentifier.reject(format!("invalid RoboChart identifier: {value}")));
    }
    Ok(())
}
