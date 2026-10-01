//! The FDR preparation facts: extents over every row, and the facts of each
//! generated case's plan.

use super::*;
use serde_json::{Value, json};

fn expression(columns: Value) -> ExpressionTestCase {
    let mut row = json!({
        "id": "plain/000", "feature": "plain", "robochart": "1 + 1",
        "inputs": "{}", "python_output": "{\"value\": 2}"
    });
    row.as_object_mut().unwrap().extend(columns.as_object().unwrap().clone());
    serde_json::from_value(row).unwrap()
}

fn statement(columns: Value) -> StatementTestCase {
    let mut row = json!({
        "id": "assign/pass", "feature": "assign", "robochart": "y = x",
        "inputs": "{\"x\":1,\"y\":0}", "python_output": "{\"state\":{\"x\":1,\"y\":1}}",
        "types": "{\"x\":\"int\",\"y\":\"int\"}", "observe": "[\"x\",\"y\"]", "expected_status": "pass"
    });
    row.as_object_mut().unwrap().extend(columns.as_object().unwrap().clone());
    serde_json::from_value(row).unwrap()
}

fn snapshot(expressions: Vec<ExpressionTestCase>, statements: Vec<StatementTestCase>) -> Snapshot {
    Snapshot { expressions, statements, shared: SharedFixtures::default() }
}

#[test]
fn an_empty_snapshot_has_robotools_default_extents() {
    let extents = extents(&Snapshot::default()).unwrap();
    assert_eq!(
        extents,
        Extents { int_range: (-2, 2), sequence_length: 2, clock_max: 10, constants: BTreeMap::new() }
    );
}

#[test]
fn extents_cover_rows_that_fail_generation() {
    // Neither row can be generated: the first records no value, the second
    // is a feature whose expression is not a RoboChart value.
    let rows = vec![
        expression(json!({"id": "plain/001", "inputs": "{\"a\": [1, 2, 3, 4]}", "python_output": "{}"})),
        expression(json!({"id": "special_real/000", "feature": "special_real", "robochart": "-40 + x"})),
    ];
    let snapshot = snapshot(rows, Vec::new());
    for row in &snapshot.expressions {
        assert!(Expression::plan(row, &snapshot.shared).is_err(), "{} should fail generation", row.id);
    }
    let extents = extents(&snapshot).unwrap();
    // `-40` in the source text counts as -40 as well as 40.
    assert_eq!(extents.int_range, (-40, 40));
    assert_eq!(extents.sequence_length, 4);
    assert!(csp_facts(&snapshot).is_empty());
}

#[test]
fn extents_read_every_column_that_holds_values() {
    let statements = vec![
        statement(json!({
            "id": "scan/pass",
            "inputs": "{\"x\":1,\"y\":0}",
            "driver": "[{\"event\": \"go\", \"value\": [7, 8, 9]}, {\"event\": \"go\"}, {\"event\": \"go\"}, {\"event\": \"go\"}, {\"event\": \"go\"}]",
            "iteration": "{\"index\": \"x\", \"count\": 12}",
            "domains": "{\"sequence_length\": 6}"
        })),
    ];
    let expressions = vec![expression(json!({
        "id": "since/000", "feature": "since", "robochart": "since(c)",
        "timing": "{\"ticks\": 3, \"max_ticks\": 30}", "python_output": "{\"value\": 3}"
    }))];
    let extents = extents(&snapshot(expressions, statements)).unwrap();
    assert_eq!(extents.int_range, (-2, 30));
    // The driver is a list of five steps, not a sequence value; only its
    // payload counts, but the explicit `sequence_length` is longer still.
    assert_eq!(extents.sequence_length, 6);
    assert_eq!(extents.clock_max, 31);
}

#[test]
fn extents_keep_only_constants_with_one_value() {
    let expressions = vec![
        expression(json!({"id": "plain/001", "inputs": "{\"a\": 1, \"b\": 1, \"s\": \"text\"}"})),
        expression(json!({"id": "plain/002", "inputs": "{\"a\": 1, \"b\": 2}"})),
    ];
    // A statement row's inputs are constants only if its `constants` column
    // says so.
    let statements = vec![statement(json!({"inputs": "{\"x\":5,\"y\":0}", "constants": "[\"x\"]"}))];
    let extents = extents(&snapshot(expressions, statements)).unwrap();
    assert_eq!(extents.constants, BTreeMap::from([("a".to_owned(), 1), ("x".to_owned(), 5)]));
}

#[test]
fn extents_include_the_shared_fixture_context() {
    let shared: SharedFixtures = serde_json::from_value(json!({
        "fixtures": {"scale": {
            "note": "a constant",
            "constants": {"k": {"type": "Seq(int)", "value": [50, 1, 2]}}
        }},
        "cases": {"plain/000": {"fixtures": ["scale"], "output_type": "int"}}
    }))
    .unwrap();
    let snapshot = Snapshot { expressions: vec![expression(json!({}))], statements: Vec::new(), shared };
    let extents = extents(&snapshot).unwrap();
    assert_eq!(extents.int_range, (-2, 50));
    assert_eq!(extents.sequence_length, 3);
}

#[test]
fn extents_skip_timing_that_does_not_parse() {
    let untimed = expression(json!({"id": "plain/001"}));
    let malformed = [
        "{\"ticks\": 3, \"max_ticks\": 90",
        "{\"ticks\": 3}",
        "{\"ticks\": 3, \"max_ticks\": 90, \"extra\": 1}",
    ];
    for timing in malformed {
        let row = expression(json!({"id": "since/000", "feature": "since", "robochart": "since(c)", "timing": timing}));
        let both = snapshot(vec![row, untimed.clone()], Vec::new());
        assert!(Expression::plan(&both.expressions[0], &both.shared).is_err(), "{timing}");
        let extents = extents(&both).unwrap();
        assert_eq!(extents.clock_max, 10, "{timing}");
    }
    // Valid timing metadata counts towards the bounds whether or not the
    // row can be generated (here ticks > max_ticks).
    let row = expression(json!({"id": "since/000", "feature": "since", "robochart": "since(c)", "timing": "{\"ticks\": 40, \"max_ticks\": 20}"}));
    let extents = extents(&snapshot(vec![row], Vec::new())).unwrap();
    assert_eq!((extents.int_range, extents.clock_max), ((-2, 40), 21));
}

#[test]
fn a_malformed_json_column_is_an_error() {
    let rows = vec![expression(json!({"python_output": "{not json"}))];
    assert!(extents(&snapshot(rows, Vec::new())).is_err());
}

#[test]
fn statement_facts_give_function_bodies_and_value_domains() {
    let row = statement(json!({
        "id": "helper/pass",
        "feature": "helper",
        "robochart": "y = increment(x)",
        "inputs": "{\"x\":2,\"y\":0,\"s\":[1],\"c\":{\"v\":1}}",
        "python_output": "{\"state\":{\"x\":2,\"y\":3}}",
        "types": "{\"x\":\"int\",\"y\":\"int\",\"s\":\"Seq(int)\",\"c\":\"Command\"}",
        "datatypes": "{\"Command\":[[\"v\",\"int\"]]}",
        "functions": "function increment(x: int): int { postcondition result == x + 1 }",
        "function_definitions": "{\"increment\": {\"parameters\": [\"x\"], \"body\": \"x + 1\"}}",
        "domains": "{\"values\":{\"Seq(int)\":[[1],[1,2,3]],\"Command\":[{\"v\":1},{\"v\":2}]}}"
    }));
    let facts = csp_facts(&snapshot(Vec::new(), vec![row]));
    let [case] = facts.as_slice() else { panic!("expected one case: {facts:?}") };
    assert_eq!((case.id.as_str(), case.kind, case.package.as_str()), ("helper/pass", "statement", "statement_helper_pass"));
    assert_eq!(
        case.facts.functions,
        [FunctionFact { name: "increment".into(), parameters: vec!["x".into()], body: "x + 1".into() }]
    );
    assert!(case.facts.binder_domains.is_empty());
    let domains: Vec<_> = case
        .facts
        .value_domains
        .iter()
        .map(|domain| (domain.is_sequence(), domain.csp_type(&case.package, 3), domain.values.clone()))
        .collect();
    // In the order of the column's keys.
    assert_eq!(
        domains,
        [
            (false, "statement_helper_pass_Command".to_owned(), vec!["(1)".to_owned(), "(2)".to_owned()]),
            (true, "LSeq(core_int,3)".to_owned(), vec!["<1>".to_owned(), "<1,2,3>".to_owned()]),
        ]
    );
}

#[test]
fn expression_facts_give_shared_fixture_function_bodies() {
    let shared: SharedFixtures = serde_json::from_value(json!({
        "fixtures": {"helper": {
            "note": "a function",
            "functions": ["function twice(n: int): int { postcondition result == 2 * n }"],
            "function_definitions": {"twice": {"parameters": ["n"], "body": "2 * n"}}
        }},
        "cases": {"plain/000": {"fixtures": ["helper"], "output_type": "int"}}
    }))
    .unwrap();
    let row = expression(json!({"robochart": "twice(1)"}));
    let facts = csp_facts(&Snapshot { expressions: vec![row], statements: Vec::new(), shared });
    let [case] = facts.as_slice() else { panic!("expected one case: {facts:?}") };
    assert_eq!((case.kind, case.package.as_str()), ("expression", "expression_plain_000"));
    assert_eq!(
        case.facts.functions,
        [FunctionFact { name: "twice".into(), parameters: vec!["n".into()], body: "2 * n".into() }]
    );
}

/// The binder declarations `name : ty` of an expression whose type has a
/// domain, as the FDR preparation found them before the context rule
/// recorded them.
fn scanned_binders(robochart: &str, domains: &Value) -> Vec<(String, String, Vec<String>)> {
    let source = robochart.replace(':', " : ");
    let tokens: Vec<&str> = source
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '{' | '|' | '@' | '}'))
        .filter(|t| !t.is_empty())
        .collect();
    let mut binders = Vec::new();
    for window in tokens.windows(3) {
        if window[1] != ":" {
            continue;
        }
        let Some(choices) = domains.get(window[2]).and_then(Value::as_array) else { continue };
        let values = choices.iter().map(|v| crate::fixtures::Fixtures::default().csp_literal(window[2], v).unwrap()).collect();
        binders.push((window[0].to_owned(), window[2].to_owned(), values));
    }
    binders
}

#[test]
fn binder_domains_are_those_of_the_declared_binders() {
    let cases = [
        ("forall_False/000", "forall x: int @ x == 1", json!([]), json!(true)),
        ("exists_True/001", "exists x: int | x > 0 @ x == 1", json!([-2, -1]), json!(false)),
        ("exists1_False/004", "exists1 x: int @ x == 1", json!([-1, 0, 1, 2]), json!(true)),
        ("multiple_variables/002", "forall x: int, y: int @ x == y", json!([0]), json!(true)),
        ("the_True/003", "the x: int | x > 0 @ x", json!([1]), json!(1)),
        ("comprehension_True_True/003", "{x: int | x > 0 @ x + 1}", json!([1]), json!({"items": [2], "type": "set"})),
    ];
    for (id, robochart, domain, output) in cases {
        let domains = json!({"int": domain});
        let row = expression(json!({
            "id": id, "feature": id.split('/').next().unwrap(), "robochart": robochart,
            "inputs": json!({"domains": domains}).to_string(),
            "python_output": json!({"value": output}).to_string()
        }));
        let facts = csp_facts(&snapshot(vec![row], Vec::new()));
        let [case] = facts.as_slice() else { panic!("{id} was not generated") };
        let binders: Vec<_> =
            case.facts.binder_domains.iter().map(|b| (b.binder.clone(), b.ty.clone(), b.values.clone())).collect();
        assert_eq!(binders, scanned_binders(robochart, &domains), "{id}");
        assert!(!binders.is_empty(), "{id}");
    }
}

#[test]
fn a_row_without_binder_domains_has_none() {
    let row = expression(json!({"robochart": "forall x: int @ x == 1", "python_output": "{\"value\": false}"}));
    let facts = csp_facts(&snapshot(vec![row], Vec::new()));
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].facts, CspFacts::default());
}

#[test]
fn a_snapshot_of_one_test_case_keeps_only_its_rows() {
    let rows = vec![expression(json!({"id": "plain/001"})), expression(json!({"id": "plain/002", "robochart": "70"}))];
    let all = snapshot(rows, vec![statement(json!({}))]);
    let one = all.only("plain/001");
    assert_eq!(one.test_case_ids(), BTreeSet::from(["plain/001".to_owned()]));
    assert_eq!(extents(&one).unwrap().int_range, (-2, 2));
    assert_eq!(extents(&all).unwrap().int_range, (-2, 70));
    assert_eq!(all.test_case_ids().len(), 3);
}
