//! The one literal engine: fixture types, and the RoboChart and CSP literal
//! of a value of a declared type. Every value that the generator writes into
//! a model or a check is rendered here.

use crate::rejection::{Category, Rejection, from_json};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Every error of the literal engine is a [`Rejection`] in its own category.
type Result<T> = std::result::Result<T, Rejection>;

/// The datatypes (records and enumerations) of a test case, which give
/// meaning to the record types of its values.
#[derive(Debug, Default)]
pub struct Fixtures {
    records: BTreeMap<String, Vec<Field>>,
    enumerations: BTreeMap<String, Vec<String>>,
    /// Package whose CSP names qualify enumeration literals.
    package: Option<String>,
}

#[derive(Debug)]
struct Field {
    name: String,
    ty: Type,
}

/// A fixture type: a RoboChart type that the literal engine can render values of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Int,
    Nat,
    Boolean,
    Real,
    /// RoboTool cannot compile a string literal (RoboTool bug B9), so a
    /// string constant is declared without a value, which reaches the model
    /// as a CSP string.
    String,
    Set(Box<Type>),
    Product(Vec<Type>),
    Vector(Box<Type>, usize),
    Matrix(Box<Type>, usize, usize),
    Sequence(Box<Type>),
    /// A record or enumeration declared by the test case's datatypes.
    Record(String),
}

impl Type {
    /// Parse a type expression, such as `Seq(int)` or `int * boolean`.
    pub fn parse(source: &str) -> Result<Type> {
        parse_type(source)
    }

    /// The domain expression RoboTool writes for this type in `package`,
    /// with sequences of at most `sequence_bound` items.
    pub fn csp_type(&self, package: &str, sequence_bound: usize) -> String {
        match self {
            Type::Int => "core_int".into(),
            Type::Nat => "core_nat".into(),
            Type::Boolean => "core_boolean".into(),
            Type::Real => "core_real".into(),
            Type::String => "core_string".into(),
            Type::Set(item) => format!("Set({})", item.csp_type(package, sequence_bound)),
            Type::Product(items) => {
                let variables = (0..items.len()).map(|i| format!("tuple_value_{i}")).collect::<Vec<_>>();
                let generators = variables
                    .iter()
                    .zip(items)
                    .map(|(v, t)| format!("{v} <- {}", t.csp_type(package, sequence_bound)))
                    .collect::<Vec<_>>();
                format!("{{ ({}) | {} }}", variables.join(","), generators.join(","))
            }
            Type::Vector(item, size) => format!("vector_{}_{}", item, size),
            Type::Matrix(item, rows, columns) => format!("matrix_{}_{}_{}", item, rows, columns),
            Type::Record(name) => format!("{package}_{name}"),
            Type::Sequence(item) => format!("LSeq({},{sequence_bound})", item.csp_type(package, sequence_bound)),
        }
    }
}

impl Fixtures {
    /// Parse the optional JSON object of named records and ordered fields.
    ///
    /// Each record value is an array of `[field_name, type]` pairs. Keeping
    /// fields in an array preserves the declared order used by CSP tuples.
    /// An enumeration is `{"enumeration": [literal, ...]}` with plain literals.
    pub fn new(datatypes: &str) -> Result<Self> {
        if datatypes.trim().is_empty() {
            return Ok(Self::default());
        }

        let value: Value = from_json(datatypes)?;
        let definitions = value
            .as_object()
            .ok_or_else(|| Category::FixtureGenerationError.reject("datatypes must be a JSON object"))?;
        let mut records = BTreeMap::new();
        let mut enumerations = BTreeMap::new();

        for (record_name, definition) in definitions {
            validate_identifier(record_name)?;
            if is_builtin_name(record_name) {
                return Err(Category::FixtureGenerationError.reject(format!(
                    "record name conflicts with a built-in type: {record_name}"
                )));
            }
            if let Some(object) = definition.as_object() {
                let literals = object
                    .get("enumeration")
                    .filter(|_| object.len() == 1)
                    .and_then(Value::as_array)
                    .filter(|literals| !literals.is_empty())
                    .ok_or_else(|| {
                        Category::FixtureGenerationError
                            .reject(format!("{record_name} must be a record or a non-empty enumeration"))
                    })?;
                let mut names = Vec::with_capacity(literals.len());
                for literal in literals {
                    let literal = literal
                        .as_str()
                        .ok_or_else(|| {
                            Category::FixtureGenerationError
                                .reject(format!("literal of {record_name} must be a string"))
                        })?;
                    validate_identifier(literal)?;
                    if names.iter().any(|name| name == literal) {
                        return Err(Category::FixtureGenerationError
                            .reject(format!("duplicate literal {literal} in {record_name}")));
                    }
                    names.push(literal.to_owned());
                }
                enumerations.insert(record_name.clone(), names);
                continue;
            }
            let pairs = definition
                .as_array()
                .ok_or_else(|| {
                    Category::FixtureGenerationError
                        .reject(format!("fields for {record_name} must be an ordered array"))
                })?;
            if pairs.is_empty() {
                return Err(Category::FixtureGenerationError
                    .reject(format!("record {record_name} must declare at least one field")));
            }

            let mut fields = Vec::with_capacity(pairs.len());
            let mut field_names = BTreeSet::new();
            for pair in pairs {
                let pair = pair
                    .as_array()
                    .filter(|pair| pair.len() == 2)
                    .ok_or_else(|| {
                        Category::FixtureGenerationError
                            .reject(format!("each field of {record_name} must be [name, type]"))
                    })?;
                let field_name = pair[0]
                    .as_str()
                    .ok_or_else(|| {
                        Category::FixtureGenerationError
                            .reject(format!("field name in {record_name} must be a string"))
                    })?;
                validate_identifier(field_name)?;
                if !field_names.insert(field_name.to_owned()) {
                    return Err(Category::FixtureGenerationError
                        .reject(format!("duplicate field {field_name} in {record_name}")));
                }
                let type_text = pair[1].as_str().ok_or_else(|| {
                    Category::FixtureGenerationError
                        .reject(format!("type of {record_name}.{field_name} must be a string"))
                })?;
                let ty = parse_type(type_text)?;
                fields.push(Field {
                    name: field_name.to_owned(),
                    ty,
                });
            }
            records.insert(record_name.clone(), fields);
        }

        let fixtures = Self { records, enumerations, package: None };
        for (record_name, fields) in &fixtures.records {
            for field in fields {
                fixtures.validate_references(&field.ty, &format!("{record_name}.{}", field.name))?;
            }
        }
        reject_recursive_records(&fixtures.records)?;

        Ok(fixtures)
    }

    /// Qualify enumeration literals in CSP with the case package's names.
    pub fn in_package(mut self, package: &str) -> Self {
        self.package = Some(package.to_owned());
        self
    }

    /// Emit named record declarations in deterministic name order.
    pub fn declarations(&self) -> String {
        self.declaration_list()
            .into_iter()
            .map(|(_, declaration)| declaration + "\n")
            .collect()
    }

    /// Each declaration with its name: the records in name order, then the
    /// enumerations in name order.
    pub fn declaration_list(&self) -> Vec<(String, String)> {
        self.records
            .iter()
            .map(|(name, fields)| {
                let fields = fields
                    .iter()
                    .map(|field| format!("{} : {}", field.name, field.ty))
                    .collect::<Vec<_>>()
                    .join(" ");
                (name.clone(), format!("datatype {name} {{ {fields} }}"))
            })
            .chain(self.enumerations.iter().map(|(name, literals)| {
                (name.clone(), format!("enumeration {name} {{ {} }}", literals.join(" ")))
            }))
            .collect()
    }

    /// The names of the declared records and enumerations.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.records.keys().chain(self.enumerations.keys())
    }

    /// Declare one more record, with its fields in order. A name that is
    /// already declared, or a type it cannot resolve, is rejected.
    pub fn declare_record(&mut self, name: &str, fields: &[(&str, Type)]) -> Result<()> {
        validate_identifier(name)?;
        if is_builtin_name(name) || self.records.contains_key(name) || self.enumerations.contains_key(name) {
            return Err(Category::FixtureGenerationError.reject(format!("datatype {name} is declared twice")));
        }
        let fields: Vec<Field> = fields.iter().map(|(field, ty)| Field { name: (*field).to_owned(), ty: ty.clone() }).collect();
        for field in &fields {
            validate_identifier(&field.name)?;
            self.check_type(&field.ty, &format!("{name}.{}", field.name))?;
        }
        self.records.insert(name.to_owned(), fields);
        reject_recursive_records(&self.records)
    }

    /// Render a value of a resolved type as a RoboChart expression.
    pub fn robo_value(&self, ty: &Type, value: &Value) -> Result<String> {
        self.check_type(ty, "type")?;
        self.literal(ty, value, LiteralStyle::RoboChart)
    }

    /// Render a value of a resolved type as a CSP literal.
    pub fn csp_value(&self, ty: &Type, value: &Value) -> Result<String> {
        self.check_type(ty, "type")?;
        self.literal(ty, value, LiteralStyle::Csp)
    }

    /// Check that every record type that `ty` names is declared; `context`
    /// says where `ty` is used.
    pub fn check_type(&self, ty: &Type, context: &str) -> Result<()> {
        self.validate_references(ty, context)
    }

    /// Parse a type expression and check it against these declarations.
    pub fn resolve(&self, ty: &str) -> Result<Type> {
        self.checked_type(ty)
    }

    /// Render a typed value as a RoboChart expression.
    pub fn robo_literal(&self, ty: &str, value: &Value) -> Result<String> {
        let ty = self.checked_type(ty)?;
        self.literal(&ty, value, LiteralStyle::RoboChart)
    }

    /// Render a typed value as a CSP literal, using record declaration order.
    pub fn csp_literal(&self, ty: &str, value: &Value) -> Result<String> {
        let ty = self.checked_type(ty)?;
        self.literal(&ty, value, LiteralStyle::Csp)
    }

    /// Parse and validate a type expression against these declarations.
    pub fn validate_type(&self, ty: &str) -> Result<()> {
        self.checked_type(ty).map(|_| ())
    }

    /// Domain expression emitted by RoboTool for this fixture type.
    pub fn csp_type(&self, ty: &str, package: &str, sequence_bound: usize) -> Result<String> {
        Ok(self.checked_type(ty)?.csp_type(package, sequence_bound))
    }

    fn checked_type(&self, ty: &str) -> Result<Type> {
        let ty = parse_type(ty)?;
        self.validate_references(&ty, "type")?;
        Ok(ty)
    }

    fn validate_references(&self, ty: &Type, context: &str) -> Result<()> {
        match ty {
            Type::Sequence(item) | Type::Set(item) | Type::Vector(item, _) | Type::Matrix(item, _, _) => self.validate_references(item, context),
            Type::Product(items) => items.iter().try_for_each(|item| self.validate_references(item, context)),
            Type::Record(name) if !self.records.contains_key(name) && !self.enumerations.contains_key(name) => {
                Err(Category::UnsupportedFixtureType.reject(format!("unknown record type {name} in {context}")))
            }
            Type::Int | Type::Nat | Type::Boolean | Type::Real | Type::String | Type::Record(_) => Ok(()),
        }
    }

    fn literal(&self, ty: &Type, value: &Value, style: LiteralStyle) -> Result<String> {
        match ty {
            Type::Int => integer_literal(value, false),
            Type::Nat => integer_literal(value, true),
            Type::Real => match style {
                LiteralStyle::RoboChart => value.as_f64().filter(|v| v.is_finite()).map(|_| value.to_string()).ok_or_else(|| Category::InvalidFixtureValue.reject("expected finite real value")),
                LiteralStyle::Csp => exact_real_literal(value),
            },
            // The JSON string literal, which for plain printable ASCII is the
            // text in double quotes.
            Type::String => value
                .as_str()
                .map(|_| value.to_string())
                .ok_or_else(|| Category::InvalidFixtureValue.reject(format!("expected string value, got {value}"))),
            Type::Set(item_ty) if value.get("type").and_then(Value::as_str) == Some("interval") => {
                interval_literal(item_ty, value, style)
            }
            Type::Set(item_ty) => {
                let items = value.as_array().or_else(|| value.get("items").and_then(Value::as_array)).ok_or_else(|| Category::InvalidFixtureValue.reject("expected set items"))?;
                let items = items.iter().map(|v| self.literal(item_ty, v, style)).collect::<Result<Vec<_>>>()?;
                Ok(format!("{{ {} }}", items.join(",")))
            }
            Type::Product(types) => {
                let items = value.as_array().or_else(|| value.get("items").and_then(Value::as_array)).ok_or_else(|| Category::InvalidFixtureValue.reject("expected tuple items"))?;
                if items.len() != types.len() { return Err(Category::FixtureGenerationError.reject("tuple arity does not match its type")); }
                let items = types.iter().zip(items).map(|(t, v)| self.literal(t, v, style)).collect::<Result<Vec<_>>>()?.join(",");
                Ok(match style { LiteralStyle::RoboChart => format!("(| {items} |)"), LiteralStyle::Csp => format!("({items})") })
            }
            Type::Vector(item_ty, size) => {
                let items = value.as_array().ok_or_else(|| Category::InvalidFixtureValue.reject("expected vector array"))?;
                if items.len() != *size { return Err(Category::FixtureGenerationError.reject("vector size does not match its type")); }
                let items = items.iter().map(|v| self.literal(item_ty, v, style)).collect::<Result<Vec<_>>>()?.join(",");
                Ok(match style { LiteralStyle::RoboChart => format!("[|{items}|]"), LiteralStyle::Csp => format!("({items})") })
            }
            Type::Matrix(item_ty, rows, columns) => {
                let values = value.as_array().ok_or_else(|| Category::InvalidFixtureValue.reject("expected matrix rows"))?;
                if values.len() != *rows { return Err(Category::FixtureGenerationError.reject("matrix row count does not match its type")); }
                let mut rendered = Vec::new();
                for row in values {
                    let items = row.as_array().ok_or_else(|| Category::InvalidFixtureValue.reject("expected matrix row array"))?;
                    if items.len() != *columns { return Err(Category::FixtureGenerationError.reject("matrix column count does not match its type")); }
                    let items = items.iter().map(|v| self.literal(item_ty, v, style)).collect::<Result<Vec<_>>>()?.join(",");
                    rendered.push(match style { LiteralStyle::RoboChart => items, LiteralStyle::Csp => format!("({items})") });
                }
                Ok(match style { LiteralStyle::RoboChart => format!("[|{}|]", rendered.join(";")), LiteralStyle::Csp => rendered.join(".") })
            }
            Type::Boolean => value
                .as_bool()
                .map(|boolean| boolean.to_string())
                .ok_or_else(|| Category::InvalidFixtureValue.reject(format!("expected boolean value, got {value}"))),
            Type::Sequence(item_ty) => {
                let items = value
                    .as_array()
                    .ok_or_else(|| Category::InvalidFixtureValue.reject(format!("expected sequence array, got {value}")))?;
                let items = items
                    .iter()
                    .map(|item| self.literal(item_ty, item, style))
                    .collect::<Result<Vec<_>>>()?;
                let separator = match style {
                    LiteralStyle::RoboChart => ", ",
                    LiteralStyle::Csp => ",",
                };
                Ok(sequence_literal(&items.join(separator)))
            }
            Type::Record(name) if self.enumerations.contains_key(name) => {
                let literal = value
                    .as_str()
                    .filter(|literal| self.enumerations[name].iter().any(|l| l == literal))
                    .ok_or_else(|| {
                        Category::InvalidFixtureValue.reject(format!("expected a literal of {name}, got {value}"))
                    })?;
                match style {
                    LiteralStyle::RoboChart => Ok(format!("{name}::{literal}")),
                    LiteralStyle::Csp => {
                        let package = self
                            .package
                            .as_deref()
                            .ok_or_else(|| {
                                Category::FixtureGenerationError
                                    .reject("enumeration CSP literals need the case package")
                            })?;
                        Ok(format!("{package}_{name}_{literal}"))
                    }
                }
            }
            Type::Record(name) => {
                let fields = self
                    .records
                    .get(name)
                    .ok_or_else(|| Category::UnsupportedFixtureType.reject(format!("unknown record type {name}")))?;
                // A recorded record value is `{"type": "record", "fields": {...}}`.
                let value = match value.as_object() {
                    Some(object) if object.len() == 2 && object.get("type").and_then(Value::as_str) == Some("record") => {
                        object.get("fields").ok_or_else(|| Category::InvalidFixtureValue.reject("record value lacks fields"))?
                    }
                    _ => value,
                };
                let object = value
                    .as_object()
                    .ok_or_else(|| {
                        Category::InvalidFixtureValue.reject(format!("expected object for record {name}, got {value}"))
                    })?;
                for key in object.keys() {
                    if !fields.iter().any(|field| field.name == *key) {
                        return Err(Category::InvalidFixtureValue.reject(format!("extra field {key} in value of {name}")));
                    }
                }
                let mut rendered = Vec::with_capacity(fields.len());
                for field in fields {
                    let field_value = object.get(&field.name).ok_or_else(|| {
                        Category::InvalidFixtureValue.reject(format!("missing field {} in value of {name}", field.name))
                    })?;
                    rendered.push(self.literal(&field.ty, field_value, style)?);
                }
                match style {
                    LiteralStyle::RoboChart => Ok(format!(
                        "{name}(| {} |)",
                        fields
                            .iter()
                            .zip(rendered)
                            .map(|(field, value)| format!("{} = {value}", field.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    // RoboTool represents a one-field record with (value),
                    // which CSP treats as the value itself.
                    LiteralStyle::Csp => Ok(format!("({})", rendered.join(","))),
                }
            }
        }
    }
}

/// An interval value, `{"type": "interval", "start": a, "end": b,
/// "left_closed": …, "right_closed": …}`, as a CSP set of integers. RoboChart
/// has no interval literal.
fn interval_literal(item_ty: &Type, value: &Value, style: LiteralStyle) -> Result<String> {
    if value.get("end").is_none_or(Value::is_null) {
        return Err(Category::UnsupportedRobochartSyntax.reject("RoboChart interval syntax requires an upper bound"));
    }
    let domain = match item_ty {
        Type::Int => "core_int",
        Type::Nat => "core_nat",
        _ => return Err(Category::InvalidFixtureValue.reject("an interval value needs an integer set type")),
    };
    if let LiteralStyle::RoboChart = style {
        return Err(Category::UnsupportedFixtureType.reject("RoboChart has no interval literal"));
    }
    let integer = |key: &str| {
        value.get(key).and_then(Value::as_i64).ok_or_else(|| Category::InvalidFixtureValue.reject(format!("interval {key} must be an integer")))
    };
    let closed = |key: &str| {
        value.get(key).and_then(Value::as_bool).ok_or_else(|| Category::InvalidFixtureValue.reject(format!("interval lacks {key}")))
    };
    let (start, end) = (integer("start")?, integer("end")?);
    let left = if closed("left_closed")? { "<=" } else { "<" };
    let right = if closed("right_closed")? { "<=" } else { "<" };
    Ok(format!("{{ interval_value | interval_value <- {domain}, {start} {left} interval_value, interval_value {right} {end} }}"))
}

#[derive(Clone, Copy)]
enum LiteralStyle {
    RoboChart,
    Csp,
}

/// Every error in a type expression: a type that the encoders do not support.
fn type_error(reason: impl Into<String>) -> Rejection {
    Category::UnsupportedFixtureType.reject(reason)
}

/// Both RoboChart and CSPm lex `<-` as one token, so a sequence whose first
/// element is negative needs a space after `<`.
pub fn sequence_literal(items: &str) -> String {
    if items.starts_with('-') {
        format!("< {items}>")
    } else {
        format!("<{items}>")
    }
}

/// CSP's current `core_real` consists of integers. Preserve exact integral
/// observations, and reject fractions instead of emitting event separators.
pub fn exact_real_literal(value: &Value) -> Result<String> {
    if let Some(n) = value.as_i64() { return Ok(n.to_string()); }
    let n = value.as_f64().ok_or_else(|| Category::InvalidFixtureValue.reject("expected numeric real value"))?;
    if n.is_finite() && n.fract() == 0.0 && n >= i64::MIN as f64 && n < -(i64::MIN as f64) {
        Ok((n as i64).to_string())
    } else {
        Err(Category::OracleRepresentationLimit
            .reject(format!("oracle cannot represent fractional real observation {value} exactly")))
    }
}

fn integer_literal(value: &Value, nonnegative: bool) -> Result<String> {
    let integer = value
        .as_i64()
        .ok_or_else(|| Category::InvalidFixtureValue.reject(format!("expected signed 64-bit integer, got {value}")))?;
    if nonnegative && integer < 0 {
        return Err(Category::InvalidFixtureValue.reject(format!("expected non-negative integer, got {integer}")));
    }
    Ok(integer.to_string())
}

fn validate_identifier(identifier: &str) -> Result<()> {
    if identifier.is_empty()
        || !identifier
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        || !identifier
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(Category::InvalidIdentifier.reject(format!("invalid identifier: {identifier}")));
    }
    Ok(())
}

fn is_builtin_name(name: &str) -> bool {
    matches!(name, "int" | "nat" | "boolean" | "real" | "string" | "Seq" | "Set")
}

fn reject_recursive_records(records: &BTreeMap<String, Vec<Field>>) -> Result<()> {
    fn visit(
        name: &str,
        records: &BTreeMap<String, Vec<Field>>,
        active: &mut BTreeSet<String>,
        complete: &mut BTreeSet<String>,
    ) -> Result<()> {
        if complete.contains(name) {
            return Ok(());
        }
        if !active.insert(name.to_owned()) {
            return Err(Category::UnsupportedRecursion.reject(format!("recursive record definition involving {name}")));
        }
        // Enumerations have no fields and cannot recurse.
        let Some(fields) = records.get(name) else { return Ok(()) };
        for field in fields {
            for dependency in record_dependencies(&field.ty) {
                visit(&dependency, records, active, complete)?;
            }
        }
        active.remove(name);
        complete.insert(name.to_owned());
        Ok(())
    }

    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    for name in records.keys() {
        visit(name, records, &mut active, &mut complete)?;
    }
    Ok(())
}

fn record_dependencies(ty: &Type) -> Vec<String> {
    match ty {
        Type::Sequence(item) | Type::Set(item) | Type::Vector(item, _) | Type::Matrix(item, _, _) => record_dependencies(item),
        Type::Product(items) => items.iter().flat_map(record_dependencies).collect(),
        Type::Record(name) => vec![name.clone()],
        Type::Int | Type::Nat | Type::Boolean | Type::Real | Type::String => Vec::new(),
    }
}

fn parse_type(source: &str) -> Result<Type> {
    let mut parser = TypeParser {
        chars: source.chars().collect(),
        position: 0,
    };
    let ty = parser.parse()?;
    parser.skip_whitespace();
    if parser.position != parser.chars.len() {
        return Err(Category::UnsupportedFixtureType.reject(format!("unexpected text in type expression: {source}")));
    }
    Ok(ty)
}

struct TypeParser {
    chars: Vec<char>,
    position: usize,
}

impl TypeParser {
    fn parse(&mut self) -> Result<Type> {
        let first = self.atom()?;
        let mut items = vec![first];
        self.skip_whitespace();
        while self.chars.get(self.position) == Some(&'*') {
            self.position += 1;
            items.push(self.atom()?);
            self.skip_whitespace();
        }
        Ok(if items.len() == 1 { items.pop().unwrap() } else { Type::Product(items) })
    }

    fn dimension(&mut self) -> Result<usize> {
        self.skip_whitespace();
        let start = self.position;
        while self.chars.get(self.position).is_some_and(|c| c.is_ascii_digit()) { self.position += 1; }
        let size: usize = self.chars[start..self.position]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|error: std::num::ParseIntError| type_error(format!("invalid dimension in type expression: {error}")))?;
        if size == 0 { return Err(type_error("vector and matrix dimensions must be positive")); }
        Ok(size)
    }

    fn atom(&mut self) -> Result<Type> {
        self.skip_whitespace();
        if self.chars.get(self.position) == Some(&'(') {
            self.position += 1;
            let ty = self.parse()?;
            self.skip_whitespace(); self.expect(')')?;
            return Ok(ty);
        }
        let name = self.identifier()?;
        match name.as_str() {
            "int" => Ok(Type::Int),
            "nat" => Ok(Type::Nat),
            "boolean" => Ok(Type::Boolean),
            "real" => Ok(Type::Real),
            "string" => Ok(Type::String),
            "vector" | "matrix" => {
                self.skip_whitespace(); self.expect('(')?;
                let item = self.parse()?;
                self.skip_whitespace(); self.expect(',')?;
                let size = self.dimension()?;
                let ty = if name == "matrix" {
                    self.skip_whitespace(); self.expect(',')?;
                    Type::Matrix(Box::new(item), size, self.dimension()?)
                } else { Type::Vector(Box::new(item), size) };
                self.skip_whitespace(); self.expect(')')?;
                Ok(ty)
            }
            "Seq" | "Set" => {
                self.skip_whitespace();
                self.expect('(')?;
                let item = self.parse()?;
                self.skip_whitespace();
                self.expect(')')?;
                Ok(if name == "Seq" { Type::Sequence(Box::new(item)) } else { Type::Set(Box::new(item)) })
            }
            _ => Ok(Type::Record(name)),
        }
    }

    fn identifier(&mut self) -> Result<String> {
        let start = self.position;
        let Some(first) = self.chars.get(self.position).copied() else {
            return Err(type_error("expected a type name"));
        };
        if !first.is_ascii_alphabetic() {
            return Err(type_error("type names must begin with an ASCII letter"));
        }
        self.position += 1;
        while self
            .chars
            .get(self.position)
            .is_some_and(|character| character.is_ascii_alphanumeric() || *character == '_')
        {
            self.position += 1;
        }
        Ok(self.chars[start..self.position].iter().collect())
    }

    fn expect(&mut self, expected: char) -> Result<()> {
        self.skip_whitespace();
        if self.chars.get(self.position) == Some(&expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(type_error(format!("expected '{expected}' in type expression")))
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .chars
            .get(self.position)
            .is_some_and(|character| character.is_whitespace())
        {
            self.position += 1;
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int => formatter.write_str("int"),
            Type::Nat => formatter.write_str("nat"),
            Type::Boolean => formatter.write_str("boolean"),
            Type::Real => formatter.write_str("real"),
            Type::String => formatter.write_str("string"),
            Type::Set(item) => write!(formatter, "Set({item})"),
            // A product inside a product needs parentheses; elsewhere it does not.
            Type::Product(items) => {
                let items = items
                    .iter()
                    .map(|item| match item {
                        Type::Product(_) => format!("({item})"),
                        _ => item.to_string(),
                    })
                    .collect::<Vec<_>>();
                formatter.write_str(&items.join(" * "))
            }
            Type::Vector(item, size) => write!(formatter, "vector({item},{size})"),
            Type::Matrix(item, rows, columns) => write!(formatter, "matrix({item},{rows},{columns})"),
            Type::Sequence(item) => write!(formatter, "Seq({item})"),
            Type::Record(name) => formatter.write_str(name),
        }
    }
}

#[cfg(test)]
#[path = "tests/fixtures.rs"]
mod tests;
