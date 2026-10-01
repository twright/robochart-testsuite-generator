//! Property tests for the case generator.
//!
//! None of these properties uses a reference implementation. Each one relates
//! the generator to itself: round trips, metamorphic relations (a related
//! input must give a related output), consistency between the generator's own
//! parts, injectivity, and acceptance or rejection of rows that are well
//! formed, or faulty, by construction.

use super::fixtures::Fixtures;
use super::test_rows::*;
use super::*;
use crate::expression::expression_model;
use crate::kind::GeneratedTestCase;
use crate::statement::statement_model;
use proptest::prelude::*;
use proptest::sample::Index;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};


const PACKAGE: &str = "statement_prop_case";

/// The row as the generator's input record.
fn row_of(spec: &Spec) -> StatementTestCase {
    let columns: Map<String, Value> = spec.columns("prop/case").into_iter().map(|(k, v)| (k.to_owned(), Value::String(v))).collect();
    serde_json::from_value(Value::Object(columns)).expect("columns match StatementTestCase")
}

fn generate(spec: &Spec) -> std::result::Result<GeneratedTestCase, TestCaseError> {
    statement_model(row_of(&spec)).map_err(|e| TestCaseError::fail(format!("rejected: {e}")))
}

static PROJECTS: AtomicUsize = AtomicUsize::new(0);

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Replace whole-identifier occurrences of `from` with `to`.
fn replace_word(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut previous: Option<char> = None;
    while let Some(i) = rest.find(from) {
        let before = rest[..i].chars().last().or(previous);
        let after = rest[i + from.len()..].chars().next();
        out.push_str(&rest[..i]);
        if before.is_some_and(is_ident) || after.is_some_and(is_ident) {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        previous = from.chars().last();
        rest = &rest[i + from.len()..];
    }
    out.push_str(rest);
    out
}

fn sorted_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    lines
}

fn fixtures_of(spec: &Spec) -> Fixtures {
    Fixtures::new(&spec.schema.datatypes()).unwrap()
}

/// Rename record types and fields in a schema, and in a value of type `ty`.
fn rename_value(schema: &Schema, ty: &Ty, value: &Value, record: &dyn Fn(&str) -> String, field: &dyn Fn(&str) -> String) -> Value {
    match (ty, value) {
        (Ty::Seq(item), Value::Array(items)) => {
            Value::Array(items.iter().map(|v| rename_value(schema, item, v, record, field)).collect())
        }
        (Ty::Rec(name), Value::Object(object)) => Value::Object(
            schema
                .fields(name)
                .iter()
                .map(|(f, t)| (field(f), rename_value(schema, t, &object[f], record, field)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn rename_ty(ty: &Ty, record: &dyn Fn(&str) -> String) -> Ty {
    match ty {
        Ty::Seq(item) => Ty::Seq(Box::new(rename_ty(item, record))),
        Ty::Rec(name) => Ty::Rec(record(name)),
        other => other.clone(),
    }
}

fn rename_schema(schema: &Schema, record: &dyn Fn(&str) -> String, field: &dyn Fn(&str) -> String) -> Schema {
    Schema {
        records: schema
            .records
            .iter()
            .map(|(name, fields)| {
                (record(name), fields.iter().map(|(f, t)| (field(f), rename_ty(t, record))).collect())
            })
            .collect(),
    }
}

// Fresh names chosen so that renaming reverses the alphabetical order of
// fields, which exposes any layout that depends on sorting by name.
fn fresh_record(name: &str) -> String {
    format!("Q{}", name.to_lowercase())
}

fn fresh_field(name: &str) -> String {
    let position = FIELD_NAMES.iter().position(|f| *f == name).unwrap();
    format!("f{}", ['z', 'y', 'x', 'w', 'v'][position])
}

// ---------------------------------------------------------------------------
// Fixture values
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Different values of one type get different literals, in both syntaxes.
    #[test]
    fn literals_are_injective(
        (schema, ty, a, b) in arb_typed_value().prop_flat_map(|(schema, ty, a)| {
            let b = arb_value(&schema, &ty);
            (Just(schema), Just(ty), Just(a), b)
        })
    ) {
        if a == b {
            return Ok(());
        }
        let fixtures = Fixtures::new(&schema.datatypes()).unwrap();
        let t = ty.text();
        prop_assert_ne!(fixtures.csp_literal(&t, &a).unwrap(), fixtures.csp_literal(&t, &b).unwrap());
        prop_assert_ne!(fixtures.robo_literal(&t, &a).unwrap(), fixtures.robo_literal(&t, &b).unwrap());
    }

    /// Renaming record types and fields (to names that sort in the opposite
    /// order) leaves the CSP literal unchanged and renames the RoboChart
    /// literal. So the CSP layout follows declaration order, not names.
    #[test]
    fn literals_follow_declaration_order_not_names((schema, ty, value) in arb_typed_value()) {
        let renamed_schema = rename_schema(&schema, &fresh_record, &fresh_field);
        let renamed_ty = rename_ty(&ty, &fresh_record);
        let renamed_value = rename_value(&schema, &ty, &value, &fresh_record, &fresh_field);
        let original = Fixtures::new(&schema.datatypes()).unwrap();
        let renamed = Fixtures::new(&renamed_schema.datatypes()).unwrap();
        prop_assert_eq!(
            renamed.csp_literal(&renamed_ty.text(), &renamed_value).unwrap(),
            original.csp_literal(&ty.text(), &value).unwrap()
        );
        let mut expected = original.robo_literal(&ty.text(), &value).unwrap();
        for (name, fields) in &schema.records {
            expected = replace_word(&expected, name, &fresh_record(name));
            for (field, _) in fields {
                expected = replace_word(&expected, field, &fresh_field(field));
            }
        }
        prop_assert_eq!(renamed.robo_literal(&renamed_ty.text(), &renamed_value).unwrap(), expected);
    }

    /// With record names, field labels and spacing erased, the RoboChart
    /// literal is the CSP literal: the two encoders agree on structure.
    #[test]
    fn robochart_and_csp_literals_agree((schema, ty, value) in arb_typed_value()) {
        let fixtures = Fixtures::new(&schema.datatypes()).unwrap();
        let mut erased: String = fixtures.robo_literal(&ty.text(), &value).unwrap();
        for (name, fields) in &schema.records {
            erased = erased.replace(&format!("{name}(|"), "(");
            for (field, _) in fields {
                erased = replace_word(&erased, field, "");
            }
        }
        let unspaced = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let erased = unspaced(&erased.replace("|)", ")").replace('=', ""));
        prop_assert_eq!(erased, unspaced(&fixtures.csp_literal(&ty.text(), &value).unwrap()));
    }

    /// No literal contains `<-`, which RoboChart and CSPm both read as one
    /// token, so a sequence starting with a negative number stays parseable.
    #[test]
    fn literals_never_contain_the_arrow_token((schema, ty, value) in arb_typed_value()) {
        let fixtures = Fixtures::new(&schema.datatypes()).unwrap();
        let csp = fixtures.csp_literal(&ty.text(), &value).unwrap();
        let robo = fixtures.robo_literal(&ty.text(), &value).unwrap();
        prop_assert!(!csp.contains("<-"), "{}", csp);
        prop_assert!(!robo.contains("<-"), "{}", robo);
    }

    /// Generated values are always accepted, and after a small structural
    /// fault the two encoders still accept or reject together.
    #[test]
    fn encoders_accept_the_same_values(
        (schema, ty, value, perturbed) in arb_typed_value()
            .prop_flat_map(|(s, t, v)| { let p = arb_perturbed(v.clone()); (Just(s), Just(t), Just(v), p) })
    ) {
        let fixtures = Fixtures::new(&schema.datatypes()).unwrap();
        prop_assert!(fixtures.csp_literal(&ty.text(), &value).is_ok());
        prop_assert!(fixtures.robo_literal(&ty.text(), &value).is_ok());
        prop_assert_eq!(
            fixtures.csp_literal(&ty.text(), &perturbed).is_ok(),
            fixtures.robo_literal(&ty.text(), &perturbed).is_ok(),
            "{}", perturbed
        );
    }

    /// Schemas built acyclic are accepted; adding one back edge that closes a
    /// cycle makes the same schema rejected.
    #[test]
    fn cycles_are_rejected(schema in arb_schema().prop_filter("has records", |s| !s.records.is_empty()), pick in any::<Index>()) {
        prop_assert!(Fixtures::new(&schema.datatypes()).is_ok());
        // A field of a record whose type mentions the record itself closes a
        // cycle, even under a sequence.
        let mut cyclic = schema.clone();
        let target = pick.get(&cyclic.records).0.clone();
        cyclic.records.iter_mut().find(|(n, _)| *n == target).unwrap().1.push(("loop".into(), Ty::Seq(Box::new(Ty::Rec(target.clone())))));
        prop_assert!(Fixtures::new(&cyclic.datatypes()).is_err());
    }
}

// ---------------------------------------------------------------------------
// Statement rows
// ---------------------------------------------------------------------------

fn with_status(mut row: StatementTestCase, status: &str) -> StatementTestCase {
    row.expected_status = status.into();
    row
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Every row that is well formed by construction is accepted.
    #[test]
    fn well_formed_rows_are_accepted(spec in arb_spec()) {
        generate(&spec)?;
    }

    /// Every value in the model and the check is the fixture encoder's
    /// encoding of the corresponding row value: initial values, constants,
    /// the expected final state (in observation order), event payloads and
    /// driver payloads.
    #[test]
    fn every_value_uses_the_fixture_encoding(spec in arb_spec()) {
        let case = generate(&spec)?;
        let fixtures = fixtures_of(&spec);
        let declarations = fixtures.declarations();
        prop_assert!(case.model.contains(&declarations), "record declarations missing: {:?}", declarations);
        for (name, ty, initial) in &spec.vars {
            let qualifier = if spec.constants.contains(name) { "const" } else { "var" };
            let line = format!("      {qualifier} {name} : {} = {}\n", ty.text(), fixtures.robo_literal(&ty.text(), initial).unwrap());
            prop_assert_eq!(case.model.matches(&line).count(), 1, "{:?}", line);
        }
        let constants: Vec<String> = spec.constants.iter().map(|n| {
            let (_, ty, v) = spec.vars.iter().find(|(m, _, _)| m == n).unwrap();
            fixtures.csp_literal(&ty.text(), v).unwrap()
        }).collect();
        prop_assert_eq!(&case.fixture_arguments, &constants);
        let state: Vec<String> = spec.observe.iter().map(|n| fixtures.csp_literal(&spec.ty(n).text(), &spec.state[n]).unwrap()).collect();
        prop_assert_eq!(&case.expected_csp, &format!("({})", state.join(",")));
        let events: Vec<String> = spec.trace.iter().map(|(name, value)| {
            let payload = value.as_ref().map(|v| {
                let ty = spec.outputs.iter().find(|(n, _)| n == name).unwrap().1.as_ref().unwrap();
                format!("!{}", fixtures.csp_literal(&ty.text(), v).unwrap())
            }).unwrap_or_default();
            format!("{PACKAGE}::mod0::{name}.out{payload}")
        }).collect();
        prop_assert_eq!(&case.expected_events, &events);
        if let Some((process, _)) = &case.driver {
            let steps: Vec<String> = spec.driver.iter().map(|(event, bind)| {
                let payload = bind.as_ref().map(|(var, v)| format!("!{}", fixtures.csp_literal(&spec.ty(var).text(), v).unwrap())).unwrap_or_default();
                format!("{PACKAGE}::mod0::{event}.in{payload}")
            }).chain(std::iter::once("STOP".to_owned())).collect();
            prop_assert_eq!(process, &steps.join(" -> "));
        }
    }

    /// Replacing the row's source changes the model only where the source is
    /// embedded, and changes nothing in the check.
    #[test]
    fn the_source_is_embedded_and_nothing_else_depends_on_it(spec in arb_spec()) {
        let base = generate(&spec)?;
        let mut row = row_of(&spec);
        row.robochart = row.robochart.replace("work", "other");
        let changed = statement_model(row).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(changed.model, replace_word(&base.model, "work", "other"));
        prop_assert_eq!(changed.expected_csp, base.expected_csp);
        prop_assert_eq!(changed.expected_events, base.expected_events);
        prop_assert_eq!(changed.fixture_arguments, base.fixture_arguments);
        prop_assert_eq!(changed.driver, base.driver);
    }

    /// Renaming one variable everywhere in the row renames it in the model
    /// and leaves the check unchanged.
    #[test]
    fn renaming_a_variable_renames_the_model(spec in arb_spec(), pick in any::<Index>()) {
        // The iteration index `i` is named by the iteration fixture itself.
        let renamable: Vec<&String> = spec.vars.iter().map(|v| &v.0).filter(|n| *n != "i").collect();
        let old = pick.get(&renamable).to_string();
        let new = "renamed";
        let rename = |n: &String| if *n == old { new.to_owned() } else { n.clone() };
        let mut renamed = spec.clone();
        renamed.vars.iter_mut().for_each(|v| v.0 = rename(&v.0));
        renamed.constants = spec.constants.iter().map(rename).collect();
        renamed.observe = spec.observe.iter().map(rename).collect();
        renamed.state = spec.state.iter().map(|(k, v)| (rename(k), v.clone())).collect();
        renamed.driver = spec.driver.iter().map(|(e, b)| (e.clone(), b.as_ref().map(|(v, x)| (rename(v), x.clone())))).collect();
        let base = generate(&spec)?;
        let after = generate(&renamed)?;
        // Declarations are listed by name, so compare line sets.
        let expected = replace_word(&base.model, &old, new);
        prop_assert_eq!(sorted_lines(&after.model), sorted_lines(&expected));
        prop_assert_eq!(after.expected_csp, base.expected_csp);
        prop_assert_eq!(after.expected_events, base.expected_events);
        let mut before_args = base.fixture_arguments.clone();
        let mut after_args = after.fixture_arguments.clone();
        before_args.sort();
        after_args.sort();
        prop_assert_eq!(after_args, before_args);
    }

    /// Reordering the observed variables reorders the observation record and
    /// the expected tuple in the same way.
    #[test]
    fn permuting_the_observation_permutes_the_check(
        (spec, order) in arb_spec().prop_flat_map(|s| {
            let order = Just((0..s.observe.len()).collect::<Vec<_>>()).prop_shuffle();
            (Just(s), order)
        })
    ) {
        let base = generate(&spec)?;
        let mut permuted = spec.clone();
        permuted.observe = order.iter().map(|&i| spec.observe[i].clone()).collect();
        let after = generate(&permuted)?;
        let fixtures = fixtures_of(&spec);
        let component = |name: &String| fixtures.csp_literal(&spec.ty(name).text(), &spec.state[name]).unwrap();
        let expected: Vec<String> = permuted.observe.iter().map(component).collect();
        prop_assert_eq!(&after.expected_csp, &format!("({})", expected.join(",")));
        let fields: Vec<String> = permuted.observe.iter().map(|n| format!("{n}: {}", spec.ty(n).text())).collect();
        let record = format!("record Obs {{ {} }}\n", fields.join(" "));
        prop_assert!(after.model.contains(&record), "missing {:?}", record);
        let values: Vec<String> = permuted.observe.iter().map(|n| format!("{n} = {n}")).collect();
        let action = format!("output ! Obs(| {} |)", values.join(", "));
        prop_assert!(after.model.contains(&action), "missing {:?}", action);
        prop_assert_eq!(sorted_lines(&after.model).len(), sorted_lines(&base.model).len());
    }

    /// The expected final state determines the expected tuple: changing an
    /// observed value changes it, and changing an initial value does not.
    #[test]
    fn the_check_depends_on_the_expected_state_only(
        (spec, pick, replacement) in arb_spec().prop_flat_map(|s| {
            let i = any::<Index>();
            (Just(s), i, any::<Index>())
        }).prop_flat_map(|(s, i, j)| {
            let name = i.get(&s.vars).0.clone();
            let ty = s.ty(&name).clone();
            let v = arb_value(&s.schema, &ty);
            (Just(s), Just((name, j)), v)
        })
    ) {
        let (name, _) = pick;
        let base = generate(&spec)?;
        if spec.state.contains_key(&name) && spec.state[&name] != replacement {
            let mut changed = spec.clone();
            changed.state.insert(name.clone(), replacement.clone());
            prop_assert_ne!(generate(&changed)?.expected_csp, base.expected_csp.clone());
        }
        let initial = &spec.vars.iter().find(|(n, _, _)| *n == name).unwrap().2;
        if *initial != replacement {
            let mut changed = spec.clone();
            changed.vars.iter_mut().find(|(n, _, _)| *n == name).unwrap().2 = replacement.clone();
            let after = generate(&changed)?;
            prop_assert_eq!(&after.expected_csp, &base.expected_csp);
            prop_assert_eq!(&after.expected_events, &base.expected_events);
            prop_assert_ne!(after.model, base.model);
        }
    }

    /// The expected status changes only the expected status.
    #[test]
    fn the_expected_status_changes_nothing_else(spec in arb_spec()) {
        let pass = statement_model(with_status(row_of(&spec), "pass")).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mismatch = statement_model(with_status(row_of(&spec), "mismatch")).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(pass.expected_status, "pass");
        prop_assert_eq!(mismatch.expected_status, "mismatch");
        prop_assert_eq!(pass.model, mismatch.model);
        prop_assert_eq!(pass.expected_csp, mismatch.expected_csp);
        prop_assert_eq!(pass.expected_events, mismatch.expected_events);
    }

    /// A bounded iteration gives the same model as writing the unrolled body
    /// by hand, apart from the comment that records the iteration.
    #[test]
    fn iteration_equals_manual_unrolling(spec in arb_spec().prop_filter("iterates", |s| s.iteration.is_some())) {
        let count = spec.iteration.unwrap();
        let iterated = generate(&spec)?;
        let mut manual = row_of(&spec);
        manual.iteration = String::new();
        manual.robochart = (1..=count).map(|k| format!("i = {k}; work({k})")).collect::<Vec<_>>().join("; ");
        let unrolled = statement_model(manual).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let without_note: String = iterated
            .model
            .lines()
            .filter(|l| !l.starts_with("// Fixed iteration fixture:"))
            .map(|l| format!("{l}\n"))
            .collect();
        prop_assert_eq!(without_note, unrolled.model);
        prop_assert_eq!(iterated.expected_csp, unrolled.expected_csp);
    }

    /// Adding a variable that is not observed changes nothing but its own
    /// declaration.
    #[test]
    fn unobserved_variables_do_not_change_the_check(spec in arb_spec()) {
        let base = generate(&spec)?;
        let mut extended = spec.clone();
        extended.vars.push(("extra".into(), Ty::Int, json!(0)));
        let with_extra = generate(&extended)?;
        prop_assert_eq!(&with_extra.expected_csp, &base.expected_csp);
        prop_assert_eq!(&with_extra.expected_events, &base.expected_events);
        prop_assert_eq!(&with_extra.fixture_arguments, &base.fixture_arguments);
        prop_assert_eq!(&with_extra.driver, &base.driver);
        let removed = with_extra.model.replacen("      var extra : int = 0\n", "", 1);
        prop_assert_eq!(removed, base.model);
    }

    /// One targeted fault in a well-formed row makes the generator reject it.
    #[test]
    fn single_faults_are_rejected(spec in arb_spec(), fault in 0u8..14, pick in any::<Index>()) {
        let mut row = row_of(&spec);
        let edit_json = |text: &str, f: &dyn Fn(&mut Value)| {
            let mut value: Value = serde_json::from_str(text).unwrap_or(json!(null));
            f(&mut value);
            value.to_string()
        };
        let typed_step = spec.driver.iter().find_map(|(_, b)| b.clone());
        match fault {
            0 => {
                let name = pick.get(&spec.vars).0.clone();
                row.types = edit_json(&row.types, &|v| { v.as_object_mut().unwrap().remove(&name); });
            }
            1 => row.python_output = edit_json(&row.python_output, &|v| { v["state"]["ghost"] = json!(0); }),
            2 => row.observe = edit_json(&row.observe, &|v| { let first = v[0].clone(); v.as_array_mut().unwrap().push(first); }),
            3 => row.observe = "[]".into(),
            4 => row.python_output = edit_json(&row.python_output, &|v| { v["events"].as_array_mut().unwrap().push(json!({"event": "ghost"})); }),
            5 => {
                if spec.trace.is_empty() {
                    return Ok(());
                }
                row.python_output = edit_json(&row.python_output, &|v| {
                    let item = &mut v["events"][0];
                    if item.get("value").is_some() { item.as_object_mut().unwrap().remove("value"); } else { item["value"] = json!(0); }
                });
            }
            6 => {
                let Some((var, _)) = typed_step else { return Ok(()) };
                let mut constants = spec.constants.clone();
                constants.insert(var);
                row.constants = json!(constants).to_string();
            }
            7 => {
                if spec.iteration.is_none() {
                    return Ok(());
                }
                row.iteration = json!({"index": "i", "count": 65}).to_string();
            }
            8 => row.datatypes = edit_json(if row.datatypes.is_empty() { "{}" } else { &row.datatypes }, &|v| { v["Obs"] = json!([["f", "int"]]); }),
            9 => row.expected_status = "maybe".into(),
            10 => {
                if spec.inputs.is_empty() {
                    return Ok(());
                }
                row.driver = String::new();
            }
            11 => row.events = edit_json(if row.events.is_empty() { "{}" } else { &row.events }, &|v| { v["output"] = Value::Null; }),
            12 => {
                let name = pick.get(&spec.vars).0.clone();
                row.inputs = edit_json(&row.inputs, &|v| { v[&name] = json!("oops"); });
            }
            _ => row.constants = json!(["ghost"]).to_string(),
        }
        prop_assert!(statement_model(row).is_err(), "fault {} was accepted", fault);
    }

    /// Arbitrary text in any column never makes the generator panic.
    #[test]
    fn arbitrary_columns_never_panic(spec in arb_spec(), column in 0usize..12, text in prop_oneof![".{0,40}", "[\\[\\]{}\":,0-9a-z ]{0,40}"]) {
        let mut row = row_of(&spec);
        let slot = match column {
            0 => &mut row.inputs, 1 => &mut row.python_output, 2 => &mut row.types, 3 => &mut row.observe,
            4 => &mut row.datatypes, 5 => &mut row.events, 6 => &mut row.input_events, 7 => &mut row.driver,
            8 => &mut row.iteration, 9 => &mut row.domains, 10 => &mut row.constants, _ => &mut row.function_definitions,
        };
        *slot = text;
        let _ = statement_model(row);
    }
}

// ---------------------------------------------------------------------------
// Typed expression rows
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A typed expression row declares its inputs and expects its output
    /// using the fixture encoder, embeds the expression verbatim, and passes
    /// constants in name order.
    #[test]
    fn typed_expressions_use_the_fixture_encoding(
        (schema, inputs, output_ty, output) in arb_schema().prop_flat_map(|schema| {
            let names = schema.names();
            let inputs = prop::collection::btree_map(prop::sample::select(vec!["a", "b", "cmd"]), arb_ty(names.clone(), 1), 1..=3);
            (Just(schema), inputs, arb_ty(names, 1))
        }).prop_flat_map(|(schema, inputs, output_ty)| {
            let values: Vec<_> = inputs.values().map(|t| arb_value(&schema, t)).collect();
            let output = arb_value(&schema, &output_ty);
            (Just(schema), Just(inputs), values, Just(output_ty), output)
        }).prop_map(|(schema, inputs, values, output_ty, output)| {
            let inputs: Vec<(String, Ty, Value)> = inputs.into_iter().zip(values).map(|((n, t), v)| (n.to_owned(), t, v)).collect();
            (schema, inputs, output_ty, output)
        })
    ) {
        let fixtures = Fixtures::new(&schema.datatypes()).unwrap();
        let case = ExpressionTestCase {
            id: "prop/case".into(),
            feature: "prop".into(),
            robochart: "a".into(),
            inputs: Value::Object(inputs.iter().map(|(n, _, v)| (n.clone(), v.clone())).collect()).to_string(),
            python_output: json!({"value": output}).to_string(),
            types: Value::Object(inputs.iter().map(|(n, t, _)| (n.clone(), json!(t.text()))).collect()).to_string(),
            datatypes: schema.datatypes(),
            output_type: output_ty.text(),
            functions: String::new(),
            function_definitions: String::new(),
            timing: String::new(),
            fixture_constants: Default::default(),
        };
        let generated = expression_model(case).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert!(generated.model.contains("action output ! (a)"));
        for (name, ty, value) in &inputs {
            let line = format!("      const {name} : {} = {}\n", ty.text(), fixtures.robo_literal(&ty.text(), value).unwrap());
            prop_assert_eq!(generated.model.matches(&line).count(), 1, "{:?}", line);
        }
        let arguments: Vec<String> = inputs.iter().map(|(_, t, v)| fixtures.csp_literal(&t.text(), v).unwrap()).collect();
        prop_assert_eq!(generated.fixture_arguments, arguments);
        prop_assert_eq!(generated.expected_csp, fixtures.csp_literal(&output_ty.text(), &output).unwrap());
    }
}

// ---------------------------------------------------------------------------
// CSV snapshots
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Copying every row of a CSV snapshot reproduces it; copying a
    /// selection reproduces exactly the selected rows, in order.
    #[test]
    fn csv_snapshots_round_trip(specs in prop::collection::vec(arb_spec(), 1..4), selection in prop::collection::vec(any::<bool>(), 4)) {
        let directory = env::temp_dir().join(format!("robochart-csv-{}-{}", std::process::id(), PROJECTS.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.csv");
        let mut writer = csv::Writer::from_path(&source).unwrap();
        writer.write_record(["id", "robochart", "inputs", "python_output"]).unwrap();
        let rows: Vec<Vec<String>> = specs.iter().enumerate().map(|(i, s)| {
            let row = row_of(&s);
            vec![format!("prop/case{i}"), row.robochart, row.inputs, row.python_output]
        }).collect();
        for row in &rows {
            writer.write_record(row).unwrap();
        }
        writer.flush().unwrap();
        let read = |path: &Path| -> Vec<Vec<String>> {
            csv::Reader::from_path(path).unwrap().records().map(|r| r.unwrap().iter().map(str::to_owned).collect()).collect()
        };
        let all = directory.join("all.csv");
        copy_selected_csv(&source, &all, &[], true).unwrap();
        let chosen: Vec<String> = rows.iter().zip(&selection).filter(|(_, s)| **s).map(|(r, _)| r[0].clone()).collect();
        let some = directory.join("some.csv");
        copy_selected_csv(&source, &some, &chosen, false).unwrap();
        let (all_rows, some_rows) = (read(&all), read(&some));
        let _ = fs::remove_dir_all(&directory);
        prop_assert_eq!(all_rows, rows.clone());
        let expected: Vec<Vec<String>> = rows.into_iter().filter(|r| chosen.contains(&r[0])).collect();
        prop_assert_eq!(some_rows, expected);
    }
}

// ---------------------------------------------------------------------------
// Every part of a row matters
// ---------------------------------------------------------------------------

/// Everything the generator produces for a row, or `None` when it rejects it.
type Output = (String, String, Vec<String>, Vec<String>, Option<(String, Vec<String>)>);

fn output_of(row: StatementTestCase) -> Option<Output> {
    statement_model(row).ok().map(|c| {
        (c.model, c.expected_csp, c.expected_events, c.fixture_arguments, c.driver)
    })
}

fn other_type(ty: &Ty) -> Ty {
    match ty {
        Ty::Bool => Ty::Int,
        _ => Ty::Bool,
    }
}

/// One small change to one part of a well-formed row, or `None` when the row
/// has no such part.
fn change(spec: &Spec, kind: u8, pick: Index) -> Option<StatementTestCase> {
    let mut changed = spec.clone();
    match kind {
        // A record field's declared type.
        0 => {
            if spec.schema.records.is_empty() {
                return None;
            }
            let (_, fields) = &mut changed.schema.records[pick.index(spec.schema.records.len())];
            let field = &mut fields[0];
            field.1 = other_type(&field.1);
        }
        // An extra record declaration that nothing uses.
        1 => changed.schema.records.push(("Extra".into(), vec![("e".into(), Ty::Int)])),
        // A record removed from the declarations.
        2 => {
            if spec.schema.records.is_empty() {
                return None;
            }
            let name = pick.get(&spec.schema.records).0.clone();
            let mut row = row_of(spec);
            let mut datatypes: Value = serde_json::from_str(&row.datatypes).ok()?;
            datatypes.as_object_mut()?.remove(&name);
            row.datatypes = datatypes.to_string();
            return Some(row);
        }
        // A field renamed in the declaration but not in the values.
        3 => {
            if spec.schema.records.is_empty() {
                return None;
            }
            let (_, fields) = &mut changed.schema.records[pick.index(spec.schema.records.len())];
            fields[0].0 = "renamedfield".into();
        }
        // A driver payload.
        4 => {
            let step = spec.driver.iter().position(|(_, b)| b.is_some())?;
            let (var, value) = spec.driver[step].1.clone()?;
            changed.driver[step].1 = Some((var, nudge(&value)?));
        }
        // An expected event payload.
        5 => {
            let item = spec.trace.iter().position(|(_, v)| v.is_some())?;
            changed.trace[item].1 = Some(nudge(spec.trace[item].1.as_ref()?)?);
        }
        // Whether a variable is a constant.
        6 => {
            let bound: BTreeSet<&String> = spec.driver.iter().filter_map(|(_, b)| b.as_ref().map(|(v, _)| v)).collect();
            let candidates: Vec<&String> = spec.vars.iter().map(|v| &v.0).filter(|n| *n != "i" && !bound.contains(n)).collect();
            if candidates.is_empty() {
                return None;
            }
            let name = pick.get(&candidates).to_string();
            if !changed.constants.remove(&name) {
                changed.constants.insert(name);
            }
        }
        // The iteration count.
        7 => changed.iteration = Some(spec.iteration? + 1),
        // An output event's payload type.
        8 => {
            if spec.outputs.is_empty() {
                return None;
            }
            let event = &mut changed.outputs[pick.index(spec.outputs.len())];
            event.1 = match &event.1 {
                Some(_) => None,
                None => Some(Ty::Int),
            };
        }
        // An input event's payload type.
        9 => {
            if spec.inputs.is_empty() {
                return None;
            }
            let event = &mut changed.inputs[pick.index(spec.inputs.len())];
            event.1 = match &event.1 {
                Some(t) => Some(other_type(t)),
                None => Some(Ty::Int),
            };
        }
        // The observed variables.
        10 => {
            if spec.observe.len() < 2 {
                return None;
            }
            let dropped = changed.observe.pop()?;
            changed.state.remove(&dropped);
        }
        // A variable's initial value.
        11 => {
            let var = pick.get(&spec.vars).0.clone();
            let entry = changed.vars.iter_mut().find(|v| v.0 == var)?;
            entry.2 = nudge(&entry.2)?;
        }
        // An expected final value.
        12 => {
            let name = pick.get(&spec.observe).clone();
            let value = nudge(&spec.state[&name])?;
            changed.state.insert(name, value);
        }
        // The source.
        _ => changed.body = format!("{}; skip", spec.body),
    }
    Some(row_of(&changed))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Changing any one part of a well-formed row either changes what the
    /// generator produces or makes it reject the row. The one exception is
    /// the source of a row that iterates zero times: its body is unrolled
    /// zero times, so the source never reaches the model.
    #[test]
    fn every_part_of_a_row_matters(spec in arb_spec(), kind in 0u8..14, pick in any::<Index>()) {
        let before = output_of(row_of(&spec)).ok_or_else(|| TestCaseError::fail("well-formed row rejected"))?;
        let Some(row) = change(&spec, kind, pick) else { return Ok(()) };
        let source_unrolled_zero_times = kind == 13 && spec.iteration == Some(0);
        match output_of(row) {
            Some(after) if source_unrolled_zero_times => prop_assert_eq!(after, before),
            Some(after) => prop_assert_ne!(after, before, "change {} was ignored", kind),
            None => prop_assert!(!source_unrolled_zero_times),
        }
    }
}

// ---------------------------------------------------------------------------
// Untyped expression rows
// ---------------------------------------------------------------------------

fn expression_test_case(row: &ExpressionRow) -> ExpressionTestCase {
    let columns: Map<String, Value> = row
        .columns("prop/case")
        .into_iter()
        .map(|(k, v)| (k.to_owned(), Value::String(v)))
        .collect();
    serde_json::from_value(Value::Object(columns)).expect("columns match ExpressionTestCase")
}

/// The declared type and literal of each constant in an expression model.
fn declared_constants(model: &str) -> BTreeMap<String, (String, String)> {
    model
        .lines()
        .filter_map(|line| line.trim().strip_prefix("const "))
        .filter_map(|rest| {
            let (name, rest) = rest.split_once(" : ")?;
            let (ty, literal) = rest.split_once(" = ")?;
            Some((name.to_owned(), (ty.to_owned(), literal.to_owned())))
        })
        .collect()
}

fn output_type(model: &str) -> Option<String> {
    model
        .lines()
        .find_map(|l| l.trim().strip_prefix("event output : ").map(str::to_owned))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Every untyped row with supported values is accepted. Each input is
    /// declared once as a constant, with the RoboChart literal of its value
    /// at its declared type, and the CSP literal of the same value is the
    /// argument passed for it to the CSP process, in name order.
    #[test]
    fn untyped_inputs_are_declared_and_passed_consistently(row in arb_expression_row()) {
        let case = expression_model(expression_test_case(&row)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let declared = declared_constants(&case.model);
        let names: Vec<&String> = row.inputs.keys().collect();
        prop_assert_eq!(declared.keys().collect::<Vec<_>>(), names);
        let fixtures = Fixtures::default();
        let literals: Vec<String> = declared.iter().map(|(n, (t, _))| fixtures.robo_literal(t, &row.inputs[n]).unwrap()).collect();
        prop_assert_eq!(literals, declared.values().map(|(_, l)| l.clone()).collect::<Vec<_>>());
        let arguments: Vec<String> = declared.iter().map(|(n, (t, _))| fixtures.csp_literal(t, &row.inputs[n]).unwrap()).collect();
        prop_assert_eq!(arguments, case.fixture_arguments.clone());
        for name in row.inputs.keys() {
            let declaration = format!("const {name} : ");
            prop_assert_eq!(case.model.matches(&declaration).count(), 1);
        }
    }

    /// Different outputs give different expected values, and different input
    /// values give different arguments. The expected value does not depend on
    /// the inputs, and the inputs' declarations do not depend on the output.
    #[test]
    fn untyped_outputs_and_inputs_are_independent_and_injective(
        row in arb_expression_row(),
        other_output in arb_untyped_value(),
        other_input in arb_untyped_value(),
        pick in any::<Index>(),
    ) {
        let base = expression_model(expression_test_case(&row)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut new_output = row.clone();
        new_output.output = other_output.clone();
        let with_output = expression_model(expression_test_case(&new_output)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&with_output.fixture_arguments, &base.fixture_arguments);
        prop_assert_eq!(declared_constants(&with_output.model), declared_constants(&base.model));
        let same_kind = output_type(&with_output.model) == output_type(&base.model);
        if other_output != row.output && same_kind && !(row.output.is_object() && other_output.is_object()) {
            prop_assert_ne!(&with_output.expected_csp, &base.expected_csp);
        }
        let names: Vec<String> = row.inputs.keys().filter(|n| *n != "domain").cloned().collect();
        if names.is_empty() {
            return Ok(());
        }
        let name = pick.get(&names).clone();
        let mut new_input = row.clone();
        new_input.inputs.insert(name.clone(), other_input.clone());
        let with_input = expression_model(expression_test_case(&new_input)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&with_input.expected_csp, &base.expected_csp);
        if other_input != row.inputs[&name] {
            prop_assert_ne!(&with_input.fixture_arguments, &base.fixture_arguments);
        }
    }

    /// When the output is a copy of an input, the output channel has that
    /// input's declared type, and the expected value is that input's argument.
    #[test]
    fn untyped_output_copying_an_input_matches_it(row in arb_expression_row()) {
        let case = expression_model(expression_test_case(&row)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let declared = declared_constants(&case.model);
        for (position, (name, value)) in row.inputs.iter().enumerate() {
            if *value == row.output && name != "domain" {
                let (ty, _) = &declared[name];
                let channel = output_type(&case.model);
                prop_assert_eq!(channel.as_ref(), Some(ty));
                prop_assert_eq!(&case.expected_csp, &case.fixture_arguments[position]);
            }
        }
    }

    /// No literal in an untyped expression's model or check contains `<-`.
    #[test]
    fn untyped_literals_never_contain_the_arrow_token(row in arb_expression_row()) {
        let case = expression_model(expression_test_case(&row)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert!(!case.model.contains("<-"), "{}", case.model);
        prop_assert!(!case.expected_csp.contains("<-"), "{}", case.expected_csp);
        for argument in &case.fixture_arguments {
            prop_assert!(!argument.contains("<-"), "{}", argument);
        }
    }

    /// Replacing the expression changes only the output action.
    #[test]
    fn untyped_expressions_are_embedded_verbatim(row in arb_expression_row()) {
        let base = expression_model(expression_test_case(&row)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut other = row.clone();
        other.robochart = "b".into();
        let changed = expression_model(expression_test_case(&other)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(changed.model, base.model.replacen("output ! (a)", "output ! (b)", 1));
        prop_assert_eq!(changed.expected_csp, base.expected_csp);
        prop_assert_eq!(changed.fixture_arguments, base.fixture_arguments);
    }

    /// Arbitrary text in the input or output column never makes the generator
    /// panic.
    #[test]
    fn untyped_rows_never_panic(row in arb_expression_row(), text in prop_oneof![".{0,40}", "[\\[\\]{}\":,0-9a-z ]{0,40}"], column in 0usize..2) {
        let mut case = expression_test_case(&row);
        if column == 0 { case.inputs = text } else { case.python_output = text }
        let _ = expression_model(case);
    }
}

// ---------------------------------------------------------------------------
// Lambda rows
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The graph context embeds the source once and checks the supplied
    /// graph slices, including undefined arguments. Expectations come from
    /// the fixture, even when they deliberately disagree with the source.
    #[test]
    fn lambda_checks_list_the_graph(
        points in prop::collection::btree_map(-3i64..=3, prop::option::of(-3i64..=3), 1..=5),
        predicate in any::<bool>(),
        pick in any::<Index>(),
    ) {
        let lambda = if predicate { "lambda x: int | x > 0 @ x + 1" } else { "lambda x: int @ x + 1" };
        let row = |points: &BTreeMap<i64, Option<i64>>| {
            let graph: Vec<Value> = points.iter().map(|(a, v)| match v {
                Some(v) => json!({"arg": a, "defined": true, "value": v}),
                None => json!({"arg": a, "defined": false}),
            }).collect();
            ExpressionTestCase {
                id: "lambda_prop/000".into(),
                feature: "lambda_prop".into(),
                robochart: lambda.into(),
                inputs: json!({"apply": points.keys().collect::<Vec<_>>()}).to_string(),
                python_output: json!({"graph": graph}).to_string(),
                types: String::new(),
                datatypes: String::new(),
                output_type: String::new(),
                functions: String::new(),
                function_definitions: String::new(),
            timing: String::new(),
            fixture_constants: Default::default(),
            }
        };
        let case = expression_model(row(&points)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let observation = format!("rimage(({lambda}), {{arg}})");
        prop_assert!(case.model.contains(&observation));
        prop_assert_eq!(case.model.matches(lambda).count(), 1);
        let check = case.custom_check.clone().expect("lambda rows have their own check");
        prop_assert!(!check.contains("{-"), "graph values must not open CSP comments: {}", check);
        for (argument, value) in &points {
            let observed = match value { Some(v) => format!("{{ {v}}}"), None => "{}".to_owned() };
            let branch = format!("lambda_argument.{argument} -> expression_lambda_prop_000::mod0::output.out.{observed} -> ");
            prop_assert_eq!(check.matches(&branch).count(), 1, "{}", check);
        }
        let branches = check.matches(" -> expression_lambda_prop_000::mod0::output.out.").count();
        prop_assert_eq!(branches, points.len(), "{}", check);
        let mut changed = points.clone();
        let argument = **pick.get(&points.keys().collect::<Vec<_>>());
        let old = changed[&argument];
        changed.insert(argument, match old { Some(v) => Some(v + 1), None => Some(0) });
        let other = expression_model(row(&changed)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_ne!(other.custom_check, case.custom_check);
    }

    /// A lambda row whose graph does not give exactly one point per argument
    /// is rejected.
    #[test]
    fn lambda_graphs_must_match_the_arguments(arguments in prop::collection::btree_set(-3i64..=3, 1..=4), extra in 4i64..=6) {
        let graph: Vec<Value> = arguments.iter().chain([&extra]).map(|a| json!({"arg": a, "defined": false})).collect();
        let case = ExpressionTestCase {
            id: "lambda_prop/000".into(),
            feature: "lambda_prop".into(),
            robochart: "lambda x: int @ x + 1".into(),
            inputs: json!({"apply": arguments}).to_string(),
            python_output: json!({"graph": graph}).to_string(),
            types: String::new(),
            datatypes: String::new(),
            output_type: String::new(),
            functions: String::new(),
            function_definitions: String::new(),
            timing: String::new(),
            fixture_constants: Default::default(),
        };
        prop_assert!(expression_model(case).is_err());
    }
}
