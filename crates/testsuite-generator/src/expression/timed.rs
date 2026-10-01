//! The timed clock shape: `since(clock)` or `sinceEntry(state)`, observed
//! through guards, with independent timing metadata.

use super::{ExpressionTestCase, TimingContext};
use crate::fixtures::Fixtures;
use crate::facts::CspFacts;
use crate::kind::TestCasePlan;
use crate::rejection::{Category, Rejection, from_json};
use crate::render::{Event, Machine, Member, Node, Transition};
use crate::{test_case_path, package_name, validate_identifier};
use serde_json::Value;
use std::collections::BTreeMap;

/// Observe a clock through guards, where RoboChart permits clock expressions.
/// The driver determines elapsed time from fixture metadata. Candidate outputs
/// cover the declared clock domain independently of the expected observation.
pub(crate) fn timed_expression_model(case: &ExpressionTestCase) -> Result<TestCasePlan, Rejection> {
    let timing: TimingContext = from_json(&case.timing)?;
    if timing.ticks > timing.max_ticks || timing.max_ticks > 100 {
        return Err(Category::FixtureGenerationError.reject("timing requires ticks <= max_ticks <= 100"));
    }
    if !from_json::<BTreeMap<String, Value>>(&case.inputs)?.is_empty()
        || [&case.types, &case.datatypes, &case.functions, &case.function_definitions, &case.output_type].iter().any(|s| !s.trim().is_empty()) {
        return Err(Category::FixtureGenerationError.reject("clock observations require empty ordinary fixtures"));
    }
    let expression = case.robochart.trim();
    let (state_clock, name) = if let Some(name) = expression.strip_prefix("sinceEntry(").and_then(|s| s.strip_suffix(')')) {
        (true, name.trim())
    } else if let Some(name) = expression.strip_prefix("since(").and_then(|s| s.strip_suffix(')')) {
        (false, name.trim())
    } else {
        return Err(Category::FixtureGenerationError
            .reject("timing context supports a direct since(clock) or sinceEntry(state) observation"));
    };
    validate_identifier(name).map_err(|_| {
        Category::InvalidIdentifier
            .reject("timing context requires an unqualified clock/state identifier; source expression is preserved")
    })?;
    if ["state", "initial", "final", "i0", "f0", "output", "ready", "sample"].contains(&name) {
        return Err(Category::InvalidIdentifier
            .reject("timing clock/state identifier conflicts with a keyword or observation declaration"));
    }
    let package = package_name(&case.id, "expression")?;
    let output: Value = from_json(&case.python_output)?;
    let value = output
        .get("value")
        .ok_or_else(|| Category::InvalidOracle.reject("expression output lacks value"))?;
    let expected_csp = Fixtures::default().csp_literal("real", value)?;
    let state = if state_clock { name } else { "observing" };
    let clock = if state_clock { Vec::new() } else { vec![Member::new(format!("clock {name}"), Some("shape: timed_clock".into()))] };
    let mut transitions = vec![Transition {
        name: "enter".into(),
        from: "i0".into(),
        to: state.into(),
        reset: (!state_clock).then(|| name.to_owned()),
        ..Transition::default()
    }];
    for candidate in 0..=timing.max_ticks {
        transitions.push(Transition {
            name: format!("observe{candidate}"),
            from: state.into(),
            to: "f0".into(),
            trigger: Some("sample".into()),
            condition: Some(format!("{expression} == {candidate}")),
            action: Some(format!("output ! {candidate}")),
            ..Transition::default()
        });
    }
    let machine = Machine {
        comments: vec!["Timed clock observation; setup is independent of expected output.".into()],
        package: package.clone(),
        import: None,
        events: vec![Event::output("ready", None), Event::input("sample", None), Event::output("output", Some("int"))],
        members: clock,
        nodes: vec![
            Node::Initial("i0".into()),
            Node::Final("f0".into()),
            Node::State { name: state.into(), entry: Some("ready".into()) },
        ],
        transitions,
        declarations: Vec::new(),
    };
    let ticks = "tock -> ".repeat(timing.ticks as usize);
    let check = format!("include \"{package}.csp\"\nready_event = {package}::mod0::ready.out\nsample_event = {package}::mod0::sample.in\ndriver = ready_event -> {ticks}sample_event -> {package}::mod0::output.out?value -> STOP\nmodel = ({package}::mod0::D__(0) [|{{|{package}::mod0::ready, {package}::mod0::sample, {package}::mod0::output, tock|}}|] driver) \\ {{|{package}::mod0::ready, {package}::mod0::sample, tock|}}\nexpected = {package}::mod0::output.out!{expected_csp} -> STOP\nassert expected [FD= model\n");
    Ok(TestCasePlan {
        timed: true,
        path: test_case_path("expressions", &case.feature, &case.id)?,
        machine,
        expected_csp,
        fixture_arguments: Vec::new(),
        expected_status: "pass".into(),
        expected_events: Vec::new(),
        driver: None,
        custom_check: Some(check),
        facts: CspFacts::default(),
    })
}
