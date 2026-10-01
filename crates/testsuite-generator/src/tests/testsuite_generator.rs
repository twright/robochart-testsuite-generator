use super::*;
use crate::expression::{apply_shared_fixtures, expression_model};
use crate::fixtures::Fixtures;
use crate::shared_fixtures::SharedFixtures;
use serde_json::{Value, json};
use std::collections::BTreeSet;

mod categories;
mod shapes;

#[test]
fn string_inputs_reach_d_without_a_string_literal() {
    let row = |value: &str| -> ExpressionTestCase { serde_json::from_value(json!({
        "id": "type_check/002", "feature": "type_check", "robochart": "a is int",
        "inputs": json!({"a": value}).to_string(), "python_output": "{\"value\": false}"
    })).unwrap() };
    let generated = expression_model(row("a")).unwrap();
    assert!(generated.model.contains("      const a : string\n"));
    assert!(!generated.model.contains("\"a\""));
    assert_eq!(generated.fixture_arguments, ["\"a\""]);
    assert!(expression_model(row("say \"hi\"")).is_err());
}

/// Shared fixtures in the form of `shared_fixtures.json`: a record constant,
/// a sequence constant used by two rows, and a helper function.
fn example_shared_fixtures() -> SharedFixtures {
    serde_json::from_value(json!({
        "fixtures": {
            "obj": {"note": "n", "datatypes": {"Obj": [["value", "int"]]},
                    "constants": {"obj": {"type": "Obj", "value": {"value": 4}}}},
            "items": {"note": "n", "constants": {"items": {"type": "Seq(int)", "value": [0, 1, 2]}}},
            "add": {"note": "n",
                    "functions": ["function add(x : int, y : int) : int { postcondition result == x + y }"],
                    "function_definitions": {"add": {"parameters": ["x", "y"], "body": "x + y"}}}
        },
        "cases": {
            "attribute/000": {"fixtures": ["obj"], "output_type": "int"},
            "index_first/000": {"fixtures": ["items"], "output_type": "int"},
            "index_last/000": {"fixtures": ["items"], "output_type": "int"},
            "call_args/000": {"fixtures": ["add"], "output_type": "int"}
        }
    }))
    .unwrap()
}

/// An untyped expression row with no inputs.
fn literal_row(id: &str, robochart: &str, output: Value) -> ExpressionTestCase {
    serde_json::from_value(json!({
        "id": id, "feature": id.split('/').next().unwrap(), "robochart": robochart,
        "inputs": "{}", "python_output": json!({"value": output}).to_string()
    }))
    .unwrap()
}

#[test]
fn shared_fixtures_apply_to_original_rows_without_changing_their_outputs() {
    let shared = example_shared_fixtures();
    let rows = [
        literal_row("attribute/000", "obj.value", json!(4)),
        literal_row("index_first/000", "items[1]", json!(0)),
        literal_row("index_last/000", "items[size(items)]", json!(2)),
        literal_row("call_args/000", "add(1, 2)", json!(3)),
        literal_row("add/000", "1 + 2", json!(3)),
    ];
    let mut applied = BTreeSet::new();
    for mut case in rows {
        let Some(context) = shared.context(&case.id).unwrap() else { continue };
        let (expression, output) = (case.robochart.clone(), case.python_output.clone());
        apply_shared_fixtures(&mut case, &shared).unwrap();
        assert_eq!((&case.robochart, &case.python_output), (&expression, &output));
        let generated = expression_model(case).unwrap();
        assert!(generated.model.contains(&format!("action output ! ({expression})")));
        for name in context.constants.keys() {
            assert!(generated.model.contains(&format!("const {name} : ")), "{name}");
        }
        applied.insert(generated.id);
    }
    assert_eq!(applied.iter().collect::<Vec<_>>(), shared.test_case_ids().collect::<Vec<_>>());
    // A row that already declares its own context is not overridden.
    let mut conflicting: ExpressionTestCase = serde_json::from_value(json!({
        "id": "attribute/000", "feature": "attribute", "robochart": "obj.value",
        "inputs": "{}", "python_output": "{\"value\": 4}", "output_type": "int"
    })).unwrap();
    assert!(apply_shared_fixtures(&mut conflicting, &shared).is_err());
}

#[test]
fn timed_observation_setup_is_independent_of_expected_output() {
    let row = |value: i64| -> ExpressionTestCase { serde_json::from_value(serde_json::json!({
        "id": "clock/000", "feature": "clock", "robochart": "since(timer)",
        "inputs": "{}", "python_output": format!("{{\"value\":{value}}}"),
        "timing": "{\"ticks\":3,\"max_ticks\":5}"
    })).unwrap() };
    let correct = expression_model(row(3)).unwrap();
    let wrong = expression_model(row(2)).unwrap();
    assert!(correct.timed);
    assert_eq!(correct.model, wrong.model);
    let check = correct.custom_check.unwrap();
    let wrong_check = wrong.custom_check.unwrap();
    assert_eq!(check.split("expected =").next(), wrong_check.split("expected =").next());
    assert!(correct.model.contains("condition since(timer) == 5"));
    assert!(!correct.model.contains("output ! since("));
    assert!(check.contains("tock -> tock -> tock -> sample_event"));
}

#[test]
fn timed_observations_reject_missing_or_unfaithful_contexts() {
    let row = |expression: &str, timing: &str, value: f64| -> ExpressionTestCase { serde_json::from_value(serde_json::json!({
        "id": "clock/000", "feature": "clock", "robochart": expression,
        "inputs": "{}", "python_output": serde_json::json!({"value":value}).to_string(),
        "timing": timing
    })).unwrap() };
    assert!(expression_model(row("since(timer)", "", 3.0)).err().unwrap().to_string().contains("missing timing context"));
    // Without timing metadata, a blocker that no context could remove is reported first.
    assert!(expression_model(row("since(timer)", "", 7.5)).err().unwrap().to_string().contains("fractional real"));
    assert!(expression_model(row("sinceEntry(pkg::state)", "", 3.0)).err().unwrap().to_string().contains("keyword state"));
    assert!(expression_model(row("since(timer)", "{\"ticks\":3,\"max_ticks\":2}", 3.0)).is_err());
    assert!(expression_model(row("since(timer)", "{\"ticks\":3,\"max_ticks\":5}", 7.5)).err().unwrap().to_string().contains("fractional real"));
    assert!(expression_model(row("sinceEntry(pkg::state)", "{\"ticks\":3,\"max_ticks\":5}", 3.0)).err().unwrap().to_string().contains("unqualified"));
    let state = expression_model(row("sinceEntry(observing)", "{\"ticks\":3,\"max_ticks\":5}", 3.0)).unwrap();
    assert!(state.model.contains("state observing { entry ready }"));
    assert!(!state.model.contains("clock timer"));
}

#[test]
fn structured_expression_checks_copy_recorded_values() {
    let rows = [
        literal_row("tuple/000", "(| 1, true |)", json!({"items": [1, true], "type": "tuple"})),
        literal_row("vector/000", "[|1, 2|]", json!([1, 2])),
        literal_row("matrix/000", "[|1, 2; 3, 4|]", json!([[1, 2], [3, 4]])),
    ];
    for row in rows {
        let source = row.robochart.clone();
        let generated = expression_model(row).unwrap();
        assert_eq!(generated.model.matches(&source).count(), 1);
        match generated.feature.as_str() {
            "tuple" => { assert!(generated.model.contains("output : int * boolean")); assert_eq!(generated.expected_csp, "(1,true)"); }
            "vector" => { assert!(generated.model.contains("output : vector(int,2)")); assert_eq!(generated.expected_csp, "(1,2)"); }
            "matrix" => { assert!(generated.model.contains("output : matrix(int,2,2)")); assert_eq!(generated.expected_csp, "(1,2).(3,4)"); }
            _ => unreachable!(),
        }
    }
}

#[test]
fn finite_binder_domains_are_metadata_not_constants() {
    let binder = |id: &str, robochart: &str, domain: Value, output: Value| -> ExpressionTestCase {
        serde_json::from_value(json!({
            "id": id, "feature": id.split('/').next().unwrap(), "robochart": robochart,
            "inputs": json!({"domains": {"int": domain}}).to_string(),
            "python_output": json!({"value": output}).to_string()
        }))
        .unwrap()
    };
    let rows = [
        binder("forall_False/000", "forall x: int @ x == 1", json!([]), json!(true)),
        binder("forall_True/003", "forall x: int | x > 0 @ x == 1", json!([1]), json!(true)),
        binder("exists_False/000", "exists x: int @ x == 1", json!([0, 2]), json!(false)),
        binder("exists_True/001", "exists x: int | x > 0 @ x == 1", json!([-2, -1]), json!(false)),
        binder("exists1_False/004", "exists1 x: int @ x == 1", json!([-1, 0, 1, 2]), json!(true)),
        binder("exists1_True/000", "exists1 x: int | x > 0 @ x == 1", json!([]), json!(false)),
        binder("multiple_variables/002", "forall x: int, y: int @ x == y", json!([0]), json!(true)),
        binder("the_False/002", "the x: int @ x", json!([0]), json!(0)),
        binder("the_True/003", "the x: int | x > 0 @ x", json!([1]), json!(1)),
        binder("comprehension_False_False/002", "{x: int}", json!([0]), json!({"items": [0], "type": "set"})),
        binder("comprehension_False_True/000", "{x: int @ x + 1}", json!([]), json!({"items": [], "type": "set"})),
        binder("comprehension_True_False/000", "{x: int | x > 0}", json!([]), json!({"items": [], "type": "set"})),
        binder("comprehension_True_True/003", "{x: int | x > 0 @ x + 1}", json!([1]), json!({"items": [2], "type": "set"})),
    ];
    for row in rows {
        let source = row.robochart.clone();
        let generated = expression_model(row).unwrap();
        assert!(!generated.model.contains("const domains"));
        assert!(generated.fixture_arguments.is_empty());
        assert!(generated.model.contains(&source));
    }
    assert!(validate_quantifier_domains(&json!({"int": [true]})).is_err());
}

#[test]
fn interval_checks_use_recorded_endpoint_flags() {
    let value = json!({"type": "interval", "start": -3, "end": 2, "left_closed": false, "right_closed": true});
    let fixtures = Fixtures::default();
    assert_eq!(fixtures.csp_literal("Set(int)", &value).unwrap(), "{ interval_value | interval_value <- core_int, -3 < interval_value, interval_value <= 2 }");
    assert!(fixtures.csp_literal("Set(int)", &json!({"type": "interval", "start": 1, "end": null})).is_err());
    // An interval has no RoboChart literal.
    assert!(fixtures.robo_literal("Set(int)", &value).is_err());
}

/// Lambda rows observe one application each: `lambda x: int @ x + 1` and its
/// predicated form, applied to `argument`, giving `argument + 1`.
fn lambda_rows() -> Vec<ExpressionTestCase> {
    let row = |id: &str, robochart: &str, argument: i64| -> ExpressionTestCase {
        serde_json::from_value(json!({
            "id": id, "feature": id.split('/').next().unwrap(), "robochart": robochart,
            "inputs": json!({"apply": [argument]}).to_string(),
            "python_output": json!({"value": argument + 1}).to_string()
        }))
        .unwrap()
    };
    let mut rows: Vec<_> = (-1..3)
        .map(|argument| row(&format!("lambda_False/{:03}", argument + 1), "lambda x: int @ x + 1", argument))
        .collect();
    rows.extend((1..3).map(|argument| row(&format!("lambda_True/{:03}", argument + 1), "lambda x: int | x > 0 @ x + 1", argument)));
    rows
}

#[test]
fn lambda_checks_use_the_recorded_application_results() {
    let rows = lambda_rows();
    for row in &rows {
        let output: Value = serde_json::from_str(&row.python_output).unwrap();
        let source = row.robochart.clone();
        let case = expression_model(row.clone()).unwrap();
        assert_eq!(case.model.matches(&source).count(), 1);
        assert!(case.custom_check.as_ref().unwrap().contains(&format!("output.out.{{ {}}}", output["value"])));
        let mut changed = row.clone();
        changed.python_output = json!({"value": output["value"].as_i64().unwrap() + 99}).to_string();
        let changed = expression_model(changed).unwrap();
        assert_eq!(case.model, changed.model, "the oracle model must not depend on the expected value");
        assert_ne!(case.custom_check, changed.custom_check);
    }
    assert_eq!(rows.len(), 6);
}

#[test]
fn lambda_observations_reject_ambiguous_arguments() {
    let mut row = lambda_rows().remove(0);
    row.inputs = json!({"apply": [0, 1]}).to_string();
    assert!(expression_model(row.clone()).err().unwrap().to_string().contains("exactly one argument"));
    row.inputs = json!({"apply": [0]}).to_string();
    row.python_output = json!({"graph": [{"arg": 0, "defined": false}, {"arg": 0, "defined": true, "value": 1}]}).to_string();
    assert!(expression_model(row.clone()).err().unwrap().to_string().contains("duplicate"));
    row.inputs = json!({"apply": [0, 0]}).to_string();
    assert!(expression_model(row).err().unwrap().to_string().contains("unique"));
}
