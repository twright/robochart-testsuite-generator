//! Planning a statement row, as a sequence of stages. Each stage takes what
//! it depends on as parameters, so the order of [`plan`] is the order of the
//! dependencies, and every stage can reject the row.

use super::StatementTestCase;
use crate::fixtures::Fixtures;
use crate::facts::{CspFacts, FunctionFact, ValueDomain};
use crate::kind::TestCasePlan;
use crate::rejection::{Category, Rejection, from_json};
use crate::render::{Event, Machine, Member, Node, Transition};
use crate::{package_name, test_case_path, validate_function_fixtures, validate_identifier};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, Rejection>;

/// Plan a statement row.
pub(crate) fn plan(row: &StatementTestCase) -> Result<TestCasePlan> {
    let package = package_name(&row.id, "statement")?;
    let expected_status = expected_status(row)?;
    let columns = columns(row)?;
    let fixtures = datatypes(row)?;
    let (domains, value_domains) = validate_value_domains(row, &fixtures, &package)?;
    let functions = validate_function_fixtures(&row.functions, &row.function_definitions)?;
    let variables = variables(row, &columns, &fixtures)?;
    let observation = observation(&columns, &fixtures)?;
    let events = events(row, &fixtures)?;
    let expected_trace = expected_trace(&columns.output, &events, &fixtures, &package)?;
    let driver = driver(row, &events, &variables, &columns.types, &fixtures, &package)?;
    let body = body(row, &columns.types)?;
    let machine = machine(row, &package, &domains, &fixtures, &variables, &observation, &events, &driver, &body)?;
    Ok(TestCasePlan {
        timed: false,
        path: test_case_path("statements", &row.feature, &row.id)?,
        machine,
        expected_csp: observation.expected_csp,
        fixture_arguments: variables.fixture_arguments,
        expected_status,
        expected_events: expected_trace,
        driver: driver.process,
        custom_check: None,
        facts: CspFacts { functions: FunctionFact::all(functions), value_domains, binder_domains: Vec::new() },
    })
}

/// The JSON columns that every statement row has.
struct Columns {
    inputs: BTreeMap<String, Value>,
    types: BTreeMap<String, String>,
    observe: Vec<String>,
    output: Value,
}

/// The state machine's variables and constants, and the constants' CSP
/// values (the parameters of `D__`).
struct Variables {
    members: Vec<Member>,
    constants: BTreeSet<String>,
    fixture_arguments: Vec<String>,
}

/// The observed variables: the `Obs` record's fields, the record value sent
/// on `output`, and the expected value as a CSP tuple.
struct Observation {
    fields: Vec<String>,
    values: Vec<String>,
    expected_csp: String,
}

/// Events, by name, with their optional types.
struct Events {
    outputs: BTreeMap<String, Option<String>>,
    inputs: BTreeMap<String, Option<String>>,
}

/// The states and transitions that receive the driver's inputs, the state
/// that the statement starts from, and the CSP driver process.
struct Driver {
    nodes: Vec<Node>,
    transitions: Vec<Transition>,
    from: String,
    process: Option<(String, Vec<String>)>,
}

#[derive(Deserialize)]
struct ScriptedInput {
    event: String,
    bind: Option<String>,
    value: Option<Value>,
}

#[derive(Deserialize)]
struct BoundedIteration {
    index: String,
    count: usize,
}

fn expected_status(row: &StatementTestCase) -> Result<String> {
    if !matches!(row.expected_status.as_str(), "pass" | "mismatch") {
        return Err(Category::FixtureGenerationError.reject("expected_status must be pass or mismatch"));
    }
    Ok(row.expected_status.clone())
}

fn columns(row: &StatementTestCase) -> Result<Columns> {
    let inputs = from_json(&row.inputs)?;
    let types = from_json(&row.types)?;
    let observe = from_json(&row.observe)?;
    let output: Value = from_json(&row.python_output)?;
    if !output.get("state").is_some_and(Value::is_object) {
        return Err(Category::InvalidOracle.reject("statement output lacks state"));
    }
    Ok(Columns { inputs, types, observe, output })
}

/// The row's records and enumerations. `Obs` is reserved for the observation.
fn datatypes(row: &StatementTestCase) -> Result<Fixtures> {
    let fixtures = Fixtures::new(&row.datatypes)?;
    if fixtures.names().any(|name| name == "Obs") {
        return Err(Category::FixtureGenerationError.reject("datatype Obs is reserved for state observations"));
    }
    Ok(fixtures)
}

/// Explicit finite value domains for record and sequence types, which the
/// FDR preparation uses; each value must be one of its type. Gives the
/// column, and each domain with the CSP literals of its values.
fn validate_value_domains(row: &StatementTestCase, fixtures: &Fixtures, package: &str) -> Result<(Value, Vec<ValueDomain>)> {
    let domains: Value = if row.domains.trim().is_empty() {
        serde_json::json!({})
    } else {
        from_json(&row.domains)?
    };
    let mut value_domains = Vec::new();
    if let Some(values) = domains.get("values") {
        for (ty, choices) in values
            .as_object()
            .ok_or_else(|| Category::FixtureGenerationError.reject("fixture value domains must be a JSON object"))?
        {
            let choices = choices
                .as_array()
                .filter(|items| !items.is_empty())
                .ok_or_else(|| Category::FixtureGenerationError.reject("fixture value domain must be a non-empty array"))?;
            let csp_type = fixtures.csp_type(ty, package, 2)?;
            if !csp_type.starts_with("LSeq(") && !csp_type.starts_with(&format!("{package}_")) {
                return Err(Category::FixtureGenerationError.reject("explicit fixture values require a record or sequence type"));
            }
            let values = choices.iter().map(|value| fixtures.csp_literal(ty, value)).collect::<Result<_>>()?;
            value_domains.push(ValueDomain { ty: fixtures.resolve(ty)?, values });
        }
    }
    Ok((domains, value_domains))
}

/// Every typed input is a variable, or a constant if the `constants` column
/// names it; constants are parameters of `D__`.
fn variables(row: &StatementTestCase, columns: &Columns, fixtures: &Fixtures) -> Result<Variables> {
    let Columns { inputs, types, observe, .. } = columns;
    if observe.is_empty() {
        return Err(Category::FixtureGenerationError.reject("statement must observe a variable"));
    }
    if inputs.len() != types.len() || types.keys().any(|name| !inputs.contains_key(name)) {
        return Err(Category::FixtureGenerationError.reject("every typed variable needs an initial input value"));
    }
    let constants: BTreeSet<String> = if row.constants.trim().is_empty() {
        BTreeSet::new()
    } else {
        from_json(&row.constants)?
    };
    if constants.iter().any(|name| !inputs.contains_key(name)) {
        return Err(Category::FixtureGenerationError.reject("constant fixture must name an input variable"));
    }
    let mut members = Vec::new();
    let mut fixture_arguments = Vec::new();
    for (name, value) in inputs {
        validate_identifier(name)?;
        let declared = types
            .get(name)
            .ok_or_else(|| Category::FixtureGenerationError.reject(format!("missing type for {name}")))?;
        let qualifier = if constants.contains(name) { "const" } else { "var" };
        if qualifier == "const" {
            fixture_arguments.push(fixtures.csp_literal(declared, value)?);
        }
        members.push(Member::new(
            format!("{qualifier} {name} : {declared} = {}", fixtures.robo_literal(declared, value)?),
            None,
        ));
    }
    Ok(Variables { members, constants, fixture_arguments })
}

/// The observed variables, in order, and their expected final values.
fn observation(columns: &Columns, fixtures: &Fixtures) -> Result<Observation> {
    let state = columns.output["state"].as_object().expect("columns checked the state");
    let mut fields = Vec::new();
    let mut values = Vec::new();
    let mut expected = Vec::new();
    let mut seen = BTreeSet::new();
    for name in &columns.observe {
        validate_identifier(name)?;
        if !seen.insert(name.clone()) {
            return Err(Category::FixtureGenerationError.reject(format!("duplicate observed variable {name}")));
        }
        let declared = columns
            .types
            .get(name)
            .ok_or_else(|| Category::FixtureGenerationError.reject(format!("missing type for {name}")))?;
        let value = state
            .get(name)
            .ok_or_else(|| Category::InvalidFixtureValue.reject(format!("missing expected state for {name}")))?;
        fields.push(format!("{name}: {declared}"));
        values.push(format!("{name} = {name}"));
        expected.push(fixtures.csp_literal(declared, value)?);
    }
    if state.len() != seen.len() {
        return Err(Category::InvalidFixtureValue.reject("expected state must contain exactly the observed variables"));
    }
    Ok(Observation { fields, values, expected_csp: format!("({})", expected.join(",")) })
}

/// Output and input events; their names are distinct, and not `output`.
fn events(row: &StatementTestCase, fixtures: &Fixtures) -> Result<Events> {
    let parse = |column: &str| -> Result<BTreeMap<String, Option<String>>> {
        if column.trim().is_empty() { Ok(BTreeMap::new()) } else { from_json(column) }
    };
    let events = Events { outputs: parse(&row.events)?, inputs: parse(&row.input_events)? };
    for (name, ty) in events.outputs.iter().chain(&events.inputs) {
        validate_identifier(name)?;
        if name == "output" || (events.outputs.contains_key(name) && events.inputs.contains_key(name)) {
            return Err(Category::FixtureGenerationError.reject(format!("event name conflicts with another event: {name}")));
        }
        if let Some(ty) = ty {
            fixtures.validate_type(ty)?;
        }
    }
    Ok(events)
}

/// The output events Python recorded, as CSP events before the observation.
fn expected_trace(output: &Value, events: &Events, fixtures: &Fixtures, package: &str) -> Result<Vec<String>> {
    let Some(trace) = output.get("events") else { return Ok(Vec::new()) };
    let mut expected = Vec::new();
    for item in trace.as_array().ok_or_else(|| Category::InvalidFixtureValue.reject("expected events must be an array"))? {
        let name = item
            .get("event")
            .and_then(Value::as_str)
            .ok_or_else(|| Category::InvalidFixtureValue.reject("expected event lacks name"))?;
        let ty = events
            .outputs
            .get(name)
            .ok_or_else(|| Category::InvalidFixtureValue.reject(format!("undeclared expected event {name}")))?;
        let suffix = match (ty, item.get("value")) {
            (Some(ty), Some(value)) => format!("!{}", fixtures.csp_literal(ty, value)?),
            (None, None) => String::new(),
            _ => return Err(Category::InvalidFixtureValue.reject(format!("payload mismatch for expected event {name}"))),
        };
        expected.push(format!("{package}::mod0::{name}.out{suffix}"));
    }
    Ok(expected)
}

/// The driver script: the state machine receives each scripted input in
/// turn (binding a variable to its payload), and a CSP process offers them.
fn driver(
    row: &StatementTestCase,
    events: &Events,
    variables: &Variables,
    types: &BTreeMap<String, String>,
    fixtures: &Fixtures,
    package: &str,
) -> Result<Driver> {
    let script: Vec<ScriptedInput> = if row.driver.trim().is_empty() {
        Vec::new()
    } else {
        from_json(&row.driver)?
    };
    if !events.inputs.is_empty() && script.is_empty() {
        return Err(Category::FixtureGenerationError.reject("input events require a finite driver script"));
    }
    let mut driver = Driver { nodes: Vec::new(), transitions: Vec::new(), from: "i0".into(), process: None };
    if script.is_empty() {
        return Ok(driver);
    }
    driver.nodes.push(Node::State { name: "ready".into(), entry: None });
    driver.transitions.push(Transition { name: "start".into(), from: "i0".into(), to: "ready".into(), ..Transition::default() });
    driver.from = "ready".into();
    let mut prefix = Vec::new();
    for (index, step) in script.iter().enumerate() {
        let ty = events
            .inputs
            .get(&step.event)
            .ok_or_else(|| Category::FixtureGenerationError.reject(format!("undeclared driver event {}", step.event)))?;
        let (trigger, suffix) = match (ty, &step.bind, &step.value) {
            (Some(ty), Some(bind), Some(value)) => {
                if variables.constants.contains(bind) {
                    return Err(Category::FixtureGenerationError.reject("driver cannot bind a constant fixture"));
                }
                if types.get(bind).map(|s| s.replace(' ', "")) != Some(ty.replace(' ', "")) {
                    return Err(Category::FixtureGenerationError.reject(format!("driver binding type mismatch for {bind}")));
                }
                (format!("{} ? {bind}", step.event), format!("!{}", fixtures.csp_literal(ty, value)?))
            }
            (None, None, None) => (step.event.clone(), String::new()),
            _ => return Err(Category::FixtureGenerationError.reject("driver payload and binding must match event type")),
        };
        let to = format!("received{index}");
        driver.nodes.push(Node::State { name: to.clone(), entry: None });
        driver.transitions.push(Transition {
            name: format!("receive{index}"),
            from: driver.from.clone(),
            to: to.clone(),
            trigger: Some(trigger),
            ..Transition::default()
        });
        prefix.push(format!("{package}::mod0::{}.in{suffix}", step.event));
        driver.from = to;
    }
    prefix.push("STOP".into());
    let channels = events.inputs.keys().map(|name| format!("{package}::mod0::{name}")).collect();
    driver.process = Some((prefix.join(" -> "), channels));
    Ok(driver)
}

/// The statement, with a bounded iteration unrolled.
fn body(row: &StatementTestCase, types: &BTreeMap<String, String>) -> Result<String> {
    let body = row.robochart.trim().trim_end_matches(';');
    if row.iteration.trim().is_empty() {
        return Ok(body.to_owned());
    }
    let iteration: BoundedIteration = from_json(&row.iteration)?;
    validate_identifier(&iteration.index)?;
    if !types
        .get(&iteration.index)
        .is_some_and(|ty| matches!(ty.trim(), "int" | "nat"))
    {
        return Err(Category::FixtureGenerationError.reject("bounded iteration requires an integer index variable"));
    }
    if iteration.count > 64 {
        return Err(Category::UnsupportedStatement.reject("unsupported bounded iteration: maximum count is 64"));
    }
    Ok((1..=iteration.count)
        .map(|index| format!("{} = {index}; {}", iteration.index, body.replace("{{index}}", &index.to_string())))
        .collect::<Vec<_>>()
        .join("; "))
}

/// The model: the statement runs on the transition from the driver's last
/// state, and then the observed variables are sent on `output`.
#[allow(clippy::too_many_arguments)]
fn machine(
    row: &StatementTestCase,
    package: &str,
    domains: &Value,
    fixtures: &Fixtures,
    variables: &Variables,
    observation: &Observation,
    events: &Events,
    driver: &Driver,
    body: &str,
) -> Result<Machine> {
    let mut comments = vec![format!("Statement case: {}", row.id)];
    comments.extend(row.notes.lines().map(str::to_owned));
    if !row.iteration.trim().is_empty() {
        comments.push(format!("Fixed iteration fixture: {}", row.iteration));
    }
    if domains.get("values").is_some() {
        comments.push("Finite CSP fixture domains:".into());
        let pretty = serde_json::to_string_pretty(domains)
            .map_err(|error| Category::FixtureGenerationError.reject(error.to_string()))?;
        comments.extend(pretty.lines().map(str::to_owned));
    }
    let mut machine_events = vec![Event::output("output", Some("Obs"))];
    machine_events.extend(events.outputs.iter().map(|(name, ty)| Event::output(name, ty.as_deref())));
    machine_events.extend(events.inputs.iter().map(|(name, ty)| Event::input(name, ty.as_deref())));
    let mut nodes = vec![Node::Initial("i0".into()), Node::State { name: "s0".into(), entry: None }];
    nodes.extend(driver.nodes.iter().cloned());
    let action = if body.is_empty() { String::new() } else { format!("{body}; ") };
    let mut transitions = driver.transitions.clone();
    transitions.push(Transition {
        name: "t0".into(),
        from: driver.from.clone(),
        to: "s0".into(),
        action: Some(format!("{action}output ! Obs(| {} |)", observation.values.join(", "))),
        ..Transition::default()
    });
    let mut declarations = vec![Member::new(format!("record Obs {{ {} }}", observation.fields.join(" ")), None)];
    declarations.extend(fixtures.declaration_list().into_iter().map(|(_, text)| Member::new(text, None)));
    if !row.functions.trim().is_empty() {
        declarations.push(Member::new(row.functions.clone(), None));
    }
    Ok(Machine {
        comments,
        package: package.to_owned(),
        import: Some("sequence_toolkit"),
        events: machine_events,
        members: variables.members.clone(),
        nodes,
        transitions,
        declarations,
    })
}
