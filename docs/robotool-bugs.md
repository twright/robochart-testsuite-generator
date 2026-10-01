# RoboTool oracle defects and limitations

Oracle defects and limitations found while validating the generated testsuite.
The system under test is the RoboChart-to-Python translator; RoboTool and
FDR provide the source-semantics oracle. These findings can prevent a check
or cause misleading evidence about the Python translator. They are not
confirmed defects in that translator. Each one has a minimal, standalone reproduction in
[`robotool-bugs/`](robotool-bugs/). The recorded results below are from the earlier reproduction run; B3 remains
a semantics/API question, and B5 has a lexical workaround. B7 was reproduced
separately with a wildcard-import control on 2026-10-01.

The findings were made while checking the ExpressionTests input, which is
maintained in
[robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite);
"the corpus" and the case IDs below refer to that input. The source-mutation
campaign and its claims document are in
[robochart-testsuite-mutation](https://github.com/twright/robochart-testsuite-mutation).
Paths under `target/` named below are logs and diagnostic projects from the
original investigation; they are not part of this repository.

## Environment

| Component | Version |
|---|---|
| RoboTool | 1.2.2026062401 |
| RoboChart textual plugin | `circus.robocalc.robochart.textual_3.1.0.202506240935` |
| CSP generator plugin | `circus.robocalc.robochart.generator.csp_3.0.0.202601080915` |
| FDR | 4.2.7 (`refines`) |
| Java | OpenJDK 25.0.3 |

## Reproducing

```sh
ROBOTOOL_HOME=/path/to/RoboTool FDR_REFINES=/path/to/refines \
  docs/robotool-bugs/reproduce.sh          # all bugs, or name some: B1 B4
```

Each `B<n>/model.rct` starts with a comment stating the bug. For each bug, the
script copies the model into a fresh project, runs
RoboTool's headless CSP generator on it in the same way `testsuite-runner` does:

```sh
java --add-opens java.base/java.lang=ALL-UNNAMED -Djava.awt.headless=true \
  -cp "$ROBOTOOL_HOME/plugins/*" circus.robocalc.robochart.generator.csp.Main true false <project>
```

Then it runs FDR on `B<n>/check.csp` in the generated `csp-gen` directory.

## Summary

| ID | Area | Summary | Minimal trigger | Observed | Expected | Severity | Found by |
|---|---|---|---|---|---|---|---|
| [B1](#b1--applying-a-function-typed-constant-produces-invalid-csp) | Translation | Applying a function-typed constant produces CSP that uses the constant's bare name | `const f : int -> int = lambda x: int @ x + 1` and `output ! f(1)` | FDR rejects the CSP: `f is not in scope` | output `2` | High: a valid model cannot be checked | Lambda-row design (named-constant form) |
| [B2](#b2--a-lambdas-domain-predicate-is-dropped) | Translation | A lambda's domain predicate is dropped, so a partial lambda becomes total | `output ! (0 in dom(lambda x: int \| x > 0 @ x + 1))` | outputs `true` | `false` | High: wrong semantics, silently | Lambda-row design (partial lambda graph check) |
| [B3](#b3--an-initialised-constants-parameter-is-ignored) | Translation | `D__` takes an initialised constant as a parameter but ignores the argument | `const a : int = 1`, `output ! a`, run as `D__(0, 2)` | outputs `1` | either `2`, or no parameter for `a` | Medium: to confirm whether intended | Equivalence checking (most general context) |
| [B4](#b4--the-integer-bound-computation-uses-saturating-arithmetic) | Bound computation | RoboTool sizes `core_int` from the model's expressions but evaluates them with the saturating operators over the default range, so the type cannot contain the values the model computes (saturation itself is intended) | `const a : int = 2`, `output ! (a * a * a)` | `core_int = { -2..2}`; outputs `2` | `core_int` contains `8`; outputs `8` | High: silent wrong values under RoboTool's own bounds | Mutation campaign, widening control |
| [B5](#b5--a-sequence-starting-with-a-negative-number-does-not-parse) | Parser | A sequence literal starting with a negative number does not parse | `const s : Seq(int) = <-1>` | parse error `mismatched character '1' expecting '>'` | the sequence `<-1>` | Low: workaround `< -1>` | Property test G28 |
| [B6](#b6--bounded-quantifiers-reach-an-unimplemented-expression-compiler-case) | CSP generation | Bounded quantifiers reach unimplemented compiler cases | `forall x: int | x in domain @ x == 1` | `ForallImpl` / `ExistsImpl` compilation fails | Valid quantifiers compile | High: checks blocked | Bounded-quantifier investigation |
| [B7](#b7--selectively-importing-size-produces-an-unresolved-precondition) | CSP generation | Selective `size` import emits an undefined precondition reference | `import sequence_toolkit::size`, `output ! size(<0>)` | FDR rejects `pre_sequence_toolkit_size is not in scope` | Generated CSP loads, as with wildcard import | Medium: wildcard import works | Core sequence context alignment |
| [B8](#b8--a-definite-description-is-translated-as-a-function-graph) | Translation | A definite description is translated as the graph of a lambda, dropping its predicate | `output ! (the x: int \| x == 1 @ x)` | FDR rejects the CSP: `funcToSet(...)` has type `{(Int, Int)}` where `Int` is required | output `1` | High: checks blocked | Review refresh, `the_*` rows |
| [B9](#b9--string-literals-are-not-compiled) | CSP generation | A validated string literal reaches an unimplemented compiler case | `output ! ("hello")` | `Case not treated by expression compiler: StringExpImpl` | CSP for the literal | Medium: checks blocked | Review refresh, `string` row |
| [B10](#b10--result-in-an-action-is-not-compiled) | Validation/CSP generation | `result` in an action validates, then reaches an unimplemented compiler case | `output ! (result)` | `Case not treated by expression compiler: ResultExpImpl` | a validation error, or CSP | Low: checks blocked | Review refresh, `special_result` row |
| [B11](#b11--from-and-to-translate-to-unbound-csp-names) | Translation | `from` and `to` validate but translate to unbound CSP names | `output ! (from)` | FDR: `from is not in scope` | a validation error, or CSP that loads | Medium: checks blocked | Review refresh, `special_from`/`special_to` |
| [B12](#b12--type-tests-and-casts-of-a-string-are-ill-typed-in-csp) | Translation | `a is int` / `a as int` for `a : string` validate, but the CSP uses `a` as an integer | `const a : string`, `output ! (a is int)` | FDR: `Couldn't match expected type Int with actual type <Char>` | output `false` for `is` | Medium: checks blocked | String-input context for `type_check`/`cast` |

The bounded-quantifier lead has now been reproduced as B6. The divide
lead is a combination of the oracle's integer approximation of real numbers
and the harness's decimal CSP literal encoding (R1 below), rather than
confirmed evidence of a Python-translation defect.

## B1 — Applying a function-typed constant produces invalid CSP

`B1/model.rct`:

```
const f : int -> int = lambda x: int @ x + 1
...
action output ! f(1)
```

RoboTool accepts the model and generates CSP. The application is translated
with the bare name `f`, not the constant's CSP name `const_b1_mod0_ctrl0_stm0_f`
(which is the parameter of `D__`):

```
output.out!(callFunc((f), (1))) -> SKIP
```

FDR then rejects the file:

```
defs/b1.csp:227:99-100: f is not in scope
```

For comparison, `dom(f)` in the same position is translated correctly, as
`relation_toolkit_dom(const_…_f)` (seen while prototyping the lambda rows). Only
the application uses the bare name.

**Impact on graph-format checks:** a model that names a lambda as `f`
and applies it hits this failure. The generator supports such checks, but the
current corpus has six original value-format lambda rows, not eight graph
rows. Those rows now use a `rimage` graph-slice context and their existing recorded
application results; all six verify. That context avoids named-function
application, so B1 does not block these rows.

**Expected:** `output.out!(callFunc(const_b1_mod0_ctrl0_stm0_f, 1))`, and
`B1/check.csp` passing with output `2`.

## B2 — A lambda's domain predicate is dropped

`B2/model.rct` (imports `relation_toolkit` for `dom`):

```
action output ! (0 in dom(lambda x: int | x > 0 @ x + 1))
```

The generated CSP builds the lambda's graph over all of `core_int`, with no
trace of the predicate `x > 0`:

```
output.out!(member(0, relation_toolkit_dom(funcToSet((\ x @ Plus(x, 1, core_int)), core_int))))
```

where `funcToSet(f, s) = { (x, f(x)) | x <- s}`. FDR's counterexample for
`B2/check.csp`:

```
Available Events: {b2::mod0::output.out.false}     (specification)
Error Event: b2::mod0::output.out.true             (model)
```

**Expected:** the graph restricted to the predicate, for example
`{ (x, Plus(x, 1, core_int)) | x <- core_int, x > 0 }`, so that `0` is not in the
domain and the output is `false`. Applying the lambda at `0` should also be
undefined; with this translation it returns `1`.

**Impact on graph-format checks:** B1 prevents checking a named lambda's
application. In exploratory generated CSP, replacing the bare application
name exposed B2 for arguments outside the domain. This was a diagnostic
oracle experiment, not a corpus result: the proposed corpus graph data and its
stand-in expected outputs were reverted. The current `rimage` context covers
only the recorded in-domain arguments of the partial-lambda rows. The standalone B2 reproduction
checks membership directly and does not need that patch.

## B3 — An initialised constant's parameter is ignored

`B3/model.rct`:

```
const a : int = 1
...
action output ! a
```

RoboTool makes `a` a parameter of the process, `D__(id__, const_b3_mod0_ctrl0_stm0_a)`,
but the translated model uses the initialiser, not the parameter.
`B3/check.csp` runs `D__(0, 2)`:

```
uses_argument [FD= model:      Failed   (the model outputs 1, not 2)
uses_initialiser [FD= model:   Passed
```

**Expected:** one of these:

- the argument is used, so the model outputs `2`;
- an initialised constant is not a parameter of `D__`.

As it stands, a caller can pass any value without effect. This may be
intended (the initialiser overrides), in which case the parameter is
misleading. It matters for testing: a check that passes constants to `D__`
does not actually vary them. The generated testsuite always passes the initialiser's own
value, so its checks are unaffected, but they do not test argument passing.

## B4 — The integer bound computation uses saturating arithmetic

Two things are involved here. Only the second is a bug.

**Saturating arithmetic (intended).** RoboTool's CSP generator emits fixed
definitions for the arithmetic operators (in `InstantiationGenerator`, under
`-- BASIC FUNCTIONS`):

```
Plus(e1,e2,T) = if member(e1+e2,T) then e1+e2 else e1
Mult(e1,e2,T) = if member(e1*e2,T) then e1*e2 else e1
```

and likewise `Minus`, `Div`, `Modulus` and `Neg`. A result outside the finite
type `T` becomes the left operand. This keeps every value inside the finite
types FDR needs, and is a deliberate design choice: a known limitation of model
checking a bounded version of RoboChart's integers, not a bug. Its consequence
is that a model is only faithful while its values stay inside the type.

**The bound computation (the bug).** To keep values inside the type, the
generator sizes `core_int` from the model's own expressions: it takes the
default range `{ -2..2}` and adds the smallest and largest value each
expression can take. For `B4/model.rct`:

```
const a : int = 2
...
action output ! (a * a * a)
```

it generates:

```
nametype core_int = union({ -2..2},{calc_type_min(Union(
    {{(Mult(Mult(a, a, { -2..2}), a, { -2..2}))|a <- { -2..2}},{2},{ -2..2}}))..calc_type_max(…)})
```

The values of `a * a * a` are computed with the saturating `Mult`, *over the
default range*. So every product that would leave `{ -2..2}` is clamped back
into it, and the computation can never add a value outside `{ -2..2}` (other
than the literals themselves, here `2`). The result is `core_int = { -2..2}`,
although the model computes `8`. With a type that small, the model's own
saturating `Mult` then gives
`Mult(Mult(2, 2, core_int), 2, core_int) = 2`. `B4/check.csp`:

```
saturated [FD= model:   Passed   (the model outputs 2 for 2 * 2 * 2)
```

**The bug is confined to the bound computation.** Changing only the bound
expression to ordinary arithmetic, replacing
`Mult(Mult(a, a, { -2..2}), a, { -2..2})` with `a*a*a` in the generated
`instantiations.csp` and leaving the model's saturating `Mult` untouched, gives
a `core_int` that contains `8`, and FDR confirms the model then outputs `8`:

```
correct [FD= model:   Passed   (correct = output.out!8 -> (SKIP |~| STOP))
```

**Expected:** the bound computation uses ordinary arithmetic (`+`, `*`, …), or
otherwise includes the values the expressions reach, so that `core_int`
contains `8` and the model outputs `8`.

**Two further gaps in the same computation**, found while building correct
bounds for the generated testsuite:

- *Only final values are collected.* Each application of a saturating operator
  checks its own result against the type, so the type must contain every
  intermediate result. The computation collects each whole expression's value
  only. For `output ! (a * a * a / 4)` with `const a : int = 2`, RoboTool
  classifies the expression as real-valued (because of the division) and adds
  its values to `core_real`. The integer multiplications inside it,
  `Mult(Mult(a, a, core_int), a, core_int)`, are checked against `core_int`, but
  their results (`4`, `8`) are never added to `core_int`. With
  `core_int = { -2..4}` the model computes `Mult(4, 2) = 4` and outputs `1`
  instead of `2`.
- *Constants range over the whole type.* A constant is enumerated as
  `a <- { -2..2}`, although it has a single value.

**Separate limitation, not part of this bug:** the computation is a single
pass over each expression, with its variables ranging over the default type.
It cannot follow values that grow through repeated assignment (for example
`x = x + 1` executed several times), which would need a fixed point. So even
with the bug fixed, a model whose values grow step by step can still leave the
computed type and saturate.

**Relevance to the generated testsuite:** `testsuite-runner prepare-fdr`
replaces `core_int` with its own bounds. It evaluates RoboTool's bound expressions itself, with plain
arithmetic, one set per operator application, constants at their values and
variables over the case's range, and then rechecks every verdict with wider
bounds. RoboTool's operators are left as generated. The prepared bound sections
are annotated `-- generate int not` and `-- generate nat not`, which RoboTool
keeps when it regenerates `instantiations.csp`. (Before this, the
literal-only bounds were too narrow in the same way: 101 surviving mutants
became killed when `core_int` was widened; see A3 in the
[claims document](https://github.com/twright/robochart-testsuite-mutation).)

**Additional harness defect found during the rerun:** negative constant
bindings in the sizing-only CSP were written as `a <- {-3}`. CSPm reads `{-`
as a multiline-comment opener, so sizing failed and fell back to fixture
ranges. This is now `a <- { -3}`. An FDR regression confirms extension to
`{-6..3}` for `(-a) + b` at `a = b = -3`, including the intermediate `3`.
The campaign rechecks all 893 cases with negative constant inputs; the first
attempt is preserved in `target/mutation-sizing-fallback/`.

**Remaining sizing gap observed in the corrected run:** RoboTool's bound
expressions can omit arithmetic used only inside a Boolean comparison, such
as `(-a) > b`. Evaluating those expressions cannot recover an omitted operand
value. The widening check reports such unstable verdicts as inconclusive.
The helper must therefore not be described as complete reachable-value analysis.

## B5 — A sequence starting with a negative number does not parse

`B5/model.rct`:

```
const s : Seq(int) = <-1>
```

RoboTool rejects the model (exit status 2):

```
ERROR:mismatched character '1' expecting '>' (… line : 13 column : 28)
ERROR:no viable alternative at input '>' (… line : 13 column : 31)
```

The lexer reads `<-` as one token. `< -1>`, with a space, parses. CSPm has the
same token, so FDR also rejects `<-1>` in a CSP script
(`Unexpected token <-`). The testsuite generator now writes `< -1>`.

**Expected:** `<-1>` parses as a one-element sequence, or the grammar documents
that a space is required.

## B6 — Bounded quantifiers reach an unimplemented expression compiler case

[`B6/model.rct`](robotool-bugs/B6/model.rct) uses a nonempty set and no
fixture machinery:

```text
const domain : Set(int) = {1}
action output ! (forall x: int | x in domain @ x == 1)
```

The textual model loads without validation errors. CSP generation exits
with status 255 and reports:

```text
LOG: Case not treated by expression compiler: class circus.robocalc.robochart.impl.ForallImpl
java.lang.RuntimeException: Case not treated by expression compiler: class circus.robocalc.robochart.impl.ForallImpl
```

Replacing `forall` with `exists` and `exists1` in this reduced model produces
`ExistsImpl` failures for both existential forms. In the 2026-10-01 review
refresh, all 70 original quantifier rows (`forall`, `exists`, `exists1` and
`multiple_variables`, bounded and unbounded) fail with these messages, so
they have an oracle compiler limitation in common. This
blocks comparison with the recorded Python results; it does not show those
results are wrong. No quantifier semantics have been substituted in the
harness. Run `reproduce.sh B6`; change only the quantifier to exercise the
other two forms.

## B7 — Selectively importing size produces an unresolved precondition

[`B7/model.rct`](robotool-bugs/B7/model.rct) imports the real library declaration
and calls it on a sequence:

```text
import sequence_toolkit::size
...
action output ! size(<0>)
```

RoboTool's headless generator exits successfully. Its CSP expression uses the
native `length(<0>)`, but the action also requires
`pre_sequence_toolkit_size(<0>)`. That precondition definition is absent, so
FDR rejects even the reflexive refinement in
[`B7/check.csp`](robotool-bugs/B7/check.csp):

```text
defs/b7.csp:214:57-82: pre_sequence_toolkit_size is not in scope
```

Changing only the import to `import sequence_toolkit::*` generates loadable CSP;
the same reflexive refinement passes. This isolates an import-dependent CSP
generation defect. Lowering `size` to native `length` is not itself the defect:
the unresolved precondition reference is. The reproduction checks CSP loading
without introducing a new expected Python output or corpus case.

The fixture and context generators use the wildcard import as the workaround,
preserving the real library declaration and its preconditions. A temporary
local empty declaration of `size` was removed because it bypassed the library
precondition. Sequence literals, one-based indexing and concatenation still use
core syntax. Logs of the selective and wildcard runs are in
`target/B7-selective.log` and `target/B7-wildcard.log` respectively. Run
`reproduce.sh B7`, then change only the import to reproduce the control.

## B8 — A definite description is translated as a function graph

[`B8/model.rct`](robotool-bugs/B8/model.rct) outputs a definite description
whose value is determined by its predicate:

```text
action output ! (the x: int | x == 1 @ x)
```

The model validates and RoboTool's headless generator exits with status 0.
The generated CSP uses the lambda translation, `funcToSet((\x @ x), core_int)`:
the set of pairs of the function `x ↦ x`, with the predicate `x == 1`
dropped. It is sent on the `int` channel. FDR rejects even the reflexive check
in [`B8/check.csp`](robotool-bugs/B8/check.csp):

```text
Couldn't match expected type {(Int, Int)} with actual type Int
    {{(funcToSet((\x @ x), { -2..2})) | x <- { -2..2}}, { -2..2}}
```

The grammar has a separate `DefiniteDescription` rule, so this is a
translation defect, not a parse ambiguity. All six original `the_*` rows stop
here (`fdr_no_result`); their recorded Python results are neither confirmed
nor contradicted. Run `reproduce.sh B8`; the log is `target/B8.log`.

## B9 — String literals are not compiled

[`B9/model.rct`](robotool-bugs/B9/model.rct) sends `"hello"` on a `string`
event. The model validates; CSP generation exits with status 255:
`Case not treated by expression compiler: class …StringExpImpl`. The original
`string/000` row stops here. String *inputs* avoid this: the generator declares
them as uninitialised constants and passes their value to `D__` (see B12).

## B10 — `result` in an action is not compiled

[`B10/model.rct`](robotool-bugs/B10/model.rct) outputs `result`. The model
validates, but generation exits with status 255: `Case not treated by expression
compiler: class …ResultExpImpl`. RoboChart gives `result` a meaning in function
postconditions, so the original `special_result/000` row may also be a case
specification error; RoboTool should either reject it or translate it.

## B11 — `from` and `to` translate to unbound CSP names

[`B11/model.rct`](robotool-bugs/B11/model.rct) outputs `from`. The model
validates and RoboTool exits with status 0, but the CSP names an unbound `from`,
and FDR reports `from is not in scope`. `to` fails in the same way
(`special_to/000`: `to is not in scope`). No context that binds them was found.
Log: `target/B9-B11.log`.

## B12 — Type tests and casts of a string are ill-typed in CSP

[`B12/model.rct`](robotool-bugs/B12/model.rct) declares an uninitialised
`const a : string` and outputs `a is int`. RoboTool validates the model and
generates `member(a, core_int)`, so `D__` takes `a` as an `Int` even though
`core_string = LSeq(Char,2)`. Supplying any string fails:

```text
Couldn't match expected type Int with actual type <Char>
    In the second argument of b12::mod0::D__, namely "a"
```

`a as int` generates `output.out!(a)` and fails identically. These are the best
contexts for `type_check/002` (`a = "a"`) and `cast/001` (`a = "3"`). For the
cast, RoboChart's `as` does not parse strings, so the case may also be a
specification error. Log: `target/B12.log`.

## R1 — Real division and decimal CSP expectations

[`R1/model.rct`](robotool-bugs/R1/model.rct) declares `output : real` and
outputs `a / b` for integer constants `a = 3`, `b = 2`. RoboTool emits:

```text
channel output: InOut.core_real
Div(e1,e2,T) = if member(e1/e2,T) then e1/e2 else e1
```

`core_real` is a finite set of CSP integers. In the reduced model FDR
confirms output `1`, as recorded by the diagnostic assertion in
[`R1/check.csp`](robotool-bugs/R1/check.csp). This assertion records oracle
behaviour; it is not a Python translator expectation.

The corpus `divide` rows record Python true-division results as JSON
floats, including `1.0` and fractional values. The earlier harness copied
these literally into CSP, such as `output.out!1.0`. CSPm treats the dot as a
field separator. The reduced checks retain this diagnostic example.

The fixture generator now encodes an exactly integral JSON float with an
integer CSP literal: `1.0` becomes `1`, and `-0.0` becomes `0`. All six such
division rows verify, including the wider-bound recheck. Their CSV values and
RoboTool's arithmetic operators remain unchanged. The six fractional division
observations are rejected with `oracle_representation_limit`; they are not
rounded. A genuinely fractional result has no exact representation in the
current integer-valued real domain.

The investigation does not establish whether RoboTool's real approximation
is intended. Run `reproduce.sh R1` for the diagnostic integer observation and
invalid fractional-check reproduction.

## How verification results are attributed

`testsuite-runner verify` gives every row that does not pass one of these statuses, in
the project's `verification-results.csv`:

- `translator_defect`: FDR refutes the recorded Python observation of a case
  that RoboTool translates. The six descending `set_range` rows are such
  defects: for `{a to b}` with `a = 0, b = -3` Python records `{-3, -2, -1, 0}`,
  while RoboChart's range is empty. RoboTool translates it as CSP `{a..b}`, and
  a diagnostic check (`target/set-range-diagnostic/`) confirms that the model
  outputs only `{}`.
- `unsupported_robotool`: the case is valid RoboChart, but a reproduced RoboTool
  defect (B6, B8–B12, recognised by its log message) or the integer-valued
  reals (R1) prevents a verdict.
- `unsupported_case`: the case expression is not valid RoboChart, whatever the
  context: either the generator rejects syntax RoboChart lacks, or RoboTool's
  first validation error lies inside the case expression. These are likely
  errors in the case specification:
  `matrix` is a keyword (`matrix[2, 1]`, `transpose(matrix)`); `::` in an
  expression names only enumeration literals (`pkg::value`, `obj.child::value`);
  indexing cannot follow a field selection (`R(| x = <1, 2> |).x[1]`);
  `TRUE` is not a literal (the translator emits Python `True` without consulting
  the environment, so a declared constant would test something else);
  `a is pkg::Type` would need a type containing `1`, but RoboChart has no type
  synonyms; `state` is a keyword (`sinceEntry(pkg::state)`); tuples need two or
  more values, records at least one field, and intervals an upper bound; and a
  type name such as `real` is not a value.
- `failure`: no verdict, without evidence for any of the above (for example a
  timeout). The ExpressionTests results have none.
