//! Property tests of the FDR preparation, on projects holding one random
//! statement row. Moved from the generator's property tests.

use super::prepare::prepare_fdr;
use proptest::prelude::*;
use serde_json::{Map, Value};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use testsuite_generator::test_rows::*;
use testsuite_generator::{Result, StatementTestCase};

/// FDR's `refines` executable: `FDR_REFINES`, or `refines` on PATH, as for
/// `testsuite-runner prepare-fdr`.
fn refines() -> PathBuf {
    env::var_os("FDR_REFINES").map_or_else(|| PathBuf::from("refines"), PathBuf::from)
}

/// The row as the generator's input record.
fn row_of(spec: &Spec) -> StatementTestCase {
    let columns: Map<String, Value> = spec.columns("prop/case").into_iter().map(|(k, v)| (k.to_owned(), Value::String(v))).collect();
    serde_json::from_value(Value::Object(columns)).expect("columns match StatementTestCase")
}

fn integers(value: &Value, out: &mut Vec<i64>) {
    match value {
        Value::Number(n) => out.extend(n.as_i64()),
        Value::Array(items) => items.iter().for_each(|v| integers(v, out)),
        Value::Object(fields) => fields.values().for_each(|v| integers(v, out)),
        _ => {}
    }
}

fn longest_sequence(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.iter().map(longest_sequence).max().unwrap_or(0).max(items.len()),
        Value::Object(fields) => fields.values().map(longest_sequence).max().unwrap_or(0),
        _ => 0,
    }
}

static PROJECTS: AtomicUsize = AtomicUsize::new(0);

/// A minimal project containing one statement row and a stand-in for the CSP
/// that RoboTool would have generated.
fn project_for(spec: &Spec) -> Result<PathBuf> {
    let project = env::temp_dir().join(format!(
        "robochart-properties-{}-{}",
        std::process::id(),
        PROJECTS.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&project);
    fs::create_dir_all(project.join("source-data"))?;
    fs::create_dir_all(project.join("csp-gen"))?;
    fs::write(project.join("source-data/cases.csv"), "id,feature,robochart,inputs,python_output\n")?;
    let row = row_of(&spec);
    let mut writer = csv::Writer::from_path(project.join("source-data/statement_cases.csv"))?;
    writer.write_record([
        "id", "feature", "robochart", "inputs", "python_output", "types", "observe", "expected_status",
        "datatypes", "functions", "events", "input_events", "driver", "iteration", "function_definitions",
        "notes", "domains", "constants",
    ])?;
    writer.write_record([
        &row.id, &row.feature, &row.robochart, &row.inputs, &row.python_output, &row.types, &row.observe,
        &row.expected_status, &row.datatypes, &row.functions, &row.events, &row.input_events, &row.driver,
        &row.iteration, &row.function_definitions, &row.notes, &row.domains, &row.constants,
    ])?;
    writer.flush()?;
    fs::write(
        project.join("csp-gen/instantiations.csp"),
        "-- generate int\nnametype core_int = { -2..2}\n\n-- generate nat\nnametype core_nat = {0..2}\n\n-- generate boolean\nnametype core_boolean = Bool\n\n-- generate Plus\nPlus(e1,e2,T) = if member(e1+e2,T) then e1+e2 else e1\n-- generate Minus\nMinus(e1,e2,T) = if member(e1-e2,T) then e1-e2 else e1\n-- generate Mult\nMult(e1,e2,T) = if member(e1*e2,T) then e1*e2 else e1\n-- generate Div\nDiv(e1,e2,T) = if member(e1/e2,T) then e1/e2 else e1\n-- generate Modulus\nModulus(e1,e2,T) = if member(e1%e2,T) then e1%e2 else e1\n-- generate Neg\nNeg(e1,T) = if member(-e1,T) then -e1 else e1\n",
    )?;
    fs::write(project.join("csp-gen/stub.csp"), "A = LSeq(core_int,2)\nB = LSeq(LSeq(core_boolean,2),2)\n")?;
    Ok(project)
}

fn range(text: &str, name: &str) -> (i64, i64) {
    let line = text.lines().find(|l| l.starts_with(&format!("nametype {name} = "))).unwrap();
    let inner = line.split('{').nth(1).unwrap().trim_end_matches('}');
    let (low, high) = inner.split_once("..").unwrap();
    (low.trim().parse().unwrap(), high.trim().parse().unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Preparing the FDR context twice gives the same files as preparing it
    /// once, and the finite domains cover every integer and sequence length
    /// that the row's values and iteration index use.
    #[test]
    fn fdr_preparation_is_idempotent_and_covers_the_fixture(spec in arb_spec()) {
        let project = project_for(&spec).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let read = |name: &str| fs::read_to_string(project.join("csp-gen").join(name)).unwrap();
        let first = prepare_fdr(&project, &refines()).map(|_| (read("instantiations.csp"), read("stub.csp")));
        let second = prepare_fdr(&project, &refines()).map(|_| (read("instantiations.csp"), read("stub.csp")));
        let _ = fs::remove_dir_all(&project);
        let (instantiations, stub) = first.map_err(|e| TestCaseError::fail(e.to_string()))?;
        let again = second.map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&again, &(instantiations.clone(), stub.clone()));

        let mut values: Vec<Value> = spec.vars.iter().map(|(_, _, v)| v.clone()).collect();
        values.extend(spec.state.values().cloned());
        values.extend(spec.trace.iter().filter_map(|(_, v)| v.clone()));
        values.extend(spec.driver.iter().filter_map(|(_, b)| b.as_ref().map(|(_, v)| v.clone())));
        let mut ints = Vec::new();
        values.iter().for_each(|v| integers(v, &mut ints));
        ints.extend(1..=spec.iteration.unwrap_or(0) as i64);
        // The bound sections the preparation rewrites are annotated so that
        // RoboTool keeps them when it regenerates the CSP.
        for name in ["int", "nat"] {
            let kept = format!("-- generate {name} not\n");
            let plain = format!("-- generate {name}\n");
            prop_assert_eq!(instantiations.matches(&kept).count(), 1, "{}", name);
            prop_assert!(!instantiations.contains(&plain), "{} is not annotated", name);
        }
        // RoboTool's arithmetic is part of the translation under test, so the
        // preparation leaves its operator definitions exactly as generated.
        for (name, definition) in [
            ("Plus", "Plus(e1,e2,T) = if member(e1+e2,T) then e1+e2 else e1"),
            ("Minus", "Minus(e1,e2,T) = if member(e1-e2,T) then e1-e2 else e1"),
            ("Mult", "Mult(e1,e2,T) = if member(e1*e2,T) then e1*e2 else e1"),
            ("Div", "Div(e1,e2,T) = if member(e1/e2,T) then e1/e2 else e1"),
            ("Modulus", "Modulus(e1,e2,T) = if member(e1%e2,T) then e1%e2 else e1"),
            ("Neg", "Neg(e1,T) = if member(-e1,T) then -e1 else e1"),
        ] {
            let section = format!("-- generate {name}\n{definition}\n");
            prop_assert_eq!(instantiations.matches(&section).count(), 1, "{}", name);
        }
        let (low, high) = range(&instantiations, "core_int");
        let (_, nat_high) = range(&instantiations, "core_nat");
        for n in &ints {
            prop_assert!(low <= *n && *n <= high, "{} outside core_int {{{}..{}}}", n, low, high);
            prop_assert!(*n < 0 || *n <= nat_high, "{} outside core_nat {{0..{}}}", n, nat_high);
        }
        let longest = values.iter().map(longest_sequence).max().unwrap_or(0).max(2);
        prop_assert!(nat_high >= longest as i64);
        // The sequence bound is exactly the longest sequence value (at least
        // the default 2): large enough, and not inflated by driver scripts or
        // event traces, which are lists of steps rather than values.
        for (i, _) in stub.match_indices("LSeq(") {
            let bound: String = stub[i..].split(',').nth(1).unwrap().chars().take_while(char::is_ascii_digit).collect();
            prop_assert_eq!(bound.parse::<usize>().unwrap(), longest, "{}", stub);
        }
    }
}

/// The runner qualifies each generated case's facts by its package, and its
/// sequence types use the snapshot's sequence bound; `core_nat` covers the
/// longest sequence.
#[test]
fn fdr_preparation_is_built_from_the_generator_facts() {
    use robotool_harness::{DomainType, FunctionInstance};
    use testsuite_generator::facts::Snapshot;
    let row: StatementTestCase = serde_json::from_value(serde_json::json!({
        "id": "helper/pass", "feature": "helper", "robochart": "y = increment(x)",
        "inputs": "{\"x\":1,\"y\":0,\"s\":[1,1,1,1]}", "python_output": "{\"state\":{\"x\":1,\"y\":2}}",
        "types": "{\"x\":\"int\",\"y\":\"int\",\"s\":\"Seq(int)\"}", "observe": "[\"x\",\"y\"]", "expected_status": "pass",
        "functions": "function increment(x: int): int { postcondition result == x + 1 }",
        "function_definitions": "{\"increment\": {\"parameters\": [\"x\"], \"body\": \"x + 1\"}}",
        "domains": "{\"values\":{\"Seq(int)\":[[1,1,1,1]]}}", "constants": "[\"x\"]"
    }))
    .unwrap();
    let snapshot = Snapshot { statements: vec![row], ..Snapshot::default() };
    let preparation = super::prepare::fdr_preparation(&snapshot).unwrap();
    assert_eq!(preparation.core_int, (-2, 2));
    assert_eq!((preparation.sequence_bound, preparation.core_nat_max, preparation.clock_max), (4, 4, 10));
    assert_eq!(preparation.sizing_constants, std::collections::BTreeMap::from([("x".to_owned(), 1)]));
    assert_eq!(
        preparation.functions,
        [FunctionInstance {
            package: "statement_helper_pass".into(),
            name: "increment".into(),
            parameters: vec!["x".into()],
            body: "x + 1".into(),
        }]
    );
    let [domain] = preparation.value_domains.as_slice() else { panic!("{:?}", preparation.value_domains) };
    assert_eq!(domain.package, "statement_helper_pass");
    assert_eq!(domain.ty, DomainType::Sequence("LSeq(core_int,4)".into()));
    assert_eq!(domain.values, ["<1,1,1,1>"]);
    assert!(preparation.binder_domains.is_empty());
}

/// A row whose `timing` does not parse cannot be generated, and must not
/// stop a project from being prepared, nor change the preparation of its
/// other rows.
#[test]
fn malformed_timing_in_a_failing_row_does_not_break_preparation() {
    let refines = PathBuf::from("no-refines");
    let untimed = "plain/000,plain,1 + 1,{},\"{\"\"value\"\": 2}\",\n";
    let prepared = |rows: &str| -> Result<String> {
        let project = env::temp_dir().join(format!(
            "robochart-timing-{}-{}",
            std::process::id(),
            PROJECTS.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join("source-data"))?;
        fs::create_dir_all(project.join("csp-gen"))?;
        fs::write(project.join("source-data/cases.csv"), format!("id,feature,robochart,inputs,python_output,timing\n{rows}"))?;
        fs::write(project.join("source-data/statement_cases.csv"), "id,feature,robochart,inputs,python_output,types,observe,expected_status\n")?;
        fs::write(
            project.join("csp-gen/instantiations.csp"),
            "-- generate int\nnametype core_int = { -2..2}\n\n-- generate nat\nnametype core_nat = {0..2}\n",
        )?;
        let result = prepare_fdr(&project, &refines).and_then(|_| Ok(fs::read_to_string(project.join("csp-gen/instantiations.csp"))?));
        let _ = fs::remove_dir_all(&project);
        result
    };
    let alone = prepared(untimed).unwrap();
    let malformed = "since/000,since,since(c),{},\"{\"\"value\"\": 1}\",\"{\"\"ticks\"\": 1, \"\"max_ticks\"\": \"\n";
    let with_malformed = prepared(&format!("{malformed}{untimed}")).unwrap();
    assert_eq!(with_malformed, alone);
    let snapshot = testsuite_generator::facts::Snapshot {
        expressions: vec![
            serde_json::from_value(serde_json::json!({
                "id": "since/000", "feature": "since", "robochart": "since(c)", "inputs": "{}",
                "python_output": "{\"value\": 1}", "timing": "{\"ticks\": 1, \"max_ticks\": "
            }))
            .unwrap(),
            serde_json::from_value(serde_json::json!({
                "id": "plain/000", "feature": "plain", "robochart": "1 + 1", "inputs": "{}", "python_output": "{\"value\": 2}"
            }))
            .unwrap(),
        ],
        ..Default::default()
    };
    let both = super::prepare::fdr_preparation(&snapshot).unwrap();
    assert_eq!(both, super::prepare::fdr_preparation(&snapshot.only("plain/000")).unwrap());
    assert_eq!(both.clock_max, 10);
}
