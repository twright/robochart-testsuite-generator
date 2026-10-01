//! The context of a scalar expression: everything declared around it.
//!
//! [`SCALAR_CONTEXT_RULES`] is the complete, ordered list of what can
//! surround a scalar expression. Each rule is a named [`ContextRule`] that
//! looks at the row and adds declarations to a [`ScalarContext`]; the rules
//! that depend on a row's feature name are in the list too. Declaring a name
//! twice is a rejection, and [`ScalarContext::check`] rejects a reference to
//! an undeclared type, so a dependency between rules cannot fail silently.
//! Every declaration records the rule that made it (its origin), and the
//! model names that rule in a comment.

use super::{ExpressionTestCase, validate_quantifier_domains};
use crate::fixtures::{Fixtures, Type};
use crate::rejection::{Category, Rejection, from_json};
use crate::facts::BinderDomain;
use crate::{FunctionDefinition, package_name, validate_function_fixtures, validate_identifier};
use serde_json::Value;
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, Rejection>;

/// One independent source of context for scalar expressions.
pub trait ContextRule {
    /// The rule's name, written as the origin of what it declares.
    fn name(&self) -> &'static str;
    /// Add this rule's declarations for `case` to `context`, or reject the row.
    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()>;
}

/// The context rules for scalar expressions, in the order in which they run.
pub const SCALAR_CONTEXT_RULES: &[&dyn ContextRule] = &[
    &DeclaredDatatypes,
    &DeclaredFunctions,
    &DeclaredOutputType,
    &VectorOutputType,
    &MatrixOutputType,
    &InferredOutputType,
    &DeclaredInputs,
    &InferredInputs,
    &SharedFixtureConstants,
    &MembershipItems,
    &RecordR,
    &QuantifierBinders,
];

/// How a constant receives its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Init {
    /// Declared with its RoboChart literal (`const x : int = 1`).
    Literal,
    /// Declared without a value (`const x : string`). Every constant is a
    /// parameter of RoboTool's `D__` process, so its value still reaches
    /// the model, as a CSP literal.
    Uninitialised,
}

/// A constant of the state machine, and a parameter of `D__`.
#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    pub ty: Type,
    pub value: Value,
    pub init: Init,
    pub origin: &'static str,
}

/// A variable of the state machine, which is not a parameter of `D__`.
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub ty: Type,
    pub origin: &'static str,
}

/// What surrounds a scalar expression, built by [`SCALAR_CONTEXT_RULES`].
#[derive(Debug)]
pub struct ScalarContext {
    /// The row's package, which qualifies enumeration literals in CSP.
    pub package: String,
    /// The row's inputs, as recorded.
    pub inputs: BTreeMap<String, Value>,
    /// The recorded value of the expression.
    pub output_value: Value,
    /// Records and enumerations.
    pub fixtures: Fixtures,
    /// The type of the `output` event, and the rule that declared it.
    pub output: Option<(Type, &'static str)>,
    /// Constants, in name order: the order of `D__`'s parameters.
    pub constants: BTreeMap<String, Constant>,
    pub variables: BTreeMap<String, Variable>,
    /// RoboChart function declarations, in the order declared.
    pub functions: Vec<(String, &'static str)>,
    /// The explicit CSP body of each declared function, by name: a fact for
    /// the FDR preparation.
    pub function_definitions: BTreeMap<String, FunctionDefinition>,
    /// The finite domains of quantifier binders (an input named `domains`):
    /// metadata for the FDR preparation, never a constant.
    pub binder_domains: Option<(Value, &'static str)>,
    /// Every declared name, with the rule that declared it.
    names: BTreeMap<String, &'static str>,
}

impl ScalarContext {
    /// An empty context for `case`, with its inputs and output parsed.
    pub fn new(case: &ExpressionTestCase) -> Result<Self> {
        let package = package_name(&case.id, "expression")?;
        let inputs: BTreeMap<String, Value> = from_json(&case.inputs)?;
        let output: Value = from_json(&case.python_output)?;
        let output_value = output
            .get("value")
            .cloned()
            .ok_or_else(|| Category::InvalidOracle.reject("expression output lacks value"))?;
        Ok(Self {
            fixtures: Fixtures::default().in_package(&package),
            package,
            inputs,
            output_value,
            output: None,
            constants: BTreeMap::new(),
            variables: BTreeMap::new(),
            functions: Vec::new(),
            function_definitions: BTreeMap::new(),
            binder_domains: None,
            names: BTreeMap::new(),
        })
    }

    /// Build the context of `case` with `rules`, and check it.
    pub fn build(case: &ExpressionTestCase, rules: &[&dyn ContextRule]) -> Result<Self> {
        let mut context = Self::new(case)?;
        for rule in rules {
            rule.contribute(case, &mut context)?;
        }
        context.check()?;
        Ok(context)
    }

    /// The rule that declared `name`, if any.
    pub fn origin(&self, name: &str) -> Option<&'static str> {
        self.names.get(name).copied()
    }

    /// Claim `name` for `origin`; a name may be declared once.
    fn claim(&mut self, name: &str, origin: &'static str) -> Result<()> {
        validate_identifier(name)?;
        if let Some(first) = self.names.insert(name.to_owned(), origin) {
            return Err(Category::FixtureGenerationError
                .reject(format!("{name} is declared by both {first} and {origin}")));
        }
        Ok(())
    }

    /// Declare a constant. A string constant is declared without a value
    /// (RoboTool bug B9), so its text must be a plain CSP string.
    pub fn declare_constant(&mut self, name: &str, ty: Type, value: Value, origin: &'static str) -> Result<()> {
        self.claim(name, origin)?;
        let init = if ty == Type::String {
            let plain = value
                .as_str()
                .is_some_and(|text| text.chars().all(|c| c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' '));
            if !plain {
                return Err(Category::FixtureGenerationError.reject("string inputs must be plain printable ASCII"));
            }
            Init::Uninitialised
        } else {
            Init::Literal
        };
        self.constants.insert(name.to_owned(), Constant { ty, value, init, origin });
        Ok(())
    }

    /// Declare a state machine variable.
    pub fn declare_variable(&mut self, name: &str, ty: Type, origin: &'static str) -> Result<()> {
        self.claim(name, origin)?;
        self.variables.insert(name.to_owned(), Variable { ty, origin });
        Ok(())
    }

    /// Declare the records and enumerations of a datatypes column.
    pub fn declare_datatypes(&mut self, datatypes: &str, origin: &'static str) -> Result<()> {
        let fixtures = Fixtures::new(datatypes)?.in_package(&self.package);
        if self.fixtures.names().next().is_some() {
            return Err(Category::FixtureGenerationError.reject(format!("{origin} must declare the first datatypes")));
        }
        for name in fixtures.names().cloned().collect::<Vec<_>>() {
            self.claim(&name, origin)?;
        }
        self.fixtures = fixtures;
        Ok(())
    }

    /// Declare a record type with its fields in order.
    pub fn declare_record(&mut self, name: &str, fields: &[(&str, Type)], origin: &'static str) -> Result<()> {
        self.claim(name, origin)?;
        self.fixtures.declare_record(name, fields)
    }

    /// Declare the type of the `output` event; it is declared once.
    pub fn declare_output(&mut self, ty: Type, origin: &'static str) -> Result<()> {
        if let Some((_, first)) = &self.output {
            return Err(Category::FixtureGenerationError
                .reject(format!("the output type is declared by both {first} and {origin}")));
        }
        self.output = Some((ty, origin));
        Ok(())
    }

    /// The finite domain of each binder that [`QuantifierBinders`] declared,
    /// from the `domains` input, for the binder's type: a fact for the FDR
    /// preparation. A binder whose type the domains do not mention keeps the
    /// whole type.
    pub fn binder_domains(&self) -> Result<Vec<BinderDomain>> {
        let Some((domains, _)) = &self.binder_domains else { return Ok(Vec::new()) };
        let mut binders = Vec::new();
        for (name, variable) in &self.variables {
            if variable.origin != QuantifierBinders.name() {
                continue;
            }
            let ty = variable.ty.to_string();
            let Some(choices) = domains.get(&ty).and_then(Value::as_array) else { continue };
            let values = choices
                .iter()
                .map(|value| Fixtures::default().csp_literal(&ty, value))
                .collect::<Result<_>>()?;
            binders.push(BinderDomain { binder: name.clone(), ty, values });
        }
        Ok(binders)
    }

    /// Reject a context that is incomplete or refers to an undeclared type.
    pub fn check(&self) -> Result<()> {
        let (output, _) = self
            .output
            .as_ref()
            .ok_or_else(|| Category::FixtureGenerationError.reject("no context rule declares the output type"))?;
        self.fixtures.check_type(output, "the output type")?;
        for (name, constant) in &self.constants {
            self.fixtures.check_type(&constant.ty, name)?;
        }
        for (name, variable) in &self.variables {
            self.fixtures.check_type(&variable.ty, name)?;
        }
        Ok(())
    }
}

/// Whether a context column is empty.
fn blank(column: &str) -> bool {
    column.trim().is_empty()
}

/// Inputs that are metadata, not constants: `domains` gives the finite
/// domains of quantifier binders.
fn is_metadata(name: &str) -> bool {
    name == "domains"
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// The records and enumerations of the `datatypes` column (or of the row's
/// shared fixtures).
pub struct DeclaredDatatypes;

impl ContextRule for DeclaredDatatypes {
    fn name(&self) -> &'static str {
        "declared_datatypes"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if blank(&case.datatypes) {
            return Ok(());
        }
        context.declare_datatypes(&case.datatypes, self.name())
    }
}

/// The functions of the `functions` column (or of the row's shared
/// fixtures). Each needs an explicit CSP definition in `function_definitions`.
pub struct DeclaredFunctions;

impl ContextRule for DeclaredFunctions {
    fn name(&self) -> &'static str {
        "declared_functions"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if blank(&case.functions) && blank(&case.function_definitions) {
            return Ok(());
        }
        let definitions = validate_function_fixtures(&case.functions, &case.function_definitions)?;
        for name in definitions.keys() {
            context.claim(name, self.name())?;
        }
        context.functions.push((case.functions.trim().to_owned(), self.name()));
        context.function_definitions = definitions;
        Ok(())
    }
}

/// The type of the `output` event, from the `output_type` column.
pub struct DeclaredOutputType;

impl ContextRule for DeclaredOutputType {
    fn name(&self) -> &'static str {
        "declared_output_type"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if blank(&case.output_type) {
            return Ok(());
        }
        let ty = Type::parse(&case.output_type)?;
        context.declare_output(ty, self.name())
    }
}

/// Features whose output is a vector: `vector(T, n)` from the recorded array.
const VECTOR_FEATURES: &[&str] = &["vector"];
/// Features whose output is a matrix: `matrix(T, rows, columns)`.
const MATRIX_FEATURES: &[&str] = &["matrix", "transpose", "inverse"];

/// A `vector` row without an output type: the output is a vector, not the
/// sequence that its recorded array would otherwise give.
pub struct VectorOutputType;

impl ContextRule for VectorOutputType {
    fn name(&self) -> &'static str {
        "vector_output_type"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if !blank(&case.output_type) || !VECTOR_FEATURES.contains(&case.feature.as_str()) {
            return Ok(());
        }
        let items = context
            .output_value
            .as_array()
            .ok_or_else(|| Category::InvalidFixtureValue.reject("expected vector output array"))?;
        let ty = Type::Vector(Box::new(collection_item_type(items)?), items.len());
        context.declare_output(ty, self.name())
    }
}

/// A `matrix`, `transpose` or `inverse` row without an output type: the
/// output is a rectangular, homogeneous matrix.
pub struct MatrixOutputType;

impl ContextRule for MatrixOutputType {
    fn name(&self) -> &'static str {
        "matrix_output_type"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if !blank(&case.output_type) || !MATRIX_FEATURES.contains(&case.feature.as_str()) {
            return Ok(());
        }
        let ty = matrix_type(&context.output_value)?;
        context.declare_output(ty, self.name())
    }
}

/// Any other row without an output type: the type of the recorded value.
pub struct InferredOutputType;

impl ContextRule for InferredOutputType {
    fn name(&self) -> &'static str {
        "inferred_output_type"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        let feature = case.feature.as_str();
        if !blank(&case.output_type) || VECTOR_FEATURES.contains(&feature) || MATRIX_FEATURES.contains(&feature) {
            return Ok(());
        }
        let ty = infer_type(&context.output_value)?;
        context.declare_output(ty, self.name())
    }
}

/// Inputs with the types of the `types` column, which must type every input.
pub struct DeclaredInputs;

impl ContextRule for DeclaredInputs {
    fn name(&self) -> &'static str {
        "declared_inputs"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if blank(&case.types) {
            return Ok(());
        }
        let types: BTreeMap<String, String> = from_json(&case.types)?;
        let inputs: Vec<_> = context.inputs.iter().filter(|(name, _)| !is_metadata(name)).map(|(n, v)| (n.clone(), v.clone())).collect();
        if inputs.len() != types.len() || types.keys().any(|name| !inputs.iter().any(|(input, _)| input == name)) {
            return Err(Category::FixtureGenerationError.reject("every expression input needs a declared type"));
        }
        for (name, value) in inputs {
            let ty = Type::parse(&types[&name])?;
            context.declare_constant(&name, ty, value, self.name())?;
        }
        Ok(())
    }
}

/// Inputs of a row without a `types` column, typed by their recorded values.
/// An input named `domain` is a set of integers. A string input is declared
/// without a value (RoboTool bug B9).
pub struct InferredInputs;

impl ContextRule for InferredInputs {
    fn name(&self) -> &'static str {
        "inferred_inputs"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if !blank(&case.types) {
            return Ok(());
        }
        let inputs: Vec<_> = context.inputs.iter().filter(|(name, _)| !is_metadata(name)).map(|(n, v)| (n.clone(), v.clone())).collect();
        for (name, value) in inputs {
            validate_identifier(&name)?;
            let ty = if name == "domain" { Type::Set(Box::new(Type::Int)) } else { infer_type(&value)? };
            context.declare_constant(&name, ty, value, self.name())?;
        }
        Ok(())
    }
}

/// The constants of the row's shared fixtures (`shared_fixtures.json`).
pub struct SharedFixtureConstants;

impl ContextRule for SharedFixtureConstants {
    fn name(&self) -> &'static str {
        "shared_fixture_constants"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        for (name, constant) in &case.fixture_constants {
            let ty = Type::parse(&constant.ty)?;
            context.declare_constant(name, ty, constant.value.clone(), self.name())?;
        }
        Ok(())
    }
}

/// `membership` rows test `a in items`: `items` is the set `{0, 1, 2}`.
pub struct MembershipItems;

impl ContextRule for MembershipItems {
    fn name(&self) -> &'static str {
        "membership_items"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if case.feature != "membership" {
            return Ok(());
        }
        let items = serde_json::json!([0, 1, 2]);
        context.declare_constant("items", Type::Set(Box::new(Type::Int)), items, self.name())
    }
}

/// `record` rows construct `R(| x = …, y = … |)`: the record `R` with
/// integer fields `x` and `y`. Recorded record values have type `R`.
pub struct RecordR;

impl ContextRule for RecordR {
    fn name(&self) -> &'static str {
        "record_r"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        if case.feature != "record" {
            return Ok(());
        }
        context.declare_record("R", &[("x", Type::Int), ("y", Type::Int)], self.name())
    }
}

/// Quantifier binders `x: int` and `y: int` in the expression are declared
/// as variables, and an input named `domains` gives their finite domains
/// (metadata for the FDR preparation, not a constant).
pub struct QuantifierBinders;

impl ContextRule for QuantifierBinders {
    fn name(&self) -> &'static str {
        "quantifier_binders"
    }

    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<()> {
        for binder in ["x", "y"] {
            if case.robochart.contains(&format!("{binder}: int")) {
                context.declare_variable(binder, Type::Int, self.name())?;
            }
        }
        if let Some(domains) = context.inputs.get("domains").cloned() {
            validate_quantifier_domains(&domains)?;
            context.binder_domains = Some((domains, self.name()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Type inference for recorded values
// ---------------------------------------------------------------------------

/// The type of a recorded value. A record value has type `R` (see
/// [`RecordR`]); an empty sequence or set holds integers.
pub fn infer_type(value: &Value) -> Result<Type> {
    let tagged = |tag: &str| value.get("type").and_then(Value::as_str) == Some(tag);
    match value {
        Value::Bool(_) => Ok(Type::Boolean),
        Value::Number(number) if number.is_i64() => Ok(Type::Int),
        Value::Number(number) if number.is_f64() => Ok(Type::Real),
        Value::String(_) => Ok(Type::String),
        Value::Array(items) => Ok(Type::Sequence(Box::new(collection_item_type(items)?))),
        Value::Object(fields) if tagged("set") => {
            let items = fields
                .get("items")
                .and_then(Value::as_array)
                .ok_or_else(|| Category::InvalidFixtureValue.reject("set value lacks items"))?;
            Ok(Type::Set(Box::new(collection_item_type(items)?)))
        }
        Value::Object(_) if tagged("record") => Ok(Type::Record("R".into())),
        Value::Object(_) if tagged("interval") => Ok(Type::Set(Box::new(Type::Int))),
        Value::Object(fields) if tagged("tuple") => {
            let items = fields
                .get("items")
                .and_then(Value::as_array)
                .ok_or_else(|| Category::InvalidFixtureValue.reject("tuple value lacks items"))?;
            if items.len() < 2 {
                return Err(Category::UnsupportedRobochartSyntax.reject("RoboChart requires tuples to contain at least two values"));
            }
            Ok(Type::Product(items.iter().map(infer_type).collect::<Result<_>>()?))
        }
        _ => Err(Category::UnsupportedFixtureType.reject(format!("fixture type is not supported yet: {value}"))),
    }
}

/// The common type of a collection's items; an empty collection holds integers.
fn collection_item_type(items: &[Value]) -> Result<Type> {
    let ty = items.first().map(infer_type).transpose()?.unwrap_or(Type::Int);
    for value in items {
        if infer_type(value)? != ty {
            return Err(Category::FixtureGenerationError.reject("collection fixture items must have the same type"));
        }
    }
    Ok(ty)
}

fn matrix_type(value: &Value) -> Result<Type> {
    let rows = value
        .as_array()
        .filter(|rows| !rows.is_empty())
        .ok_or_else(|| Category::InvalidFixtureValue.reject("expected non-empty matrix rows"))?;
    let first = rows[0]
        .as_array()
        .filter(|row| !row.is_empty())
        .ok_or_else(|| Category::InvalidFixtureValue.reject("expected non-empty matrix row"))?;
    let ty = collection_item_type(first)?;
    for row in rows {
        let items = row
            .as_array()
            .ok_or_else(|| Category::InvalidFixtureValue.reject("expected matrix row array"))?;
        if items.len() != first.len() || collection_item_type(items)? != ty {
            return Err(Category::FixtureGenerationError.reject("matrix fixture must be rectangular and homogeneous"));
        }
    }
    Ok(Type::Matrix(Box::new(ty), rows.len(), first.len()))
}

#[cfg(test)]
#[path = "../tests/context.rs"]
mod tests;
