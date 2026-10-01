# Architecture

The testsuite generator turns recorded RoboChart-to-Python translator
observations into RoboChart models and CSP checks. RoboTool and FDR then judge
each check. This page describes how the code is organised and, above all, where
the rules that decide each case's context live.

## Repositories

The work is split across three repositories:

| Repository | Contents |
| --- | --- |
| [robochart-testsuite-generator](https://github.com/twright/robochart-testsuite-generator) (this one) | The three crates below, the diagram plug-in, and the documentation of the input format and of the oracle's defects. It contains no particular testsuite. |
| [robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite) | The ExpressionTests input (`data/`), the testsuite project generated from it with its diagrams and verification results, and a script that regenerates and rechecks it. |
| [robochart-testsuite-mutation](https://github.com/twright/robochart-testsuite-mutation) | The `mutate` tool, which depends on `testsuite-generator` through a Cargo git dependency and runs the two binaries (found on `PATH` or given as options), and the source-mutation campaign's evidence and claims. |

Tests in this repository check properties of the code. They use synthetic
rows (`test_rows`), small literal rows, or the few input rows copied into
`crates/testsuite-runner/src/tests/data/`; none reads a particular testsuite.

## Crates

```
Cargo.toml                          workspace
crates/testsuite-generator/         lib testsuite_generator + bin testsuite-generator
crates/robotool-harness/            lib
crates/testsuite-runner/            lib testsuite_runner + bin testsuite-runner
diagram-plugin/                     org.robocalc.testsuitegenerator.diagrams
```

Each crate keeps its self-tests in `src/tests/` (child modules of the code they
test, declared with `#[path]`, and `testsuite-runner`'s `pipeline_properties`
integration test, registered with `[[test]]`). Proptest regression files are in
`proptest-regressions/tests/`, parallel to those sources.

| Crate | Owns | Must not |
| --- | --- | --- |
| `testsuite-generator` | Reading case rows and shared fixtures, deciding each case's context, rendering `.rct` models and CSP checks, writing a review project | Run subprocesses or depend on RoboTool/FDR code |
| `robotool-harness` | Running headless RoboTool, running FDR with a timeout, rewriting RoboTool's generated CSP from a typed `FdrPreparation` | Read case CSVs or know about any testsuite |
| `testsuite-runner` | Running the oracle checks (RoboTool and FDR, not the translator under test): `compile`, `prepare-fdr`, `verify`, `report-markdown`; wider-bounds recheck policy; failure attribution to reproduced RoboTool defects (`docs/robotool-bugs.md`) | Reinterpret case rows itself; it asks the generator |

Dependencies point one way: `testsuite-runner` uses `testsuite-generator` and
`robotool-harness`. The generator and the harness depend on neither. Outside
this repository, `mutate` uses the `testsuite-generator` library and runs both
binaries as subprocesses.

Neither binary has built-in knowledge of where a testsuite lives. The SOURCE
of `testsuite-generator` is always given, and PROJECT defaults to
`testsuite-project` in the current directory for both binaries:

```sh
cargo run -p testsuite-generator -- /path/to/input testsuite-project
cargo run -p testsuite-runner -- verify testsuite-project
```

## Where the generation rules live

A reviewer who wants to know *what surrounds a case* reads these places, in
this order.

### 1. Case kinds (`kind.rs`)

```rust
pub trait TestCaseKind {
    const KIND: &'static str;            // "expression" | "statement"
    type Row: DeserializeOwned;
    type Plan;                           // TestCasePlan for both kinds
    fn test_case_id(row: &Self::Row) -> TestCaseId;
    fn plan(row: &Self::Row, shared: &SharedFixtures) -> Result<Self::Plan, Rejection>;
    fn render(id: &TestCaseId, plan: &Self::Plan) -> GeneratedTestCase;
}
```

Every kind separates **planning** (decisions, as data) from **rendering**
(templates, no decisions). A `TestCasePlan` holds the model as a `Machine`
(section 6), what the CSP check needs, and the facts that the FDR preparation
needs (section 7); rendering cannot fail. One generic
pipeline (`Generation::run`) generates all kinds. A `Rejection` carries its
failure category from the place it is raised (`rejection.rs`), so the
category in `generation-failures.csv` is never inferred from message text;
a JSON column that does not parse is categorised by serde_json's own
classification of the error. One rule depends on the row rather than the
site: a row of feature `record_empty` is reported as unsupported syntax
(`feature_rejection`).

### 2. Expression shapes (`expression/shapes.rs`)

Shared fixtures are merged into a row first (`apply_shared_fixtures` in
`expression/mod.rs`): they fill the row's context columns, which must be
empty. The shape is then chosen by an ordered table; the first rule that
applies wins, and the list order is the precedence:

```rust
pub const SHAPE_RULES: &[ShapeRule] = &[ /* timed_clock, since_without_timing,
    unsupported_feature, lambda_graph, scalar */ ];
```

Timed and lambda shapes are single, self-contained rules (`timed.rs`,
`lambda.rs`). Typed and untyped rows have the one `scalar` shape
(`scalar.rs`), whose context comes from section 3.

### 3. Scalar context (`expression/context.rs`)

The context of a scalar expression is built by an ordered list of independent
contributors. Each one is a named, documented `impl ContextRule`:

```rust
pub trait ContextRule {
    fn name(&self) -> &'static str;
    fn contribute(&self, case: &ExpressionTestCase, context: &mut ScalarContext) -> Result<(), Rejection>;
}
pub const SCALAR_CONTEXT_RULES: &[&dyn ContextRule] = &[
    &DeclaredDatatypes, &DeclaredFunctions, &DeclaredOutputType, &VectorOutputType,
    &MatrixOutputType, &InferredOutputType, &DeclaredInputs, &InferredInputs,
    &SharedFixtureConstants, &MembershipItems, &RecordR, &QuantifierBinders,
];
```

`SCALAR_CONTEXT_RULES` is the complete list of what can surround a scalar
expression, including the rules that depend on a row's feature name:

| Rule | Applies to | Declares |
| --- | --- | --- |
| `declared_datatypes`, `declared_functions` | rows with `datatypes`, `functions` | records, enumerations and functions (with their CSP definitions checked) |
| `declared_output_type` | rows with `output_type` | the type of `output` |
| `vector_output_type`, `matrix_output_type` | features `vector`; `matrix`, `transpose`, `inverse` (without `output_type`) | `vector(T, n)` or `matrix(T, r, c)` from the recorded value |
| `inferred_output_type` | other rows without `output_type` | the type of the recorded value |
| `declared_inputs` | rows with `types` | each input, with its declared type |
| `inferred_inputs` | rows without `types` | each input, with the type of its value; `domain` is `Set(int)`; a string is declared without a value, which reaches `D__` as a CSP string (RoboTool bug B9) |
| `shared_fixture_constants` | rows with shared fixtures | the fixtures' constants |
| `membership_items` | feature `membership` | `items : Set(int) = { 0,1,2 }` |
| `record_r` | feature `record` | `datatype R { x : int y : int }` (a recorded record value has type `R`) |
| `quantifier_binders` | expressions with `x: int` or `y: int`; inputs named `domains` | the binders as variables; the binder domains as metadata, never a constant |

Declaring the same name twice is a rejection that names both rules, and
`ScalarContext::check()` rejects an output, constant or variable whose type
names an undeclared record, so dependencies between contributors cannot
fail silently. Every constant is typed (`name`, `fixtures::Type`, value,
`Init`, origin); constants are declared in name order, the order of `D__`'s
parameters. The origin (the rule's name) is written as a comment in the
generated `.rct` file, so a reviewer of a generated project can see which
rule produced each declaration:

```
    stm stm0 {
      // context rule: inferred_inputs
      const a : int = -1
      // context rule: membership_items
      const items : Set(int) = { 0,1,2 }
      // context rule: inferred_output_type
      event output : boolean
```

### 4. Statement plans (`statement/plan.rs`)

Statement rows have no implicit rules; every part of their context comes from a
column. `statement::plan` is a short sequence of staged functions whose
parameters state their dependencies:

```rust
let fixtures       = datatypes(row)?;
let domains        = validate_value_domains(row, &fixtures, &package)?;
let variables      = variables(row, &columns, &fixtures)?;
let observation    = observation(&columns, &fixtures)?;
let events         = events(row, &fixtures)?;
let expected_trace = expected_trace(&columns.output, &events, &fixtures, &package)?;
let driver         = driver(row, &events, &variables, &columns.types, &fixtures, &package)?;
let body           = body(row, &columns.types)?;
```

### 5. Literals and types (`fixtures.rs`)

One literal engine renders every value as RoboChart and as CSP, for a declared
`fixtures::Type`: integers, naturals, Booleans, reals (integral in CSP),
strings, sets (including recorded intervals, as CSP sets), tuples, vectors,
matrices, sequences, records and enumerations. There is no second, untyped
literal path; untyped rows reach the engine through the types that
`inferred_inputs` and `inferred_output_type` give them.

### 6. Rendering (`render.rs`)

All model shapes render through one `Machine` description (module, platform,
controller, state machine, connections, declarations), and
`Machine::render` takes no decisions. CSP checks remain per shape.

### 7. FDR preparation facts (`facts.rs`)

RoboTool's CSP needs finite bounds, function instantiations and domain
restrictions before FDR can check it. The generator states what these depend
on as facts about a project's `source-data/` snapshot (`facts::Snapshot`,
the rows of `cases.csv` and `statement_cases.csv` with the shared fixtures):

- `extents(snapshot)` scans every row, including rows that fail generation,
  because the global bounds have always covered them. It gives the integer
  range of every value and every integer literal in the source text (at
  least `{-2..2}`), the longest sequence value or explicit `sequence_length`
  (at least 2; driver scripts and event traces count only their payloads),
  the timed clock bound (above every `max_ticks`, at least 10), and the
  integer constants that have one value over all rows.
- `csp_facts(snapshot)` plans each row and, for each row that can be
  generated, gives the `CspFacts` that its plan recorded (`TestCasePlan::facts`):
  the CSP body of each fixture function (`declared_functions`, a statement's
  `function_definitions`, or the shared fixtures), a statement's explicit
  value domains for record and sequence types, and the finite domain of each
  binder that `quantifier_binders` declared. Nothing scans the expression text
  for binders: the context rule's binders are the ones restricted.

`testsuite-runner` turns the facts into a `robotool_harness::FdrPreparation`:

```rust
pub struct FdrPreparation {
    pub core_int: (i64, i64),                    // extents' integer range
    pub core_nat_max: i64,                       // and the longest sequence
    pub sequence_bound: usize,                   // every LSeq type
    pub sizing_constants: BTreeMap<String, i64>, // for the bound computation
    pub clock_max: u32,                          // core_clock_type
    pub functions: Vec<FunctionInstance>,        // package, name, parameters, body
    pub value_domains: Vec<ValueDomain>,         // package, Datatype | Sequence, CSP values
    pub binder_domains: Vec<BinderDomain>,       // package, binder, type, CSP values
}
```

`robotool_harness::prepare(project, &preparation, refines)` applies it to
`csp-gen/` and reads no case data. `prepare-fdr` builds one preparation from
the whole snapshot; `verify` builds each case's from the rows with its ID
(`Snapshot::only`), as if its project held only them, and uses the same
clock bound for the wider-bounds recheck.

## Verification outputs

`testsuite-runner verify` checks each test case in its own temporary project
and writes `verification-results.csv`, `verification-results.md` and
`verification-logs/` into the project. Log paths that depend on the machine
are replaced by `<test-case-project>` (the temporary project),
`<ROBOTOOL_HOME>` and `<testsuite-project>`. `verify --jobs N` checks N
test cases at once; the log names are fixed before any check runs and rows
are written in index order, so the results do not depend on N. RoboTool's
directory is `--robotool-home` or `ROBOTOOL_HOME`; FDR's `refines` is `--fdr-refines` or
`FDR_REFINES`, by default `refines` on `PATH`, and is passed explicitly to the
harness functions that run FDR.

## How this layout was reached

The code was reorganised from a single repository in three stages, each with a
mechanical acceptance check against the ExpressionTests suite:

1. **Restructure without changing behaviour:** the crates, the two binaries,
   `TestCaseKind`, the shape table, rejection categories set where they are
   raised, the single `verify` command and the plug-in rename. Regenerating
   the suite's project was byte-identical.
2. **Unify scalar context:** context rules, one literal engine, the `Machine`
   renderer, staged statement plans, and deliberate category corrections
   (JSON errors by serde_json's classification, malformed type expressions,
   `project_setup_error`). Every one of the 576 rows kept its `verify`
   status, category and reason; the models changed only by origin comments,
   layout and literal formatting.
3. **Move preparation rules into the generator:** `facts.rs` and
   `FdrPreparation`. `prepare-fdr` produced byte-identical CSP, and every row
   kept its status.
