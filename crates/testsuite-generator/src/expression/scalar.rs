//! The scalar shape: an expression whose value is sent on `output`.
//!
//! Typed and untyped rows have one shape. Their context is built by
//! [`SCALAR_CONTEXT_RULES`](super::context::SCALAR_CONTEXT_RULES), and every
//! value is rendered by the literal engine (`fixtures.rs`).

use super::ExpressionTestCase;
use super::context::{Init, SCALAR_CONTEXT_RULES, ScalarContext};
use crate::facts::{CspFacts, FunctionFact};
use crate::kind::TestCasePlan;
use crate::rejection::Rejection;
use crate::render::{Event, Machine, Member, Node, Transition};
use crate::test_case_path;

/// The comment that names the rule that declared something.
fn origin(rule: &str) -> Option<String> {
    Some(format!("context rule: {rule}"))
}

/// Plan a scalar row: build and check its context, then render its values.
pub(crate) fn scalar_plan(case: &ExpressionTestCase) -> Result<TestCasePlan, Rejection> {
    let context = ScalarContext::build(case, SCALAR_CONTEXT_RULES)?;
    let fixtures = &context.fixtures;
    let (output_type, output_origin) = context.output.clone().expect("a checked context declares the output type");
    let expected_csp = fixtures.csp_value(&output_type, &context.output_value)?;
    let mut members = Vec::new();
    for (name, variable) in &context.variables {
        members.push(Member::new(format!("var {name} : {}", variable.ty), origin(variable.origin)));
    }
    // Constants are declared in name order, the order of D__'s parameters.
    let mut fixture_arguments = Vec::new();
    for (name, constant) in &context.constants {
        let declaration = match constant.init {
            Init::Literal => format!("const {name} : {} = {}", constant.ty, fixtures.robo_value(&constant.ty, &constant.value)?),
            Init::Uninitialised => format!("const {name} : {}", constant.ty),
        };
        members.push(Member::new(declaration, origin(constant.origin)));
        fixture_arguments.push(fixtures.csp_value(&constant.ty, &constant.value)?);
    }
    let mut declarations: Vec<Member> = fixtures
        .declaration_list()
        .into_iter()
        .map(|(name, text)| Member::new(text, context.origin(&name).and_then(origin)))
        .collect();
    for (text, rule) in &context.functions {
        declarations.push(Member::new(text.clone(), origin(rule)));
    }
    let output = output_type.to_string();
    let machine = Machine {
        package: context.package.clone(),
        import: Some("sequence_toolkit"),
        events: vec![Event { asynchronous: true, origin: origin(output_origin), ..Event::output("output", Some(&output)) }],
        members,
        nodes: vec![Node::Initial("i0".into()), Node::Final("f0".into())],
        transitions: vec![Transition {
            name: "t0".into(),
            from: "i0".into(),
            to: "f0".into(),
            action: Some(format!("output ! ({})", case.robochart)),
            ..Transition::default()
        }],
        declarations,
        ..Machine::default()
    };
    Ok(TestCasePlan {
        timed: false,
        path: test_case_path("expressions", &case.feature, &case.id)?,
        machine,
        expected_csp,
        fixture_arguments,
        expected_status: "pass".into(),
        expected_events: Vec::new(),
        driver: None,
        custom_check: None,
        facts: CspFacts {
            functions: FunctionFact::all(context.function_definitions.clone()),
            value_domains: Vec::new(),
            binder_domains: context.binder_domains()?,
        },
    })
}
