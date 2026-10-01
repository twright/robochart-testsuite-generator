//! The context rules of scalar expressions, and the checks on the context.

use super::*;
use crate::rejection::Category;
use crate::shared_fixtures;
use serde_json::{Value, json};

/// An expression row with `columns` replaced.
fn row(columns: Value) -> ExpressionTestCase {
    let mut row = json!({
        "id": "plain/000", "feature": "plain", "robochart": "1 + 1",
        "inputs": "{}", "python_output": "{\"value\": 2}"
    });
    row.as_object_mut().unwrap().extend(columns.as_object().unwrap().clone());
    serde_json::from_value(row).unwrap()
}

fn build(columns: Value) -> Result<ScalarContext> {
    ScalarContext::build(&row(columns), SCALAR_CONTEXT_RULES)
}

fn int() -> Type {
    Type::Int
}

fn set_of_int() -> Type {
    Type::Set(Box::new(Type::Int))
}

#[test]
fn scalar_context_rules_run_in_their_listed_order() {
    let names: Vec<_> = SCALAR_CONTEXT_RULES.iter().map(|rule| rule.name()).collect();
    assert_eq!(names, [
        "declared_datatypes", "declared_functions", "declared_output_type", "vector_output_type",
        "matrix_output_type", "inferred_output_type", "declared_inputs", "inferred_inputs",
        "shared_fixture_constants", "membership_items", "record_r", "quantifier_binders",
    ]);
}

#[test]
fn inferred_inputs_type_each_value_and_name_their_origin() {
    let context = build(json!({
        "inputs": json!({"a": 1, "b": [true], "s": "text", "domain": [2, 1], "domains": {"int": [0]}}).to_string(),
        "python_output": "{\"value\": {\"type\": \"set\", \"items\": [1]}}"
    }))
    .unwrap();
    let constant = |name: &str| context.constants.get(name).cloned();
    assert_eq!(constant("a").map(|c| (c.ty, c.init, c.origin)), Some((int(), Init::Literal, "inferred_inputs")));
    assert_eq!(constant("b").unwrap().ty, Type::Sequence(Box::new(Type::Boolean)));
    // A string is declared without a value (RoboTool bug B9).
    assert_eq!(constant("s").map(|c| (c.ty, c.init)), Some((Type::String, Init::Uninitialised)));
    // `domain` is a set of integers, `domains` is binder metadata.
    assert_eq!(constant("domain").unwrap().ty, set_of_int());
    assert_eq!(constant("domains"), None);
    assert_eq!(context.binder_domains, Some((json!({"int": [0]}), "quantifier_binders")));
    assert_eq!(context.output, Some((set_of_int(), "inferred_output_type")));
    assert_eq!(context.origin("a"), Some("inferred_inputs"));
}

#[test]
fn feature_rules_add_their_context() {
    let membership = build(json!({"feature": "membership", "id": "membership/000", "inputs": "{\"a\": 1}", "python_output": "{\"value\": true}"})).unwrap();
    let items = &membership.constants["items"];
    assert_eq!((&items.ty, &items.value, items.origin), (&set_of_int(), &json!([0, 1, 2]), "membership_items"));

    let record = build(json!({"feature": "record", "id": "record/000", "python_output": "{\"value\": {\"type\": \"record\", \"fields\": {\"x\": 1, \"y\": 2}}}"})).unwrap();
    assert_eq!(record.fixtures.declarations(), "datatype R { x : int y : int }\n");
    assert_eq!(record.origin("R"), Some("record_r"));
    assert_eq!(record.output, Some((Type::Record("R".into()), "inferred_output_type")));

    let binders = build(json!({"robochart": "forall x: int, y: int @ x == y", "python_output": "{\"value\": true}"})).unwrap();
    assert_eq!(binders.variables.keys().collect::<Vec<_>>(), ["x", "y"]);
    assert_eq!(binders.variables["x"].origin, "quantifier_binders");
    assert!(binders.constants.is_empty());

    let vector = build(json!({"feature": "vector", "id": "vector/000", "python_output": "{\"value\": [1, 2]}"})).unwrap();
    assert_eq!(vector.output, Some((Type::Vector(Box::new(int()), 2), "vector_output_type")));
    let matrix = build(json!({"feature": "transpose", "id": "transpose/000", "python_output": "{\"value\": [[1], [2]]}"})).unwrap();
    assert_eq!(matrix.output, Some((Type::Matrix(Box::new(int()), 2, 1), "matrix_output_type")));
}

#[test]
fn declared_columns_and_shared_fixture_constants_are_declared() {
    let mut case = row(json!({
        "inputs": "{\"p\": {\"value\": 3}}", "types": "{\"p\": \"Point\"}", "output_type": "int",
        "datatypes": "{\"Point\": [[\"value\", \"int\"]]}",
        "functions": "function f(a : int) : int { postcondition result == a }",
        "function_definitions": "{\"f\": {\"parameters\": [\"a\"], \"body\": \"a\"}}"
    }));
    case.fixture_constants.insert("k".into(), shared_fixtures::Constant { ty: "Seq(int)".into(), value: json!([1]) });
    let context = ScalarContext::build(&case, SCALAR_CONTEXT_RULES).unwrap();
    assert_eq!(context.constants["p"].origin, "declared_inputs");
    assert_eq!(context.constants["p"].ty, Type::Record("Point".into()));
    assert_eq!(context.constants["k"].origin, "shared_fixture_constants");
    assert_eq!(context.output, Some((int(), "declared_output_type")));
    assert_eq!(context.origin("Point"), Some("declared_datatypes"));
    assert_eq!(context.origin("f"), Some("declared_functions"));
    assert_eq!(context.functions.len(), 1);
}

#[test]
fn a_name_declared_twice_is_rejected() {
    let rejection = build(json!({"feature": "membership", "id": "membership/000", "inputs": "{\"items\": 1}", "python_output": "{\"value\": true}"})).unwrap_err();
    assert_eq!(rejection.category, Category::FixtureGenerationError);
    assert_eq!(rejection.reason, "items is declared by both inferred_inputs and membership_items");
    let rejection = build(json!({"robochart": "exists x: int @ x == 1", "inputs": "{\"x\": 1}", "python_output": "{\"value\": true}"})).unwrap_err();
    assert_eq!(rejection.reason, "x is declared by both inferred_inputs and quantifier_binders");
    let rejection = build(json!({"feature": "record", "id": "record/000", "datatypes": "{\"R\": [[\"z\", \"int\"]]}", "output_type": "int"})).unwrap_err();
    assert_eq!(rejection.reason, "R is declared by both declared_datatypes and record_r");
    let mut context = ScalarContext::new(&row(json!({}))).unwrap();
    context.declare_output(int(), "first").unwrap();
    assert_eq!(context.declare_output(int(), "second").unwrap_err().reason, "the output type is declared by both first and second");
}

#[test]
fn check_rejects_undeclared_types_and_a_missing_output() {
    // A recorded record has type R, which only `record` rows declare.
    let rejection = build(json!({"python_output": "{\"value\": {\"type\": \"record\", \"fields\": {\"x\": 1}}}"})).unwrap_err();
    assert_eq!(rejection.category, Category::UnsupportedFixtureType);
    assert!(rejection.reason.contains("unknown record type R"), "{}", rejection.reason);
    let rejection = build(json!({"inputs": "{\"a\": 1}", "types": "{\"a\": \"Unknown\"}", "output_type": "int"})).unwrap_err();
    assert_eq!(rejection.category, Category::UnsupportedFixtureType);
    assert!(rejection.reason.contains("unknown record type Unknown in a"), "{}", rejection.reason);
    let mut context = ScalarContext::new(&row(json!({}))).unwrap();
    assert_eq!(context.check().unwrap_err().category, Category::FixtureGenerationError);
    context.declare_constant("c", Type::Record("Missing".into()), json!({}), "test").unwrap();
    context.declare_output(int(), "test").unwrap();
    assert_eq!(context.check().unwrap_err().category, Category::UnsupportedFixtureType);
}

#[test]
fn origins_are_written_as_comments_in_the_model() {
    let case = row(json!({"feature": "membership", "id": "membership/000", "robochart": "a in items", "inputs": "{\"a\": 1}", "python_output": "{\"value\": true}"}));
    let model = crate::expression::expression_model(case).unwrap().model;
    assert!(model.contains(
        "      // context rule: inferred_inputs\n      const a : int = 1\n      // context rule: membership_items\n      const items : Set(int) = { 0,1,2 }\n      // context rule: inferred_output_type\n      event output : boolean\n"
    ), "{model}");
}
