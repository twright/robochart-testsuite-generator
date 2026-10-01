# RoboChart testsuite generator

The system under test is a RoboChart-to-Python translator. For each RoboChart
expression or statement, a test case records the input values and the result
that the translated Python produced. The tools in this repository turn such
recorded observations, which you provide, into a testsuite and check them
against RoboChart's semantics:

- `testsuite-generator` reads the test cases you provide and writes one
  RoboTool Modeling Project, the *testsuite project*, with a separate
  RoboChart model and a CSP check for each case it can represent.
- `testsuite-runner` is the oracle. It has RoboTool compile each model to CSP
  headlessly and has FDR check that the model produces the recorded result.

`testsuite-runner` does not execute the translator or the Python code. It
checks the observations recorded in the input against RoboTool's semantics of
the source models; a refuted observation is evidence of a translator defect.
Any Python text stored in the input is documentation only. The checks are
finite and fixture-specific: a verdict applies to the supplied inputs, value
domains, event scripts and helper equations, and does not establish the
behaviour of a complete system.

This repository contains the tools only. Two related repositories hold a
particular testsuite and the tools' validation:

- [robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite)
  holds the ExpressionTests input, the testsuite project generated from it
  with its diagrams and verification results, and instructions for reviewing
  and rechecking that suite. It is a complete example of the input described
  below.
- [robochart-testsuite-mutation](https://github.com/twright/robochart-testsuite-mutation)
  holds the `mutate` tool, which tests the checks by source mutation of an
  input, and the claims of its mutation campaign.

## Prerequisites

These instructions are for Linux on x86-64, where the tools have been used.

| Tool | Version | Used for |
| --- | --- | --- |
| Rust and Cargo | 1.85 or later (the workspace uses the 2024 edition) | Building the two binaries. Install with [rustup](https://rustup.rs/). The first build downloads the crates listed in `Cargo.lock`. |
| RoboTool | 1.2.2026062401 | Compiling the models to CSP during the checks, and reviewing the diagrams. Download it from the [RoboTool releases](https://robostar.cs.york.ac.uk/robotool/releases/). This is the version the tools have been tested with. |
| Java | 17 or later, as `java` on `PATH` | Running RoboTool's CSP generator during the checks, and building the diagram plug-in (which also needs `javac` and `jar`). |
| FDR | 4.2.7, activated with a licence | Running the refinement checks with its `refines` executable. |

Generating a testsuite project needs only Rust; RoboTool, Java and FDR are
needed to check it.

RoboTool's own window runs on the Java 17 runtime bundled inside it, in
`plugins/org.eclipse.justj.openjdk.hotspot.jre.full.linux.x86_64_17.*/jre/bin`.
`testsuite-runner`, however, starts RoboTool's CSP generator itself with the
`java` found on `PATH` and the classpath `$ROBOTOOL_HOME/plugins/*`. If you
have no system Java 17 or later, put that bundled directory on `PATH`; it also
contains `javac` and `jar`.

FDR must be activated before it runs any check: the first run of `refines` or
`fdr4` asks for a licence and contacts FDR's licensing servers. Installation
and activation instructions for FDR and RoboTool are on the
[SETSS 2025 tutorial page](https://robostar.cs.york.ac.uk/events/setss2025/).
That page also notes that on Ubuntu FDR is likely to need the legacy
`libpng12-0` library, and provides packages for Ubuntu 20.04 and 22.04.
`refines --version` prints the installed version. If activation fails, see
[FDR cannot reach its licensing servers](#fdr-cannot-reach-its-licensing-servers).

Two settings tell `testsuite-runner` where the tools are. Each can be given
as an environment variable or as an option, and the option takes precedence:

| Environment variable | Option | Value |
| --- | --- | --- |
| `ROBOTOOL_HOME` | `--robotool-home DIR` | The RoboTool installation directory, which contains `eclipse` and `plugins/`. |
| `FDR_REFINES` | `--fdr-refines FILE` | FDR's `refines` executable, for example `/path/to/fdr/bin/refines`. If it is omitted, `refines` is looked up on `PATH`. |

The examples below use POSIX shell syntax and placeholder paths such as
`/path/to/RoboTool`; replace them with the paths on your machine.

## Build and install

From the repository root, either install both binaries into Cargo's binary
directory (usually `~/.cargo/bin`, which `rustup` puts on `PATH`):

```sh
cargo install --locked --path crates/testsuite-generator
cargo install --locked --path crates/testsuite-runner
```

or build them in place:

```sh
cargo build --release
```

which writes `target/release/testsuite-generator` and
`target/release/testsuite-runner`. The examples below assume that the
binaries are on `PATH`; after an in-place build, either add
`target/release` to `PATH` or call them by that path. Each binary describes
its arguments with `--help`, and each `testsuite-runner` command has its own
help, for example `testsuite-runner verify --help`.

## Quick start

The repository includes a few test cases, copied from the ExpressionTests
input, for its own tests (see [Example input](#example-input)). From the
repository root, generate a project from one expression case and one
statement case, and check it:

```sh
testsuite-generator crates/testsuite-runner/src/tests/data /tmp/testsuite-smoke add/000 sequence/pass
export ROBOTOOL_HOME=/path/to/RoboTool
export FDR_REFINES=/path/to/fdr/bin/refines
testsuite-runner verify /tmp/testsuite-smoke
```

The first command reports `Generated 2 models and recorded 0 generation
failures`. The check takes a few seconds and ends with:

```text
add/000: pass (verified)
sequence/pass: pass (verified)
All test cases passed; results are in /tmp/testsuite-smoke/verification-results.csv
```

If this fails, see [Troubleshooting](#troubleshooting). The results files are
described in [Check a project](#check-a-project).

## The input you provide

The generator's input is a set of test cases that you supply, as CSV files and
an optional JSON file. The SOURCE argument of `testsuite-generator` is either:

- **a directory**, which must contain `cases.csv` (expression cases) and
  `statement_cases.csv` (statement cases), and may contain
  `shared_fixtures.json`. Other files in it are not read; or
- **a single expression CSV file**, generated on its own, without statement
  cases or shared fixtures.

Both CSV files have a header row, one row per case, and may have extra
columns, which are ignored. Several columns hold JSON values, quoted as usual
in CSV. Each `id` has the form `<feature>/<name>`, where `<feature>`
equals the row's `feature` column and is a RoboChart identifier (a letter
followed by letters, digits and `_`), and `<name>` uses letters, digits and
`_`. IDs must be unique across both files.

**Expression CSV** (`cases.csv`). Required columns:

| Column | Contents |
| --- | --- |
| `id`, `feature` | The case ID and its feature, as above. |
| `robochart` | The RoboChart expression. |
| `inputs` | JSON object mapping each input name to its value, `{}` if none. `lambda` and quantified expressions use two special keys, `apply` and `domains`. |
| `python_output` | JSON object `{"value": v}` holding the result the translated Python produced (or a function graph, for a `lambda` expression). |

Optional columns, empty or absent when not needed: `types` (JSON object of
input types), `datatypes` (JSON record and enumeration declarations),
`output_type`, `functions` and `function_definitions` (helper functions and
their CSP equations), and `timing` (for `since` and `sinceEntry`
expressions). A type that a row does not declare is inferred from the
recorded values: without `types`, the input types; without `output_type`,
the output type. A row with none of these columns is untyped. For example (the `python` column is documentation and is
ignored):

```csv
id,feature,robochart,python,inputs,python_output,types,datatypes,output_type,functions,function_definitions
add/000,add,a + b,lambda _env: _env['a'] + _env['b'],"{""a"": -3, ""b"": -3}","{""value"": -6}",,,,,
```

**Statement CSV** (`statement_cases.csv`). Required columns:

| Column | Contents |
| --- | --- |
| `id`, `feature` | The case ID and its feature, as above. |
| `robochart` | The RoboChart statement, for example `x = x + 1`. |
| `inputs` | JSON object of initial variable values. |
| `types` | JSON object giving the type of every variable. |
| `observe` | JSON list of the variables to check. |
| `python_output` | JSON object `{"state": {…}, "events": [ … ]}`. `state` must contain exactly the observed variables; `events`, the ordered output events, is optional. |
| `expected_status` | `pass` if FDR should confirm the observation, `mismatch` for a deliberately wrong observation that FDR should refute. |

Optional columns: `datatypes`, `domains` (finite value domains for FDR),
`events` and `input_events` (event declarations), `driver` (a finite script of
input events), `iteration` (a fixed unrolling), `constants`, `functions`,
`function_definitions` and `notes`.

**Shared fixtures** (`shared_fixtures.json`, optional, directory sources
only). Some expressions name context that their row does not declare, such as
a record constant or a helper function. This file declares such fixtures once
(key `fixtures`) and lists the expression cases that use them (key `cases`,
mapping each case ID to its fixture names and `output_type`). A case with
shared fixtures must have no recorded inputs and no context columns of its
own.

[`docs/test-case-reference.md`](docs/test-case-reference.md) gives the JSON
forms of all optional columns and of the shared fixtures file, the fixture
types, timed observations and which RoboChart constructs the generator
supports.

### Example input

`crates/testsuite-runner/src/tests/data/` holds the rows that the tools'
pipeline tests use, copied unchanged from the ExpressionTests input: 14
expression cases, five statement cases (including the deliberately wrong
`sequence/mismatch`) and a separate three-row expression CSV file. Its
`README.md` lists them. The complete ExpressionTests input, with its shared
fixtures, is in
[robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite).

## Generate a testsuite project

```sh
testsuite-generator /path/to/input
```

This writes into `testsuite-project/` in the current directory, the default
project directory, and prints how many models it generated and how many
generation failures it recorded. The generator:

- writes one `.rct` model per representable case under `cases/`, grouped by
  kind and feature, and its CSP check under `csp-gen/` (`csp-gen/timed/` for
  timed cases);
- writes `case-index.csv`, which maps each case ID to its model, diagram name
  and expected status;
- records each case that cannot be represented, with a category and reason,
  in `generation-failures.csv`;
- snapshots the input rows it used into `source-data/`, where the checks read
  them;
- keeps an existing `representations.aird`, and creates an empty one
  otherwise.

It does not delete models of cases that are no longer in the input; generate
into an empty directory if you have removed cases.

Give a second argument to write elsewhere, and case IDs after it, or
`--statements`, to generate a subset. Case IDs need the project argument,
because the second argument is always the project directory:

```sh
testsuite-generator /path/to/input /tmp/testsuite-subset add/000 sequence/pass
testsuite-generator /path/to/input /tmp/testsuite-statements --statements
testsuite-generator /path/to/expression_cases.csv /tmp/testsuite-expressions
```

## Check a project

```sh
export ROBOTOOL_HOME=/path/to/RoboTool
export FDR_REFINES=/path/to/fdr/bin/refines
testsuite-runner verify
```

Without a project argument, `verify` checks `testsuite-project/` in the
current directory; pass a directory, as in the quick start, to check another
project. It checks each case on its own, in a temporary project in the system
temporary directory, so that one invalid model does not stop later cases. For
each case it:

1. runs RoboTool's CSP generator headlessly on the model;
2. prepares the CSP for FDR: it widens the finite integer and sequence bounds
   to contain every value the case computes, applies any explicit record or
   sequence value domains, and adds helper-function definitions;
3. runs FDR's refinement check, with a 90-second limit;
4. reruns the check with wider bounds (see below);
5. attributes a case that does not pass (see below).

It prints one line per check, and writes into the project:

- `verification-results.csv`: one row per case, with the columns `id`,
  `status`, `category` and `reason/log`. Cases that could not be generated
  appear here too, with the category from `generation-failures.csv`.
- `verification-results.md`: the same results as a Markdown report, with
  counts per status and per category, and links to each model.
- `verification-logs/`: one log per case, with RoboTool's and FDR's output,
  named in the `reason/log` column.

`verify` exits with a non-zero status if any case does not pass. An input
that contains cases expected not to pass therefore exits non-zero; read the
results files rather than the exit status. A large project is checked case by
case and takes much longer than the quick start; `--jobs N` checks N cases
at once, each in its own temporary project. The results files, their order
and the log names do not depend on N.

### What each status means

A `pass` means that FDR's result matched the case's expected status. An
expression case expects FDR to confirm its recorded result; a statement case
marked `mismatch` deliberately records a wrong result, and passes when FDR
refutes it. Each check observes the result and then permits successful
termination or deadlock, so termination itself is not tested.

Each case is given the best context the generator can supply. A case that
does not pass is attributed, only with evidence, to one of:

| Status | Meaning |
| --- | --- |
| `translator_defect` | FDR refutes the recorded Python observation of a case that RoboTool translates. |
| `unsupported_robotool` | Valid RoboChart, but a reproduced RoboTool defect (recognised by its log message) or the integer-valued reals (R1) prevents a verdict. |
| `unsupported_case` | The expression is not valid RoboChart: the generator rejects syntax that RoboChart lacks, or RoboTool's first validation error lies inside the case expression. This is likely to be an error in the case specification. |
| `failure` | No verdict and no evidence for the above, such as a timeout, a harness error or a missing tool. |

The `category` column records the stage at which the check stopped, for
example `verified`, `fdr_mismatch`, `robochart_validation_error` or
`fdr_error`. The ExpressionTests results in
[robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite)
include cases with each status. The reproduced RoboTool defects, and how each
status is decided, are in
[the oracle bug list](docs/robotool-bugs.md#how-verification-results-are-attributed).

### Wider-bounds recheck

RoboTool's CSP uses finite integer types, and its arithmetic saturates: a
result outside the finite type becomes the left operand. A check is therefore
only faithful if the bounds contain every value that the case computes. The
preparation evaluates RoboTool's own bound expressions with plain arithmetic,
including intermediate results, and widens the integer bounds to contain them.
Each check that reaches a verdict is then rerun with wider bounds. The
`reason/log` column records the outcome, for example `same verdict with
core_int {-10..6}`; a verdict that changes is reported as a `failure` with
category `bounds_too_narrow`.

The sections of `csp-gen/instantiations.csp` that preparation changes (the
integer and natural-number bounds, and helper-function definitions) are
annotated `-- generate <name> not`, so RoboTool keeps them if the CSP is
regenerated, for example from the RoboTool IDE.

### Other commands

```sh
testsuite-runner report-markdown
```

rewrites `verification-results.md` from an existing
`verification-results.csv`, without running RoboTool or FDR.

`compile` and `prepare-fdr` run the first two stages over a whole project at
once, for example to inspect the CSP in `csp-gen/`:

```sh
testsuite-runner compile /tmp/testsuite-smoke
testsuite-runner prepare-fdr /tmp/testsuite-smoke
```

`compile` needs `ROBOTOOL_HOME` and `prepare-fdr` needs `FDR_REFINES`. If
RoboTool fails on the whole project, `compile` retries each case on its own
and records the cases that fail in `csp-gen/generator-failures.csv`.

## Review the diagrams

Each generated model has a native RoboChart diagram in the project's
`representations.aird`. The generator does not draw diagrams; a RoboTool
start-up plug-in, `org.robocalc.testsuitegenerator.diagrams`, creates the
diagrams of new cases, refreshes those of changed models, and arranges them
with RoboTool's native layout commands.
[`diagram-plugin/README.md`](diagram-plugin/README.md) describes it in more
detail.

### Build and install the plug-in

Build the plug-in with RoboTool's installation and a JDK 17 or later, given as
the directory containing `javac` and `jar`. RoboTool's bundled runtime
contains both:

```sh
diagram-plugin/build.sh /path/to/RoboTool \
  /path/to/RoboTool/plugins/org.eclipse.justj.openjdk.hotspot.jre.full.linux.x86_64_17.*/jre/bin
```

This writes `diagram-plugin/build/org.robocalc.testsuitegenerator.diagrams.jar`.
Install it in RoboTool's `dropins/` directory:

```sh
cp diagram-plugin/build/org.robocalc.testsuitegenerator.diagrams.jar /path/to/RoboTool/dropins/
```

### Create or refresh the diagrams

Start RoboTool with the project's absolute path as a JVM property. Use a
workspace that is not open in another RoboTool window:

```sh
/path/to/RoboTool/eclipse -clean -nosplash -data /path/to/review-workspace \
  -vmargs -Dtestsuitegenerator.project=/absolute/path/to/testsuite-project
```

The plug-in imports the project into that workspace if necessary (under the
name of its directory), creates every missing diagram listed in
`case-index.csv`, arranges it and saves the session. It prints how many
diagrams it created and how many were already present; the RoboTool window
stays open for review. Existing diagrams are left unchanged unless you ask for
a refresh, by adding a second property after `-vmargs`:

- `-Dtestsuitegenerator.refresh=all` refreshes and arranges every existing
  diagram;
- `-Dtestsuitegenerator.refresh=expression_add_000,statement_sequence_pass`
  refreshes only the named diagrams (names from the `diagram` column of
  `case-index.csv`).

Refresh a diagram after its model has changed, so that its labels and
connections match the model. To stop the plug-in from running, start RoboTool
without `-Dtestsuitegenerator.project`; the diagrams remain in the project.

### Open the diagrams

To import a project without the plug-in, choose **File → Import → General →
Existing Projects into Workspace**, select the project directory, and leave
**Copy projects into workspace** unchecked, so that RoboTool shows the files
that the generator writes. Open `representations.aird` in Model Explorer and
expand **RoboChart → Package** in the Representations pane to choose a
diagram. The diagram names match the `diagram` column of `case-index.csv`, for
example `expression_add_000`; the corresponding `.rct` models are under
`cases/`, at the path in the `model` column.

After regenerating a project, refresh it in RoboTool (select it and press
F5), then run the plug-in again to add diagrams for new cases and refresh
those of changed models.

## Self-tests

The tools' own tests do not need RoboTool or FDR, and take a few seconds:

```sh
cargo test --release --workspace
```

They check properties of the code with synthetic rows, small literal rows and
the example input; none of them checks a particular testsuite. Many of them
are property-based tests; set `PROPTEST_CASES` (for example
`PROPTEST_CASES=10000`) to run more random cases. Pipeline tests that run
RoboTool and FDR on small generated projects are ignored by default; run them
with:

```sh
ROBOTOOL_HOME=/path/to/RoboTool FDR_REFINES=/path/to/fdr/bin/refines \
  cargo test --release -p testsuite-runner -- --ignored
```

They take one to a few minutes.

## Troubleshooting

### FDR cannot reach its licensing servers

FDR fails to activate or to validate its licence, although the machine is
online. FDR uses an old bundled networking library that expects the system's
CA certificates at the fixed path `/etc/pki/tls/certs/ca-bundle.crt`. Most
modern Linux distributions do not provide a file at that path in one way or
another, so FDR cannot make a secure connection to its licensing servers. The
fix is to link that path to the distribution's CA bundle, as described on the
[SETSS 2025 tutorial page](https://robostar.cs.york.ac.uk/events/setss2025/).

On Ubuntu and Debian, the CA bundle is
`/etc/ssl/certs/ca-certificates.crt`, and the directory must be created
first:

```sh
sudo mkdir -p /etc/pki/tls/certs/
sudo ln -s /etc/ssl/certs/ca-certificates.crt /etc/pki/tls/certs/ca-bundle.crt
```

On Fedora, `/etc/pki/tls/certs/` already exists, but no package provides
`ca-bundle.crt` in it (as of Fedora 44 and `ca-certificates` 2025.2.80).
Create the same link to Fedora's bundle:

```sh
sudo ln -s /etc/ssl/certs/ca-certificates.crt /etc/pki/tls/certs/ca-bundle.crt
```

On other distributions, link `/etc/pki/tls/certs/ca-bundle.crt` to the
distribution's PEM bundle of trusted CA certificates. Then activate FDR again.

### Tool paths are not set

- `Error: "Set --robotool-home or ROBOTOOL_HOME to the installed RoboTool
  directory"`: set `ROBOTOOL_HOME`, or pass `--robotool-home`.
- `Error: "RoboTool plugins directory does not exist: …/plugins"`:
  `ROBOTOOL_HOME` must be the RoboTool installation directory itself, the one
  containing `eclipse` and `plugins/`.
- Every case fails with category `fdr_error` and the reason `could not start
  FDR executable refines`: `FDR_REFINES` is unset and `refines` is not on
  `PATH`, or `FDR_REFINES` is wrong. Set it to the `refines` executable in
  FDR's `bin/` directory.
- Every case fails with category `csp_generator_error` and the reason `could
  not start RoboTool CSP generator`: there is no `java` on `PATH`. See
  [Prerequisites](#prerequisites).

### The project is not found

Both binaries resolve PROJECT, and its default `testsuite-project`, relative
to the current directory. If `testsuite-runner` stops with `No such file or
directory`, run it from the directory that contains the project, or pass the
project's path.

### An old plug-in jar is still installed

An earlier build of the plug-in was called
`org.robocalc.robocase.diagrams.jar`. If RoboTool's `dropins/` directory still
contains it, both the old and the new plug-in load, and the old one does not
read the `-Dtestsuitegenerator.*` properties. Remove it and restart RoboTool
with `-clean`:

```sh
rm -f /path/to/RoboTool/dropins/org.robocalc.robocase.diagrams.jar
```

### The plug-in stops at "Saving Sirius session"

When RoboTool opens a modelling project that has a `representations.aird`, it
may ask which representations to open. While that dialog is waiting, it holds
the workspace, so the plug-in's final save cannot run: the log ends with
`Saving Sirius session` and nothing more happens. Answer the dialog (choosing
no representations is fine) and the save completes. Run the plug-in on a
machine where you can see and answer RoboTool's window.

### The RoboTool workspace is stale

- The plug-in reports `Workspace project testsuite-project points to …,
  expected …`: the workspace already has a project of that name at another
  location. Delete it from the workspace (select it in Model Explorer, choose
  **Edit → Delete**, and leave **Delete project contents on disk**
  unchecked), or use a fresh workspace directory with `-data`.
- RoboTool shows old models or diagrams after regeneration: select the project
  and press F5 to refresh it, and refresh the affected diagrams with the
  plug-in.

## Repository layout

| Path | Purpose |
| --- | --- |
| `Cargo.toml` | Cargo workspace of the three crates below. |
| `crates/testsuite-generator/` | Library and `testsuite-generator` binary: reads the input, decides each case's context and writes the testsuite project. It runs no subprocesses. |
| `crates/robotool-harness/` | Library: runs headless RoboTool and FDR and rewrites RoboTool's generated CSP. It knows nothing about the case data. |
| `crates/testsuite-runner/` | Library and `testsuite-runner` binary: runs the oracle checks with RoboTool and FDR (`compile`, `prepare-fdr`, `verify`, `report-markdown`), with the failure attribution. |
| `crates/*/src/tests/` | Each crate's self-tests; `crates/testsuite-runner/src/tests/data/` holds the [example input](#example-input). |
| `diagram-plugin/` | The RoboTool start-up plug-in that creates and arranges native diagrams. |
| `docs/` | Further documentation, listed below. |

The plug-in build output and Rust build output are excluded from Git.

## Further reading

- [Test case reference](docs/test-case-reference.md): authoring test case
  input, the supported constructs, typed and shared fixtures, and timed
  observations.
- [Architecture](docs/architecture.md): how the crates fit together, and where
  the rules that decide each case's context live.
- [Oracle findings](docs/robotool-bugs.md): RoboTool defects and limitations
  that can affect the checks, with reproductions.
- [Diagram plug-in](diagram-plugin/README.md): building and running the
  plug-in.
- [robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite):
  the ExpressionTests testsuite, its diagrams and its verification results.
- [robochart-testsuite-mutation](https://github.com/twright/robochart-testsuite-mutation):
  the source-mutation tool that tests the checks, and its campaign's claims.
