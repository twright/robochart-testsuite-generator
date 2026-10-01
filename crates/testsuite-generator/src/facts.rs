//! What the FDR preparation of a project needs to know about its cases, stated
//! by the generator so that no other crate reinterprets case rows.
//!
//! RoboTool's CSP needs finite bounds, function instantiations and domain
//! restrictions before FDR can check it. There are two kinds of fact:
//!
//! - [`extents`] scans every row of a [`Snapshot`], including rows that fail
//!   generation (the global bounds have always covered them): the integer
//!   range, the longest sequence, the timed clock bound and the constants with
//!   one value.
//! - [`csp_facts`] gives, for each row that can be generated, what its plan
//!   decided for the CSP: the explicit body of each fixture function, the
//!   explicit value domains of a statement's record and sequence types, and the
//!   finite domains of a quantifier's binders.
//!
//! `testsuite-runner` turns both into a `robotool_harness::FdrPreparation`.

use crate::expression::{Expression, ExpressionTestCase, TimingContext};
use crate::fixtures::Type;
use crate::kind::TestCaseKind;
use crate::shared_fixtures::SharedFixtures;
use crate::statement::{Statement, StatementTestCase};
use crate::{FunctionDefinition, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The rows of a project's `source-data/` snapshot, with its shared fixtures.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub expressions: Vec<ExpressionTestCase>,
    pub statements: Vec<StatementTestCase>,
    pub shared: SharedFixtures,
}

impl Snapshot {
    /// Read `cases.csv`, `statement_cases.csv` and, if present, the shared
    /// fixtures from `directory` (a project's `source-data/`).
    pub fn load(directory: &Path) -> Result<Self> {
        let expressions = csv::Reader::from_path(directory.join("cases.csv"))?
            .deserialize()
            .collect::<std::result::Result<_, _>>()?;
        let statements = csv::Reader::from_path(directory.join("statement_cases.csv"))?
            .deserialize()
            .collect::<std::result::Result<_, _>>()?;
        Ok(Self { expressions, statements, shared: SharedFixtures::load(directory)? })
    }

    /// The rows with ID `id`, as a snapshot of their own: what a test case
    /// is prepared from when it is checked on its own.
    pub fn only(&self, id: &str) -> Self {
        Self {
            expressions: self.expressions.iter().filter(|row| row.id == id).cloned().collect(),
            statements: self.statements.iter().filter(|row| row.id == id).cloned().collect(),
            shared: self.shared.clone(),
        }
    }

    /// The ID of every row, in both CSVs.
    pub fn test_case_ids(&self) -> BTreeSet<String> {
        self.expressions.iter().map(|row| row.id.clone()).chain(self.statements.iter().map(|row| row.id.clone())).collect()
    }
}

/// The values that the rows of a snapshot use, over all its rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extents {
    /// The smallest and largest integer in any value or source text (at most
    /// -2 and at least 2, RoboTool's default range).
    pub int_range: (i64, i64),
    /// The length of the longest sequence value, or a row's explicit
    /// `sequence_length` domain; at least 2, RoboTool's default.
    pub sequence_length: usize,
    /// The bound of the timed clock type: above every row's `max_ticks`, and
    /// at least 10. Rows whose `timing` is not a timing context are skipped.
    pub clock_max: u32,
    /// The integer constants and their values: every input of an expression
    /// row, and every declared constant of a statement row. A name with
    /// different values in different rows is left out.
    pub constants: BTreeMap<String, i64>,
}

/// The extents of every row in `snapshot`, whether or not it can be
/// generated. A `timing` column that does not parse (as JSON, or for the
/// clock bound as a timing context) is skipped; any other JSON column of a
/// row that does not parse is an error.
pub fn extents(snapshot: &Snapshot) -> Result<Extents> {
    let mut scan = Scan { minimum: -2, maximum: 2, sequence_length: 2 };
    for row in &snapshot.expressions {
        scan.source(&[&row.robochart, &row.functions, &row.function_definitions]);
        for (field, raw) in [("inputs", &row.inputs), ("python_output", &row.python_output)] {
            scan.json(field, raw)?;
        }
        // Timing metadata that does not parse has no values to cover; the
        // row cannot be generated, and must not stop the others' preparation.
        if serde_json::from_str::<Value>(&row.timing).is_ok() {
            scan.json("timing", &row.timing)?;
        }
    }
    for row in &snapshot.statements {
        scan.source(&[&row.robochart, &row.functions, &row.function_definitions]);
        for (field, raw) in [
            ("inputs", &row.inputs),
            ("python_output", &row.python_output),
            ("domains", &row.domains),
            ("driver", &row.driver),
            ("iteration", &row.iteration),
        ] {
            scan.json(field, raw)?;
        }
    }
    for row in &snapshot.expressions {
        let Some(context) = snapshot.shared.context(&row.id)? else { continue };
        scan.source(&[&context.functions, &context.function_definitions]);
        for constant in context.constants.values() {
            integer_bounds(&constant.value, &mut scan.minimum, &mut scan.maximum);
            sequence_length(&constant.value, &mut scan.sequence_length);
        }
    }
    let mut clock_max = 10u32;
    // As above, timing metadata that is not a timing context is skipped.
    for row in &snapshot.expressions {
        if let Ok(timing) = serde_json::from_str::<TimingContext>(&row.timing) {
            clock_max = clock_max.max(timing.max_ticks + 1);
        }
    }
    Ok(Extents {
        int_range: (scan.minimum, scan.maximum),
        sequence_length: scan.sequence_length,
        clock_max,
        constants: constants(snapshot),
    })
}

/// The running bounds of [`extents`].
struct Scan {
    minimum: i64,
    maximum: i64,
    sequence_length: usize,
}

impl Scan {
    /// The integer literals of source text (expressions, function declarations
    /// and CSP bodies).
    fn source(&mut self, texts: &[&String]) {
        for text in texts {
            integer_literals(text, &mut self.minimum, &mut self.maximum);
        }
    }

    /// The integers and sequence lengths of a JSON column, if it is not empty.
    fn json(&mut self, field: &str, raw: &str) -> Result<()> {
        if raw.trim().is_empty() {
            return Ok(());
        }
        let value: Value = serde_json::from_str(raw)?;
        integer_bounds(&value, &mut self.minimum, &mut self.maximum);
        if field == "domains" {
            if let Some(bound) = value.get("sequence_length").and_then(Value::as_u64) {
                self.sequence_length = self.sequence_length.max(usize::try_from(bound)?);
            }
        } else {
            for item in fixture_values(field, &value) {
                sequence_length(item, &mut self.sequence_length);
            }
        }
        Ok(())
    }
}

/// The constants with one value over the snapshot (see [`Extents::constants`]).
fn constants(snapshot: &Snapshot) -> BTreeMap<String, i64> {
    let mut values: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let rows = snapshot
        .expressions
        .iter()
        .map(|row| (&row.inputs, None))
        .chain(snapshot.statements.iter().map(|row| (&row.inputs, Some(&row.constants))));
    for (inputs, declared) in rows {
        let Ok(Value::Object(inputs)) = serde_json::from_str::<Value>(inputs) else { continue };
        // Every input of an expression row is a constant.
        let names: BTreeSet<String> = match declared {
            None => inputs.keys().cloned().collect(),
            Some(declared) => serde_json::from_str(declared).unwrap_or_default(),
        };
        for name in names {
            if let Some(value) = inputs.get(&name).and_then(Value::as_i64) {
                values.entry(name).or_default().insert(value);
            }
        }
    }
    values
        .into_iter()
        .filter(|(_, v)| v.len() == 1)
        .map(|(name, v)| (name, *v.iter().next().unwrap()))
        .collect()
}

fn integer_literals(source: &str, minimum: &mut i64, maximum: &mut i64) {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if let Ok(value) = source[start..index].parse::<i64>() {
            *minimum = (*minimum).min(value);
            *maximum = (*maximum).max(value);
            if start > 0 && bytes[start - 1] == b'-' {
                *minimum = (*minimum).min(-value);
            }
        }
    }
}

// The fixture values inside a column. A driver script and an expected event
// trace are lists of steps, not sequence values, so only their payloads count
// towards the sequence bound.
fn fixture_values<'a>(field: &str, value: &'a Value) -> Vec<&'a Value> {
    match field {
        "driver" => value
            .as_array()
            .map(|steps| steps.iter().filter_map(|step| step.get("value")).collect())
            .unwrap_or_default(),
        "python_output" => {
            let mut values: Vec<&Value> = ["value", "state"].iter().filter_map(|key| value.get(*key)).collect();
            if let Some(events) = value.get("events").and_then(Value::as_array) {
                values.extend(events.iter().filter_map(|event| event.get("value")));
            }
            values
        }
        _ => vec![value],
    }
}

fn sequence_length(value: &Value, maximum: &mut usize) {
    match value {
        Value::Array(items) => {
            *maximum = (*maximum).max(items.len());
            for item in items {
                sequence_length(item, maximum);
            }
        }
        Value::Object(fields) => {
            for item in fields.values() {
                sequence_length(item, maximum);
            }
        }
        _ => {}
    }
}

fn integer_bounds(value: &Value, minimum: &mut i64, maximum: &mut i64) {
    match value {
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                *minimum = (*minimum).min(integer);
                *maximum = (*maximum).max(integer);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| integer_bounds(item, minimum, maximum)),
        Value::Object(fields) => fields.values().for_each(|item| integer_bounds(item, minimum, maximum)),
        _ => {}
    }
}

/// What a case's plan decided for its CSP, beyond the model itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspFacts {
    /// The fixture functions, each with its explicit CSP body.
    pub functions: Vec<FunctionFact>,
    /// Explicit finite domains of record and sequence types (statement rows).
    pub value_domains: Vec<ValueDomain>,
    /// Finite domains of quantifier binders (scalar expression rows).
    pub binder_domains: Vec<BinderDomain>,
}

/// A fixture function and its CSP body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionFact {
    pub name: String,
    pub parameters: Vec<String>,
    pub body: String,
}

impl FunctionFact {
    /// The functions of a validated `function_definitions` column, by name.
    pub(crate) fn all(definitions: BTreeMap<String, FunctionDefinition>) -> Vec<Self> {
        definitions
            .into_iter()
            .map(|(name, FunctionDefinition { parameters, body })| Self { name, parameters, body })
            .collect()
    }
}

/// The values that a record or sequence type ranges over in FDR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueDomain {
    /// A record or enumeration ([`Type::Record`]) or a sequence
    /// ([`Type::Sequence`]); planning rejects any other type.
    pub ty: Type,
    /// The CSP literal of each value, in the order given.
    pub values: Vec<String>,
}

impl ValueDomain {
    /// Whether the type is a sequence type, whose CSP name depends on the
    /// sequence bound.
    pub fn is_sequence(&self) -> bool {
        matches!(self.ty, Type::Sequence(_))
    }

    /// The type's CSP name in `package`, for sequences of at most
    /// `sequence_bound` items: the nametype of a record, or the `LSeq` type
    /// of a sequence.
    pub fn csp_type(&self, package: &str, sequence_bound: usize) -> String {
        self.ty.csp_type(package, sequence_bound)
    }
}

/// The values a quantifier binder ranges over in FDR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinderDomain {
    pub binder: String,
    /// The binder's RoboChart type, such as `int`.
    pub ty: String,
    /// The CSP literal of each value, in the order given.
    pub values: Vec<String>,
}

/// The [`CspFacts`] of one case that can be generated.
#[derive(Clone, Debug, PartialEq)]
pub struct TestCaseFacts {
    pub id: String,
    /// `expression` or `statement`.
    pub kind: &'static str,
    /// The CSP package of the case's model.
    pub package: String,
    pub facts: CspFacts,
}

/// The facts of every row in `snapshot` that can be generated, from the
/// row's plan, in file order (expression rows first). A row that cannot be
/// generated has no model to prepare, and so no facts.
pub fn csp_facts(snapshot: &Snapshot) -> Vec<TestCaseFacts> {
    let mut facts = Vec::new();
    collect::<Expression>(&snapshot.expressions, &snapshot.shared, &mut facts);
    collect::<Statement>(&snapshot.statements, &snapshot.shared, &mut facts);
    facts
}

fn collect<K: TestCaseKind<Plan = crate::kind::TestCasePlan>>(
    rows: &[K::Row],
    shared: &SharedFixtures,
    out: &mut Vec<TestCaseFacts>,
) {
    for row in rows {
        if let Ok(plan) = K::plan(row, shared) {
            out.push(TestCaseFacts {
                id: K::test_case_id(row).id,
                kind: K::KIND,
                package: plan.machine.package,
                facts: plan.facts,
            });
        }
    }
}

#[cfg(test)]
#[path = "tests/facts.rs"]
mod tests;
