# Test case reference

This page is for authors of test case input. It describes the RoboChart
constructs the generator supports, the optional fixture columns and their JSON
forms, timed observations and shared fixtures. The
[README](../README.md#the-input-you-provide) gives the required columns of
each CSV file and how to run the tools.

The generator was developed against the ExpressionTests input, which is
maintained, with the testsuite project generated from it, in
[robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite).
Examples on this page that name a case ID (such as `attribute/000` or
`sequence/pass`) refer to that input.

## Supported constructs

The table lists the constructs that occur in the ExpressionTests source
cases, including cases that cannot currently be checked, and what the
generator does with each.

| Source construct | Generator/context requirement | Current position |
| --- | --- | --- |
| Integers, Booleans, arithmetic and logic | Typed constants, unchanged source expressions, exact observations and sufficient finite bounds | Supported; bounds are checked by widening, not proved complete. |
| Real and string expressions | Faithful value encodings and suitable oracle types | Integral real observations are supported; fractional reals are blocked by the oracle (R1). String inputs are uninitialised `string` constants whose values reach `D__`, avoiding RoboTool's missing string-literal compilation (B9); `is`/`as` on them then produce ill-typed CSP (B12). |
| Core sequences | Literals, empty values, one-based indexing, `cat`, `^`, and `size` | Supported; the two original indexing rows use the shared `items` fixture. |
| Sets, bounded intervals, pair tuples, vectors and matrices | Native output types and literal encodings | Supported; intervals are checked by finite integer membership. The six descending `{a to b}` rows are translator defects: Python records `{b..a}` where RoboChart's range is empty. |
| Records and field access | Record declarations, ordered fields, and bound operand values | Supported; `obj.value` uses the shared `obj` fixture. `obj.child::value` and `R(| x = <1, 2> |).x[1]` are outside RoboChart's syntax. |
| Quantifiers, comprehensions, descriptions, `let` and lambdas | Scoped binders, finite evaluation domains and graph observations for function results | Fixture-domain preparation and existing lambda observations are supported; quantifier/description compilation remains an oracle limitation. |
| Calls, enums and qualified types | Function/enum/type declarations and input environments independent of expected outputs | `add(1, 2)`, `answer()` and `Color::red` use shared fixtures. `pkg::value` (in RoboChart, `::` names only enumeration literals) and `a is pkg::Type` (RoboChart has no type synonyms) cannot be given a faithful context. |
| `since` and `sinceEntry` | Clock/state declarations and a timed observation context | Discrete timed guard observations are supported through `timing` metadata. The original rows remain blocked: `since(timer)` records fractional time 7.5, and `state` in `sinceEntry(pkg::state)` is a RoboChart keyword. |
| Source forms outside RoboChart's accepted syntax | Preserve the source row and report incompatibility | Includes open-ended intervals, degenerate tuples, empty records and other parser/validation failures. |

The sequence examples use native literals and indexing, and native
concatenation operators. The real library declaration of `size` is resolved
with `import sequence_toolkit::*`; the selective import produces invalid CSP
(B7 in
[the oracle bug list](robotool-bugs.md#b7--selectively-importing-size-produces-an-unresolved-precondition)).
RoboTool compiles `size(sequence)` to CSP `length(sequence)`. No toolkit
`concat`, `head`, `last`, or replacement indexing function is used. The source
`sequence/pass` and `sequence/mismatch` statement cases test the execution
order of two assignments; that meaning of “sequence” is distinct from the
`Seq(T)` type.

## Timed expression observations

An optional `timing` CSV column selects the timed CSP for a direct
`since(clock)` or `sinceEntry(state)` expression. For example,
`{"ticks":3,"max_ticks":5}` establishes three elapsed discrete ticks and an
observation range from zero to five. Both numbers describe the fixture; neither
is derived from `python_output`. The current context supports an unqualified
clock or state identifier and no ordinary input bindings.

The model declares the clock or state, signals readiness after reset/entry,
and observes the expression through guarded transitions. A CSP driver permits
exactly the specified ticks before sampling and blocks further ticks. The
output branches cover the whole declared observation range, so an incorrect
recorded result fails refinement. The expression stays in the guards rather
than being sent as an action value. `case-index.csv` records `timed=true`, and
the check runs under `csp-gen/timed/`; ordinary cases use `csp-gen/`.
Preparation and widening preserve the generated clock definitions and
arithmetic operators.

These contexts use RoboTool's discrete tick semantics. Fractional observations
remain unsupported. Neither original time row can fit this context:
`since(timer)` records 7.5, and `sinceEntry(pkg::state)` does not parse because
`state` is a keyword. The generator reports these blockers before reporting
missing timing metadata, and it does not infer elapsed time from the recorded
outputs. The harness tests exercise clock contexts independently; they are not
new translator cases.

## Extend typed fixtures

The CSV columns for typed expressions and statement fixtures accept JSON
values. The expression metadata columns (`types`, `datatypes`, `output_type`,
`functions`, and `function_definitions`) may be empty or absent for older
rows. Statement rows use `types` to declare every variable; `datatypes`, event
maps, drivers, iteration, function fixture columns, and `constants` are
optional.

The supported fixture types are `int`, `nat`, `boolean`, `real`, named
records, `Seq(T)`, `Set(T)`, product types such as `int * boolean`, and
`vector(T, n)` / `matrix(T, rows, columns)`. Collections and products can
nest. The current CSP oracle represents real numbers with integers: exact
integral JSON observations such as `1.0` are encoded as `1`; fractional
observations are reported as `oracle_representation_limit` without rounding.
A `datatypes` object maps each record name to an **ordered array** of
`[field_name, field_type]` pairs. The array order defines the record layout;
JSON record values are objects keyed by field name. Records can contain other
records and sequences, but recursive record definitions are rejected.

### Expression rows

For an expression case, `types` maps each input name to its type and
`output_type` declares the result type. For example, the metadata for a typed
expression with one record input could be:

```json
{
  "types": {"command": "SpinCommand"},
  "datatypes": {
    "SpinCommand": [["angleVelocity", "int"], ["duration", "int"]]
  },
  "output_type": "SpinCommand"
}
```

Each type that a row does not declare is inferred from the recorded JSON
values: without `types`, the type of every input; without `output_type`, the
type of the output. A row with none of these columns is untyped. Every row
gets the same kind of model, and each declaration in it is preceded by a
comment that names the context rule that made it (see
[architecture.md](architecture.md#3-scalar-context-expressioncontextrs)).

The original expression cases come from ExpressionTests' `cases.csv`; their
source expressions, inputs and recorded outputs are preserved. Pair tuples,
vector and matrix literals use their native RoboChart output types and CSP
encodings. A bounded interval observation is represented by the integers it
contains, using its recorded endpoints and open/closed flags; this is a finite
membership check, not a comparison of Python interval object identity.
Open-ended intervals and tuples with fewer than two elements remain outside
RoboChart's accepted syntax.

Two keys of an expression row's `inputs` object are not input names:

- `apply` lists the integer arguments at which a `lambda` expression is
  observed, for example `{"apply": [-1]}`. Its `python_output` is either
  `{"value": v}` for a single argument or a function graph,
  `{"graph": [{"arg": a, "defined": true, "value": v}, …]}`, where a point
  with `"defined": false` has no value.
- `domains` records the domain of bound variables of a quantified expression,
  not a constant named `domains`, for example `{"domains": {"int": [0, 1, 2]}}`
  (the domain types are `int`, `nat` and `boolean`). Preparation restricts
  only the corresponding CSP binder generators; arithmetic and output types
  retain their wider bounds. The RoboChart expression remains unchanged. This
  makes the unbounded set-comprehension cases checkable; quantifier and
  definite-description support still depends on RoboTool's expression
  compiler.

Equivalence contexts use explicit `domains.values` choices for both constant
and event inputs. Input sequence length is independent of the larger sequence
type needed for computed outputs, and includes recorded input lengths. For a
direct sequence-index expression, the context excludes sequences shorter than
the original literal index. This restriction uses the original expression, so
an index mutation can still fail. It does not infer arbitrary programme
preconditions. Missing input environments are reported by the oracle; the
generator does not obtain undeclared values from the expected output.

### Statement rows

For a statement case, `inputs` gives the initial variable values, `types`
gives their types, and `observe` lists the variables to check. The oracle's
`python_output.state` must contain exactly those variables.
`python_output.events` is an ordered output-event trace checked before the
state observation. Declare outgoing event names and their payload types in
`events`; use `null` for a signal event. This example shows a record update
followed by a typed event and a signal. It is written as one JSON object whose
keys are the row's columns; in the CSV file each JSON value is the content of
its column, and `robochart` is plain text:

```json
{
  "datatypes": {
    "SpinCommand": [["angleVelocity", "int"], ["duration", "int"]]
  },
  "types": {"command": "SpinCommand"},
  "observe": ["command"],
  "inputs": {"command": {"angleVelocity": 2, "duration": 1}},
  "robochart": "command.angleVelocity = 4; commandSent ! command; accepted",
  "events": {"commandSent": "SpinCommand", "accepted": null},
  "python_output": {
    "state": {"command": {"angleVelocity": 4, "duration": 1}},
    "events": [
      {"event": "commandSent", "value": {"angleVelocity": 4, "duration": 1}},
      {"event": "accepted"}
    ]
  },
  "domains": {
    "int": [1, 2, 3, 4],
    "values": {
      "SpinCommand": [
        {"angleVelocity": 2, "duration": 1},
        {"angleVelocity": 4, "duration": 1}
      ]
    }
  }
}
```

For incoming events, `input_events` maps event names to a payload type or
`null`. `driver` is a finite JSON array of inputs. A typed input supplies an
event name, a variable binding, and a value; for example,
`[{"event":"scan","bind":"reading","value":2}]`. A signal input supplies only
its event name. If a case declares input events, provide a finite driver
script for the inputs it needs.

The optional `constants` column is a JSON list of input variable names that
should be declared immutable, such as `["ranges"]`. These values are passed as
case parameters to the generated CSP process. A driver event cannot bind to a
constant.

`iteration` can request a fixed unrolling such as `{"index":"i","count":3}`.
The generator assigns the index values 1 to `count` and repeats the case's
statement body that many times. Within that body, `{{index}}` expands to the
current literal index: `total = total + ranges[{{index}}]` becomes separate
actions reading `ranges[1]`, `ranges[2]`, and `ranges[3]`. This avoids dynamic
index reads over unrelated values in FDR's finite domain. Counts above 64 are
rejected; this fixture does not add a native RoboChart loop. A count of zero
executes no body statements.

In `domains`, `int` and `nat` arrays contribute to the finite numeric bounds
from their minimum to their maximum, including intermediate integers.
`sequence_length` raises the finite sequence-length bound. `values` can
explicitly list choices for named record or `Seq(T)` types to keep FDR's value
space small. These choices replace the domain for that type in this case's
CSP package; they must include every record or sequence value the case may
reach, including intermediate values. Numeric bounds must also include
intermediate results: RoboTool's bounded arithmetic can otherwise conceal an
extra increment at the upper bound. A value outside the listed choices can
cause generation or FDR to fail. Without `values`, RoboTool's generated finite
type domains are used.

### Helper functions

For a pure helper, `functions` contains the RoboChart function declaration and
`function_definitions` maps each helper name to its CSP parameters and
equation, for example `{"increment":{"parameters":["x"],"body":"x + 1"}}`. The
explicit equation supplies test semantics for calls in that case. The check
does not prove that the equation implements the RoboChart function
postcondition. Every declared function needs exactly one definition, and a
definition may not call another declared helper.

## Shared fixtures for original rows

Some expression rows name context that the source CSV does not declare; in
ExpressionTests, for example, `obj.value`, `items[1]`, `add(1, 2)`, `answer()`
and `Color::red`. A `shared_fixtures.json` file in the source directory
declares that context once, with fixed values, and maps each such case ID to
the fixtures it uses and its output type. The generator merges these
declarations into the row's model when it loads the CSV; `cases.csv` itself is
unchanged. Fixture objects use the
same forms as the typed columns: `datatypes` (records, or
`{"enumeration": [...]}` for an enumeration), `constants` (type and value),
`functions` and `function_definitions`. The generator snapshots the file into the
project's `source-data/` directory so that preparation and verification see
the same context.

The file has three keys:

```json
{
  "description": ["Free text explaining where the fixture values come from."],
  "fixtures": {
    "obj": {
      "note": "A one-field record constant for obj.value; the field value 4 is a fixture choice.",
      "datatypes": {"Obj": [["value", "int"]]},
      "constants": {"obj": {"type": "Obj", "value": {"value": 4}}}
    }
  },
  "cases": {
    "attribute/000": {"fixtures": ["obj"], "output_type": "int"}
  }
}
```

Each fixture needs a `note`; each entry in `cases` names at least one fixture
and the case's `output_type`. Every fixture must be used by some case, every
case must be an expression case in `cases.csv`, and two fixtures of one case
may not declare the same name.

Fixtures are data, not derived from `python_output`. When the input does not
record the environment used to obtain the Python results, as in
ExpressionTests, a fixture value such as `obj.value = 4` is a choice made for
the generated testsuite, and its note in the file should say so. A row that already declares its own context
columns, or that has recorded inputs, is rejected rather than overridden. A
fixture may not declare a RoboChart library name such as `size`; library
operations come from the real imports. Because the recorded results depend on
the fixture values, replacing a fixture value with a different one can make
the affected checks fail.

## Included examples

The ExpressionTests input in
[robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite)
adds statement cases and typed expression cases to the original expression
cases. They use integer instantiations of the paper's LiDAR, mask and spin
command structures, and exercise record construction and updates, nested
sequences, Boolean sequences, concatenation, threshold boundaries, bounded
scan accumulation, typed events, input scripts and explicitly instantiated
helper calls. Their numeric values are test data; these checks do not
establish real-number arithmetic correctness. That repository describes the
cases and their verification results.
