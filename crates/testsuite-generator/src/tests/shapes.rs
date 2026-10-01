//! The shape table: precedence is the list order.

use crate::ExpressionTestCase;
use crate::expression::expression_model;
use serde_json::{Value, json};

/// An untyped expression row with `columns` replaced.
pub(super) fn expression_row(columns: Value) -> ExpressionTestCase {
    let mut row = json!({
        "id": "plain/000", "feature": "plain", "robochart": "1 + 1",
        "inputs": "{}", "python_output": "{\"value\": 2}"
    });
    row.as_object_mut().unwrap().extend(columns.as_object().unwrap().clone());
    serde_json::from_value(row).unwrap()
}

#[test]
fn shape_rules_apply_in_precedence_order() {
    use crate::expression::shapes::{SHAPE_RULES, shape_of};
    let names: Vec<_> = SHAPE_RULES.iter().map(|rule| rule.name).collect();
    assert_eq!(names, ["timed_clock", "since_without_timing", "unsupported_feature", "lambda_graph", "scalar"]);
    // Each row also satisfies every later rule but the last, so the first
    // rule that applies decides its shape.
    let lambda = "lambda x: int @ x";
    let timing = "{\"ticks\":1,\"max_ticks\":2}";
    let graph = "{\"graph\": []}";
    let rows = [
        (json!({"timing": timing, "robochart": "since(t)", "types": "{}", "feature": "record_empty", "python_output": graph}), "timed_clock"),
        (json!({"robochart": "since(t)", "types": "{}", "feature": "record_empty", "python_output": graph}), "since_without_timing"),
        (json!({"output_type": "int", "feature": "special_real", "robochart": lambda}), "unsupported_feature"),
        (json!({"functions": "function f(): int {}", "feature": "record_empty", "python_output": graph}), "unsupported_feature"),
        (json!({"feature": "special_real", "robochart": lambda}), "unsupported_feature"),
        (json!({"feature": "record_empty", "python_output": graph}), "unsupported_feature"),
        (json!({"robochart": lambda}), "lambda_graph"),
        (json!({"output_type": "int", "types": "{}", "robochart": lambda}), "lambda_graph"),
        (json!({"python_output": graph}), "lambda_graph"),
        (json!({"timing": "  ", "robochart": " since(t)"}), "since_without_timing"),
        (json!({}), "scalar"),
        (json!({"output_type": "int", "types": "{}"}), "scalar"),
    ];
    for (columns, expected) in rows {
        let row = expression_row(columns.clone());
        assert_eq!(shape_of(&row).name, expected, "{columns}");
    }
    // The shape that applies is the one that generates the row.
    let empty_record_lambda = expression_row(json!({"feature": "record_empty", "id": "record_empty/000", "robochart": lambda}));
    assert_eq!(expression_model(empty_record_lambda).err().unwrap().reason, "RoboChart record literals require at least one field");
}
