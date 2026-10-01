//! The lambda graph shape: a function-valued expression observed through
//! its graph at the recorded arguments.

use super::ExpressionTestCase;
use crate::facts::CspFacts;
use crate::kind::TestCasePlan;
use crate::rejection::{Category, Rejection, from_json};
use crate::render::{Event, Machine, Member, Node, Transition};
use crate::{test_case_path, package_name};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Observe a function-valued expression through its graph at the supplied
/// arguments. `rimage(f, {a})` is its graph slice: a singleton value set when
/// defined, empty otherwise. This uses RoboTool's relation toolkit without
/// applying a named function constant (oracle issue B1). Existing scalar
/// expectations are already application observations at a single argument;
/// their values are copied, never recomputed from the lambda body.
pub(crate) fn lambda_model(case: &ExpressionTestCase) -> Result<TestCasePlan, Rejection> {
    let package = package_name(&case.id, "expression")?;
    let inputs: Value = from_json(&case.inputs)?;
    let arguments: Vec<i64> = inputs
        .get("apply")
        .and_then(Value::as_array)
        .ok_or_else(|| Category::FixtureGenerationError.reject("lambda rows need an `apply` list of arguments"))?
        .iter()
        .map(|a| a.as_i64().ok_or_else(|| Category::FixtureGenerationError.reject("lambda arguments must be integers")))
        .collect::<Result<_, _>>()?;
    if arguments.is_empty() {
        return Err(Category::FixtureGenerationError.reject("lambda rows need at least one argument"));
    }
    let output: Value = from_json(&case.python_output)?;
    if arguments.iter().copied().collect::<BTreeSet<_>>().len() != arguments.len() {
        return Err(Category::FixtureGenerationError.reject("lambda arguments must be unique"));
    }
    let mut points = BTreeMap::new();
    if let Some(value) = output.get("value") {
        if arguments.len() != 1 {
            return Err(Category::FixtureGenerationError.reject("a scalar lambda expectation needs exactly one argument"));
        }
        let value = value
            .as_i64()
            .ok_or_else(|| Category::FixtureGenerationError.reject("lambda observation must be an integer"))?;
        points.insert(arguments[0], Some(value));
    } else {
        let graph = output["graph"]
            .as_array()
            .ok_or_else(|| Category::FixtureGenerationError.reject("graph must be a list"))?;
        for point in graph {
            let argument = point["arg"]
                .as_i64()
                .ok_or_else(|| Category::FixtureGenerationError.reject("graph point lacks an integer arg"))?;
            let defined = point["defined"]
                .as_bool()
                .ok_or_else(|| Category::FixtureGenerationError.reject("graph point lacks defined"))?;
            let value = if defined {
                Some(point["value"].as_i64().ok_or_else(|| {
                    Category::FixtureGenerationError.reject("defined graph point lacks an integer value")
                })?)
            } else {
                None
            };
            if points.insert(argument, value).is_some() {
                return Err(Category::FixtureGenerationError.reject("duplicate lambda graph argument"));
            }
        }
    }
    if points.keys().copied().collect::<BTreeSet<_>>() != arguments.iter().copied().collect() {
        return Err(Category::FixtureGenerationError.reject("the graph must give exactly one point per argument"));
    }
    let parameter = case
        .robochart
        .trim()
        .strip_prefix("lambda")
        .and_then(|rest| rest.split_once(':'))
        .map(|(_, rest)| rest.split(['|', '@']).next().unwrap_or("").trim().to_owned())
        .ok_or_else(|| Category::InvalidFixtureValue.reject("expected a lambda `lambda x: T ...`"))?;
    if parameter != "int" {
        return Err(Category::FixtureGenerationError.reject("unsupported lambda parameter type"));
    }
    let machine = Machine {
        package: package.clone(),
        import: Some("relation_toolkit"),
        events: vec![Event { asynchronous: true, ..Event::output("output", Some("Set(int)")) }],
        members: vec![Member::new("const arg : int", Some("shape: lambda_graph".into()))],
        nodes: vec![Node::Initial("i0".into()), Node::Final("f0".into())],
        transitions: vec![Transition {
            name: "t0".into(),
            from: "i0".into(),
            to: "f0".into(),
            action: Some(format!("output ! rimage(({}), {{arg}})", case.robochart.trim())),
            ..Transition::default()
        }],
        ..Machine::default()
    };
    let set = format!(
        "{{ {}}}",
        arguments.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
    );
    let branches = points
        .iter()
        .map(|(argument, value)| {
            let observed = match value {
                Some(v) => format!("{{ {v}}}"),
                None => "{}".to_owned(),
            };
            format!(
                "lambda_argument.{argument} -> {package}::mod0::output.out.{observed} -> (SKIP |~| STOP)"
            )
        })
        .collect::<Vec<_>>()
        .join("\n  [] ");
    let check = format!(
        "include \"{package}.csp\"\nchannel lambda_argument : {set}\nexpected =\n  {branches}\nmodel = [] a : {set} @ lambda_argument.a -> {package}::mod0::D__(0, a)\nassert expected [FD= model\n"
    );
    Ok(TestCasePlan {
        timed: false,
        path: test_case_path("expressions", &case.feature, &case.id)?,
        machine,
        expected_csp: String::new(),
        fixture_arguments: Vec::new(),
        expected_status: "pass".into(),
        expected_events: Vec::new(),
        driver: None,
        custom_check: Some(check),
        facts: CspFacts::default(),
    })
}
