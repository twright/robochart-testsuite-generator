//! Random, well-formed fixture values and case rows, shared by the
//! generator's property tests and the pipeline integration tests.

#![allow(dead_code)]

use proptest::prelude::*;
use proptest::sample::Index;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------
// Fixture types and values
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    Int,
    Nat,
    Bool,
    Seq(Box<Ty>),
    Rec(String),
}

impl Ty {
    pub fn text(&self) -> String {
        match self {
            Ty::Int => "int".into(),
            Ty::Nat => "nat".into(),
            Ty::Bool => "boolean".into(),
            Ty::Seq(item) => format!("Seq({})", item.text()),
            Ty::Rec(name) => name.clone(),
        }
    }
}

/// Records in declaration order; each field list keeps its declared order.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    pub records: Vec<(String, Vec<(String, Ty)>)>,
}

impl Schema {
    pub fn fields(&self, name: &str) -> &[(String, Ty)] {
        &self
            .records
            .iter()
            .find(|(n, _)| n == name)
            .expect("record in schema")
            .1
    }

    pub fn names(&self) -> Vec<String> {
        self.records.iter().map(|(n, _)| n.clone()).collect()
    }

    pub fn datatypes(&self) -> String {
        if self.records.is_empty() {
            return String::new();
        }
        let object: Map<String, Value> = self
            .records
            .iter()
            .map(|(name, fields)| {
                let pairs = fields.iter().map(|(f, t)| json!([f, t.text()])).collect();
                (name.clone(), Value::Array(pairs))
            })
            .collect();
        Value::Object(object).to_string()
    }
}

pub const RECORD_NAMES: [&str; 3] = ["Spin", "Pose", "Cmd"];
// Deliberately not in alphabetical order, so a sorted layout would be caught.
pub const FIELD_NAMES: [&str; 5] = ["zeta", "alpha", "mid", "beta", "omega"];

pub fn arb_ty(records: Vec<String>, depth: u32) -> BoxedStrategy<Ty> {
    let mut leaves = vec![Just(Ty::Int).boxed(), Just(Ty::Nat).boxed(), Just(Ty::Bool).boxed()];
    if !records.is_empty() {
        leaves.push(prop::sample::select(records).prop_map(Ty::Rec).boxed());
    }
    prop::strategy::Union::new(leaves)
        .prop_recursive(depth, 4, 1, |inner| inner.prop_map(|t| Ty::Seq(Box::new(t))))
        .boxed()
}

/// A schema whose record `i` refers only to records declared before it, so
/// it is acyclic by construction.
pub fn arb_schema() -> BoxedStrategy<Schema> {
    (0..=RECORD_NAMES.len(), Just(RECORD_NAMES.to_vec()).prop_shuffle())
        .prop_flat_map(|(n, names)| {
            let names: Vec<String> = names[..n].iter().map(|s| s.to_string()).collect();
            let fields: Vec<_> = (0..n).map(|i| arb_fields(names[..i].to_vec())).collect();
            (Just(names), fields)
        })
        .prop_map(|(names, fields)| Schema {
            records: names.into_iter().zip(fields).collect(),
        })
        .boxed()
}

pub fn arb_fields(visible: Vec<String>) -> BoxedStrategy<Vec<(String, Ty)>> {
    (1..=3usize, Just(FIELD_NAMES.to_vec()).prop_shuffle())
        .prop_flat_map(move |(k, names)| {
            let names: Vec<String> = names[..k].iter().map(|s| s.to_string()).collect();
            prop::collection::vec(arb_ty(visible.clone(), 1), k)
                .prop_map(move |tys| names.clone().into_iter().zip(tys).collect())
        })
        .boxed()
}

pub fn arb_value(schema: &Schema, ty: &Ty) -> BoxedStrategy<Value> {
    match ty {
        Ty::Int => (-3i64..=3).prop_map(Value::from).boxed(),
        Ty::Nat => (0i64..=3).prop_map(Value::from).boxed(),
        Ty::Bool => any::<bool>().prop_map(Value::from).boxed(),
        Ty::Seq(item) => prop::collection::vec(arb_value(schema, item), 0..=2)
            .prop_map(Value::Array)
            .boxed(),
        Ty::Rec(name) => schema
            .fields(name)
            .iter()
            .map(|(field, ty)| {
                let field = field.clone();
                arb_value(schema, ty).prop_map(move |v| (field.clone(), v))
            })
            .collect::<Vec<_>>()
            .prop_map(|pairs| Value::Object(pairs.into_iter().collect()))
            .boxed(),
    }
}

/// A schema, a type over it, and a value of that type.
pub fn arb_typed_value() -> BoxedStrategy<(Schema, Ty, Value)> {
    arb_schema()
        .prop_flat_map(|schema| {
            let ty = arb_ty(schema.names(), 2);
            (Just(schema), ty)
        })
        .prop_flat_map(|(schema, ty)| {
            let value = arb_value(&schema, &ty);
            (Just(schema), Just(ty), value)
        })
        .boxed()
}


pub fn arb_json() -> BoxedStrategy<Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        (-4i64..=4).prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        (-2.0f64..2.0).prop_map(|f| json!(f)),
        "[a-z]{0,3}".prop_map(Value::from),
    ];
    leaf.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(Value::Array),
            prop::collection::btree_map(prop::sample::select(FIELD_NAMES.to_vec()), inner, 0..3)
                .prop_map(|m| Value::Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())),
        ]
    })
    .boxed()
}

/// Every path into a JSON value, as a list of array indices or object keys.
pub fn paths(value: &Value) -> Vec<Vec<Value>> {
    let mut out = vec![Vec::new()];
    match value {
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                for mut p in paths(item) {
                    p.insert(0, json!(i));
                    out.push(p);
                }
            }
        }
        Value::Object(fields) => {
            for (k, item) in fields {
                for mut p in paths(item) {
                    p.insert(0, json!(k));
                    out.push(p);
                }
            }
        }
        _ => {}
    }
    out
}

pub fn at_mut<'a>(value: &'a mut Value, path: &[Value]) -> &'a mut Value {
    path.iter().fold(value, |v, step| match step {
        Value::Number(i) => &mut v[i.as_u64().unwrap() as usize],
        Value::String(k) => &mut v[k.as_str()],
        _ => unreachable!(),
    })
}

/// A well-typed value with at most one small structural fault, which may or
/// may not break its type.
pub fn arb_perturbed(value: Value) -> BoxedStrategy<Value> {
    let all = paths(&value);
    (any::<Index>(), 0u8..6, arb_json())
        .prop_map(move |(index, op, junk)| {
            let mut v = value.clone();
            let target = at_mut(&mut v, index.get(&all));
            match (op, &mut *target) {
                (0, _) => *target = junk,
                (1, Value::Object(m)) => {
                    if let Some(k) = m.keys().next().cloned() {
                        m.remove(&k);
                    }
                }
                (2, Value::Object(m)) => {
                    m.insert("extra".into(), junk);
                }
                (3, Value::Number(n)) => {
                    if let Some(i) = n.as_i64() {
                        *target = json!(-i - 1);
                    }
                }
                (4, Value::Array(items)) => items.push(junk),
                _ => {}
            }
            v
        })
        .boxed()
}


pub const VARIABLE_NAMES: [&str; 6] = ["x", "total", "ranges", "cmd", "flag", "level"];
pub const OUTPUT_EVENTS: [&str; 2] = ["sent", "ack"];
pub const INPUT_EVENTS: [&str; 2] = ["scan", "go"];

/// A well-formed statement row in structured form.
#[derive(Clone, Debug)]
pub struct Spec {
    pub schema: Schema,
    /// Name, type and initial value, including the iteration index `i`.
    pub vars: Vec<(String, Ty, Value)>,
    pub constants: BTreeSet<String>,
    pub observe: Vec<String>,
    pub state: BTreeMap<String, Value>,
    pub outputs: Vec<(String, Option<Ty>)>,
    pub trace: Vec<(String, Option<Value>)>,
    pub inputs: Vec<(String, Option<Ty>)>,
    /// Event, and the bound variable with its value for a typed event.
    pub driver: Vec<(String, Option<(String, Value)>)>,
    pub iteration: Option<usize>,
    /// The `robochart` source.
    pub body: String,
    pub expected_status: String,
}

impl Spec {
    pub fn ty(&self, name: &str) -> &Ty {
        &self.vars.iter().find(|(n, _, _)| n == name).unwrap().1
    }

    /// The final state when the body does nothing: the initial state, updated
    /// by each driver binding in turn and by the iteration index.
    pub fn final_state_of_skip(&self) -> BTreeMap<String, Value> {
        let mut state: BTreeMap<String, Value> =
            self.vars.iter().map(|(n, _, v)| (n.clone(), v.clone())).collect();
        for (_, bind) in &self.driver {
            if let Some((var, value)) = bind {
                state.insert(var.clone(), value.clone());
            }
        }
        if let Some(count) = self.iteration.filter(|&c| c > 0) {
            state.insert("i".into(), json!(count));
        }
        state
    }

    /// The row as CSV columns, keyed by header name.
    pub fn columns(&self, id: &str) -> Vec<(&'static str, String)> {
        let object = |pairs: Vec<(String, Value)>| Value::Object(pairs.into_iter().collect()).to_string();
        let event_map = |events: &[(String, Option<Ty>)]| {
            if events.is_empty() {
                String::new()
            } else {
                object(
                    events
                        .iter()
                        .map(|(n, t)| (n.clone(), t.as_ref().map_or(Value::Null, |t| json!(t.text()))))
                        .collect(),
                )
            }
        };
        let trace: Vec<Value> = self
            .trace
            .iter()
            .map(|(event, value)| match value {
                Some(v) => json!({"event": event, "value": v}),
                None => json!({"event": event}),
            })
            .collect();
        let driver: Vec<Value> = self
            .driver
            .iter()
            .map(|(event, bind)| match bind {
                Some((var, value)) => json!({"event": event, "bind": var, "value": value}),
                None => json!({"event": event}),
            })
            .collect();
        vec![
            ("id", id.to_owned()),
            ("feature", id.split('/').next().unwrap_or("").to_owned()),
            ("robochart", self.body.clone()),
            ("inputs", object(self.vars.iter().map(|(n, _, v)| (n.clone(), v.clone())).collect())),
            ("python_output", json!({"state": self.state, "events": trace}).to_string()),
            ("types", object(self.vars.iter().map(|(n, t, _)| (n.clone(), json!(t.text()))).collect())),
            ("observe", json!(self.observe).to_string()),
            ("expected_status", self.expected_status.clone()),
            ("datatypes", self.schema.datatypes()),
            ("functions", String::new()),
            ("events", event_map(&self.outputs)),
            ("input_events", event_map(&self.inputs)),
            ("driver", if driver.is_empty() { String::new() } else { json!(driver).to_string() }),
            (
                "iteration",
                self.iteration
                    .map(|count| json!({"index": "i", "count": count}).to_string())
                    .unwrap_or_default(),
            ),
            ("function_definitions", String::new()),
            ("notes", String::new()),
            ("domains", String::new()),
            (
                "constants",
                if self.constants.is_empty() {
                    String::new()
                } else {
                    json!(self.constants).to_string()
                },
            ),
        ]
    }
}

#[allow(clippy::type_complexity)]
pub fn arb_spec() -> BoxedStrategy<Spec> {
    (arb_schema(), prop::option::of(0usize..=4))
        .prop_flat_map(|(schema, iteration)| {
            let records = schema.names();
            let vars = (1..=4usize, Just(VARIABLE_NAMES.to_vec()).prop_shuffle()).prop_flat_map(
                move |(k, pool)| {
                    let names: Vec<String> = pool[..k].iter().map(|s| s.to_string()).collect();
                    (Just(names), prop::collection::vec(arb_ty(records.clone(), 1), k))
                },
            );
            let outputs = prop::collection::vec(prop::option::of(arb_ty(schema.names(), 1)), 0..=2);
            (Just(schema), Just(iteration), vars, outputs, 0..=2usize)
        })
        .prop_flat_map(|(schema, iteration, (names, types), output_types, n_inputs)| {
            let mut declared: Vec<(String, Ty)> = names.into_iter().zip(types).collect();
            if iteration.is_some() {
                declared.push(("i".into(), Ty::Int));
            }
            let n = declared.len();
            let pools: Vec<_> = declared
                .iter()
                .map(|(_, t)| prop::collection::vec(arb_value(&schema, t), 2))
                .collect();
            let output_pools: Vec<_> = output_types
                .iter()
                .map(|t| match t {
                    Some(t) => prop::collection::vec(arb_value(&schema, t), 2).boxed(),
                    None => Just(Vec::new()).boxed(),
                })
                .collect();
            (
                Just(schema),
                Just(iteration),
                Just(declared),
                Just(output_types),
                pools,
                output_pools,
                (Just((0..n).collect::<Vec<_>>()).prop_shuffle(), 1..=n),
                prop::collection::vec(any::<bool>(), n),
                prop::collection::vec((any::<Index>(), any::<Index>()), 0..=3),
                prop::collection::vec((any::<bool>(), any::<Index>()), n_inputs),
                prop::collection::vec((any::<Index>(), any::<Index>(), any::<Index>()), 1..=3),
            )
        })
        .prop_map(
            |(schema, iteration, declared, output_types, pools, output_pools, (order, n_observed), const_mask, trace, inputs, steps)| {
                let constants: BTreeSet<String> = declared
                    .iter()
                    .zip(&const_mask)
                    .filter(|((name, _), c)| **c && name != "i")
                    .map(|((name, _), _)| name.clone())
                    .collect();
                let vars: Vec<(String, Ty, Value)> = declared
                    .iter()
                    .zip(&pools)
                    .map(|((n, t), pool)| (n.clone(), t.clone(), pool[0].clone()))
                    .collect();
                let observe: Vec<String> = order[..n_observed].iter().map(|&i| declared[i].0.clone()).collect();
                let state = observe
                    .iter()
                    .map(|name| {
                        let i = declared.iter().position(|(n, _)| n == name).unwrap();
                        (name.clone(), pools[i][1].clone())
                    })
                    .collect();
                let outputs: Vec<(String, Option<Ty>)> = output_types
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (OUTPUT_EVENTS[i].to_owned(), t.clone()))
                    .collect();
                let trace = if outputs.is_empty() {
                    Vec::new()
                } else {
                    trace
                        .iter()
                        .map(|(e, v)| {
                            let i = e.index(outputs.len());
                            let value = outputs[i].1.as_ref().map(|_| v.get(&output_pools[i]).clone());
                            (outputs[i].0.clone(), value)
                        })
                        .collect()
                };
                // A typed input event carries the type of a non-constant variable.
                let assignable: Vec<usize> =
                    (0..declared.len()).filter(|&i| !constants.contains(&declared[i].0)).collect();
                let inputs: Vec<(String, Option<Ty>)> = inputs
                    .iter()
                    .enumerate()
                    .map(|(i, (typed, var))| {
                        let ty = (*typed && !assignable.is_empty())
                            .then(|| declared[*var.get(&assignable)].1.clone());
                        (INPUT_EVENTS[i].to_owned(), ty)
                    })
                    .collect();
                let driver = if inputs.is_empty() {
                    Vec::new()
                } else {
                    steps
                        .iter()
                        .map(|(e, var, value)| {
                            let (event, ty) = e.get(&inputs).clone();
                            let bind = ty.map(|ty| {
                                let candidates: Vec<usize> =
                                    assignable.iter().copied().filter(|&i| declared[i].1 == ty).collect();
                                let chosen = *var.get(&candidates);
                                (declared[chosen].0.clone(), value.get(&pools[chosen]).clone())
                            });
                            (event, bind)
                        })
                        .collect()
                };
                let body = if iteration.is_some() { "work({{index}})" } else { "work()" }.to_owned();
                Spec {
                    body,
                    expected_status: "pass".into(),
                    schema,
                    vars,
                    constants,
                    observe,
                    state,
                    outputs,
                    trace,
                    inputs,
                    driver,
                    iteration,
                }
            },
        )
        .boxed()
}


// ---------------------------------------------------------------------------
// Untyped expression rows
// ---------------------------------------------------------------------------

/// An untyped expression row: inputs whose types the generator infers from
/// their JSON values, and an expected output value.
#[derive(Clone, Debug)]
pub struct ExpressionRow {
    pub inputs: BTreeMap<String, Value>,
    pub output: Value,
    pub robochart: String,
}

impl ExpressionRow {
    pub fn columns(&self, id: &str) -> Vec<(&'static str, String)> {
        vec![
            ("id", id.to_owned()),
            ("feature", id.split('/').next().unwrap_or("").to_owned()),
            ("robochart", self.robochart.clone()),
            ("inputs", json!(self.inputs).to_string()),
            ("python_output", json!({"value": self.output}).to_string()),
        ]
    }
}

/// A value of one of the kinds untyped expression rows support: an integer,
/// a Boolean, a sequence of integers, or a set of integers.
pub fn arb_untyped_value() -> BoxedStrategy<Value> {
    prop_oneof![
        (-3i64..=3).prop_map(Value::from),
        any::<bool>().prop_map(Value::from),
        prop::collection::vec((-3i64..=3).prop_map(Value::from), 0..=3).prop_map(Value::Array),
        prop::collection::vec((-3i64..=3).prop_map(Value::from), 0..=3)
            .prop_map(|items| json!({"type": "set", "items": items})),
    ]
    .boxed()
}

/// Inputs named `a`, `b` and `c`, and optionally a set `domain`; the output
/// is either a fresh value or a copy of one input.
pub fn arb_expression_row() -> BoxedStrategy<ExpressionRow> {
    let inputs = prop::collection::btree_map(
        prop::sample::select(vec!["a", "b", "c"]).prop_map(str::to_owned),
        arb_untyped_value(),
        1..=3,
    );
    let domain = prop::option::of(prop::collection::vec((-3i64..=3).prop_map(Value::from), 0..=3));
    (inputs, domain, arb_untyped_value(), any::<Option<Index>>())
        .prop_map(|(mut inputs, domain, fresh, copy)| {
            if let Some(items) = domain {
                inputs.insert("domain".into(), Value::Array(items));
            }
            let names: Vec<String> = inputs.keys().filter(|n| *n != "domain").cloned().collect();
            let output = match copy {
                Some(index) if !names.is_empty() => inputs[index.get(&names)].clone(),
                _ => fresh,
            };
            ExpressionRow {
                inputs,
                output,
                robochart: "a".into(),
            }
        })
        .boxed()
}

/// The value with its first integer incremented, or else its first Boolean
/// negated. `None` when it contains neither.
pub fn nudge(value: &Value) -> Option<Value> {
    fn walk(value: &mut Value) -> bool {
        match value {
            Value::Number(n) => {
                *value = json!(n.as_i64().unwrap() + 1);
                true
            }
            Value::Bool(b) => {
                *b = !*b;
                true
            }
            Value::Array(items) => items.iter_mut().any(walk),
            Value::Object(fields) => fields.values_mut().any(walk),
            _ => false,
        }
    }
    let mut changed = value.clone();
    walk(&mut changed).then_some(changed)
}

// ---------------------------------------------------------------------------
// Size of the finite model
// ---------------------------------------------------------------------------

/// An upper estimate of the number of values of a type in the finite model
/// FDR explores: about 8 integers, 5 naturals, and sequences of length at
/// most 2, which are the bounds `prepare_fdr` derives for generated rows.
pub fn domain_size(schema: &Schema, ty: &Ty) -> u64 {
    match ty {
        Ty::Int => 8,
        Ty::Nat => 5,
        Ty::Bool => 2,
        Ty::Seq(item) => {
            let n = domain_size(schema, item);
            1u64.saturating_add(n).saturating_add(n.saturating_mul(n))
        }
        Ty::Rec(name) => schema
            .fields(name)
            .iter()
            .fold(1u64, |size, (_, t)| size.saturating_mul(domain_size(schema, t))),
    }
}

impl Spec {
    /// Whether every variable and event type, and the observation record,
    /// has a finite domain of at most `limit` values.
    pub fn is_tractable(&self, limit: u64) -> bool {
        let types = self
            .vars
            .iter()
            .map(|(_, t, _)| t)
            .chain(self.outputs.iter().filter_map(|(_, t)| t.as_ref()))
            .chain(self.inputs.iter().filter_map(|(_, t)| t.as_ref()));
        let observed = self
            .observe
            .iter()
            .map(|n| self.vars.iter().find(|v| &v.0 == n).unwrap())
            .fold(1u64, |size, (_, t, _)| size.saturating_mul(domain_size(&self.schema, t)));
        observed <= limit && types.into_iter().all(|t| domain_size(&self.schema, t) <= limit)
    }
}
