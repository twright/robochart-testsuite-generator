//! Rejections carry the category of the site that raises them.

use super::shapes::expression_row;
use crate::rejection::Category;
use crate::shared_fixtures::SharedFixtures;
use crate::statement::statement_model;
use crate::StatementTestCase;
use serde_json::{Value, json};

#[test]
fn category_strings_are_those_written_to_generation_failures() {
    let strings: Vec<_> = Category::ALL.iter().map(|category| category.as_str()).collect();
    assert_eq!(strings, [
        "oracle_representation_limit", "missing_fixture_context", "unsupported_robochart_syntax",
        "unsupported_fixture_type", "unsupported_statement", "unsupported_recursion", "invalid_fixture_value",
        "invalid_function_fixture", "invalid_oracle", "invalid_identifier", "fixture_generation_error",
    ]);
}

#[test]
fn rejections_carry_the_category_of_their_site() {
    use crate::expression::Expression;
    use crate::kind::{TestCaseKind, feature_rejection};
    use crate::rejection::Category::*;
    let none = SharedFixtures::default();
    let typed = |columns: Value| {
        let mut columns = columns;
        columns.as_object_mut().unwrap().entry("types").or_insert(json!("{}"));
        columns.as_object_mut().unwrap().entry("output_type").or_insert(json!("int"));
        expression_row(columns)
    };
    let expressions = [
        // Inferred types (expression/context.rs) and the literal engine (fixtures.rs).
        (expression_row(json!({"python_output": "{\"value\": 1.5}"})), OracleRepresentationLimit),
        (expression_row(json!({"python_output": "{\"value\": {\"type\": \"interval\", \"start\": 1, \"end\": null}}"})), UnsupportedRobochartSyntax),
        (expression_row(json!({"python_output": "{\"value\": {\"type\": \"tuple\", \"items\": [1]}}"})), UnsupportedRobochartSyntax),
        (expression_row(json!({"python_output": "{\"value\": {\"type\": \"tuple\"}}"})), InvalidFixtureValue),
        (expression_row(json!({"python_output": "{\"value\": {\"type\": \"other\"}}"})), UnsupportedFixtureType),
        (expression_row(json!({"inputs": "{\"domain\": [true]}"})), InvalidFixtureValue),
        (expression_row(json!({"feature": "vector", "id": "vector/000"})), InvalidFixtureValue),
        (expression_row(json!({"python_output": "{\"value\": [1, true]}"})), FixtureGenerationError),
        (expression_row(json!({"python_output": "{}"})), InvalidOracle),
        (expression_row(json!({"inputs": "{\"1x\": 1}"})), InvalidIdentifier),
        (expression_row(json!({"inputs": "{\"domains\": {\"int\": [true]}}"})), InvalidFixtureValue),
        // Test case identity (lib.rs).
        (expression_row(json!({"id": "plain/000/1"})), InvalidIdentifier),
        (expression_row(json!({"id": "plain/"})), InvalidIdentifier),
        // Clock shapes (shapes.rs, timed.rs).
        (expression_row(json!({"robochart": "since(t)"})), MissingFixtureContext),
        (expression_row(json!({"robochart": "sinceEntry(state)"})), UnsupportedRobochartSyntax),
        (expression_row(json!({"robochart": "since(t)", "timing": "{\"ticks\":3,\"max_ticks\":2}"})), FixtureGenerationError),
        (expression_row(json!({"robochart": "since(output)", "timing": "{\"ticks\":1,\"max_ticks\":2}"})), InvalidIdentifier),
        // Lambda graphs (lambda.rs).
        (expression_row(json!({"robochart": "lambda x: int @ x", "inputs": "{\"apply\": [0, 0]}"})), FixtureGenerationError),
        (expression_row(json!({"robochart": "lambda x @ x", "inputs": "{\"apply\": [0]}"})), InvalidFixtureValue),
        // Typed rows: datatypes, types and functions (fixtures.rs, lib.rs).
        (typed(json!({"datatypes": "{\"A\": [[\"b\", \"A\"]]}"})), UnsupportedRecursion),
        (typed(json!({"output_type": "Unknown"})), UnsupportedFixtureType),
        (typed(json!({"output_type": "int extra"})), UnsupportedFixtureType),
        (typed(json!({"output_type": "vector(int, 0)"})), UnsupportedFixtureType),
        (typed(json!({"python_output": "{\"value\": true}"})), InvalidFixtureValue),
        (typed(json!({"datatypes": "{\"int\": [[\"a\", \"int\"]]}"})), FixtureGenerationError),
        (typed(json!({"functions": "function f(): int {}"})), InvalidFunctionFixture),
        (typed(json!({"functions": "function f(): int {}", "function_definitions": "{\"f\": {\"parameters\": [], \"body\": \"f\"}}"})), UnsupportedRecursion),
        (typed(json!({"inputs": "{\"a\": 1}"})), FixtureGenerationError),
        // JSON columns: serde_json's classification of its errors.
        (expression_row(json!({"inputs": "x"})), FixtureGenerationError),
        (expression_row(json!({"inputs": "{"})), FixtureGenerationError),
        (expression_row(json!({"inputs": "{} x"})), FixtureGenerationError),
        (expression_row(json!({"robochart": "since(t)", "timing": "{\"ticks\":1}"})), InvalidFixtureValue),
        (expression_row(json!({"robochart": "since(t)", "timing": "{\"ticks\":-1,\"max_ticks\":2}"})), InvalidFixtureValue),
        (expression_row(json!({"robochart": "since(t)", "timing": "{\"ticks\":1,\"ticks\":1,\"max_ticks\":2}"})), InvalidFixtureValue),
    ];
    for (row, expected) in expressions {
        let rejection = Expression::plan(&row, &none).err().unwrap_or_else(|| panic!("{} was accepted", row.robochart));
        assert_eq!(rejection.category, expected, "{}", rejection.reason);
    }
    // Shared fixtures merge before dispatch; a conflict is rejected there.
    let shared: SharedFixtures = serde_json::from_value(json!({
        "fixtures": {"obj": {"note": "n", "datatypes": {"Obj": [["value", "int"]]},
                             "constants": {"obj": {"type": "Obj", "value": {"value": 4}}}}},
        "cases": {"attribute/000": {"fixtures": ["obj"], "output_type": "int"}}
    }))
    .unwrap();
    let id = shared.test_case_ids().next().unwrap().clone();
    let conflicting = expression_row(json!({"id": id, "feature": id.split('/').next().unwrap(), "output_type": "int"}));
    assert_eq!(Expression::plan(&conflicting, &shared).err().unwrap().category, FixtureGenerationError);
    // Statement rows (statement/mod.rs).
    let statement = |columns: Value| -> StatementTestCase {
        let mut row = json!({
            "id": "assign/000", "feature": "assign", "robochart": "x = 1",
            "inputs": "{\"x\": 0, \"i\": 0}", "python_output": "{\"state\": {\"x\": 1}}",
            "types": "{\"x\": \"int\", \"i\": \"int\"}", "observe": "[\"x\"]", "expected_status": "pass"
        });
        row.as_object_mut().unwrap().extend(columns.as_object().unwrap().clone());
        serde_json::from_value(row).unwrap()
    };
    assert!(statement_model(statement(json!({}))).is_ok());
    for (row, expected) in [
        (statement(json!({"iteration": "{\"index\": \"i\", \"count\": 65}"})), UnsupportedStatement),
        (statement(json!({"iteration": "{\"index\": \"x\", \"count\": 2}", "types": "{\"x\": \"int\", \"i\": \"boolean\"}"})), InvalidFixtureValue),
        (statement(json!({"iteration": "{\"index\": \"i\", \"count\": 2}", "inputs": "{\"x\": 0, \"i\": false}", "types": "{\"x\": \"int\", \"i\": \"boolean\"}"})), FixtureGenerationError),
        (statement(json!({"expected_status": "other"})), FixtureGenerationError),
        (statement(json!({"python_output": "{\"state\": {}}"})), InvalidFixtureValue),
        (statement(json!({"python_output": "{}"})), InvalidOracle),
        (statement(json!({"observe": "[\"x\", \"x\"]"})), FixtureGenerationError),
        (statement(json!({"datatypes": "{\"Obs\": [[\"a\", \"int\"]]}"})), FixtureGenerationError),
    ] {
        let rejection = statement_model(row).err().unwrap();
        assert_eq!(rejection.category, expected, "{}", rejection.reason);
    }
    // A record_empty row is unsupported syntax whatever rejects it, unless
    // it is rejected for an oracle limit or missing context.
    // (A scalar record_empty row is rejected by its shape; a timed one is not.)
    let empty_record = expression_row(json!({"feature": "record_empty", "id": "record_empty/000", "robochart": "since(t)", "timing": "{\"ticks\":3,\"max_ticks\":2}"}));
    let rejection = Expression::plan(&empty_record, &none).err().unwrap();
    assert_eq!(rejection.category, FixtureGenerationError);
    assert_eq!(feature_rejection("record_empty", rejection.clone()).category, UnsupportedRobochartSyntax);
    assert_eq!(feature_rejection("other", rejection).category, FixtureGenerationError);
    let scalar = expression_row(json!({"feature": "record_empty", "id": "record_empty/000", "output_type": "Unknown", "types": "{}"}));
    assert_eq!(Expression::plan(&scalar, &none).err().unwrap().category, UnsupportedRobochartSyntax);
    for kept in [OracleRepresentationLimit, MissingFixtureContext] {
        assert_eq!(feature_rejection("record_empty", kept.reject("r")).category, kept);
    }
}
