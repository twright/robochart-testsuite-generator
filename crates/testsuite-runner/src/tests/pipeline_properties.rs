//! Property tests of the whole pipeline: generation through the
//! `testsuite-generator` library (the binary's `generate`, in process), and
//! verification through the built `testsuite-runner` binary.
//!
//! `no_row_is_dropped` runs the generator on random CSV files. The RoboTool
//! tests are ignored by default because they run RoboTool and FDR; run them with
//! `ROBOTOOL_HOME=… FDR_REFINES=… cargo test --release -p testsuite-runner -- --ignored`.

use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use testsuite_generator::test_rows::{ExpressionRow, Spec, arb_expression_row, arb_spec};

/// The largest finite domain the RoboTool test allows for any one type.
const TRACTABLE: u64 = 1_000;
const RUNNER: &str = env!("CARGO_BIN_EXE_testsuite-runner");
static DIRECTORIES: AtomicUsize = AtomicUsize::new(0);

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "robochart-pipeline-{label}-{}-{}",
        std::process::id(),
        DIRECTORIES.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

type Columns = Vec<(&'static str, String)>;

/// Write rows as a CSV file whose header is the first row's column names.
fn write_csv(path: &Path, header: &[&str], rows: &[Columns]) {
    let mut writer = csv::Writer::from_path(path).unwrap();
    writer.write_record(header).unwrap();
    for row in rows {
        let values: BTreeMap<&str, &String> = row.iter().map(|(k, v)| (*k, v)).collect();
        writer
            .write_record(header.iter().map(|h| values.get(h).map_or("", |v| v.as_str())))
            .unwrap();
    }
    writer.flush().unwrap();
}

fn read_csv(path: &Path) -> Vec<BTreeMap<String, String>> {
    let mut reader = csv::Reader::from_path(path).unwrap();
    let headers = reader.headers().unwrap().clone();
    reader
        .records()
        .map(|r| {
            let r = r.unwrap();
            headers.iter().map(str::to_owned).zip(r.iter().map(str::to_owned)).collect()
        })
        .collect()
}

const STATEMENT_HEADER: [&str; 18] = [
    "id", "feature", "robochart", "inputs", "python_output", "types", "observe", "expected_status",
    "datatypes", "functions", "events", "input_events", "driver", "iteration",
    "function_definitions", "notes", "domains", "constants",
];
const EXPRESSION_HEADER: [&str; 5] = ["id", "feature", "robochart", "inputs", "python_output"];

/// Write a data directory with the given statement and expression rows.
fn data_directory(statements: &[Columns], expressions: &[Columns]) -> PathBuf {
    let data = scratch("data");
    write_csv(&data.join("statement_cases.csv"), &STATEMENT_HEADER, statements);
    write_csv(&data.join("cases.csv"), &EXPRESSION_HEADER, expressions);
    data
}

/// Input rows copied from the ExpressionTests input (see `data/README.md`
/// beside this file): a directory source with `cases.csv` and
/// `statement_cases.csv`, and a separate expression CSV file.
fn vendored_data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/data")
}

/// Generate a project from `source`, as `testsuite-generator SOURCE PROJECT IDS…` does.
fn generate(source: &Path, project: &Path, ids: &[&str]) -> Result<(), String> {
    testsuite_generator::generate(source, project, ids.iter().map(|id| id.to_string()).collect())
        .map_err(|error| error.to_string())
}

fn runner(args: &[&Path], env: &[(&str, String)]) -> std::process::Output {
    let mut command = Command::new(RUNNER);
    command.args(args);
    for (k, v) in env {
        command.env(k, v);
    }
    command.output().expect("run the testsuite-runner binary")
}

/// A row made invalid in a way the generator must reject.
fn break_statement(mut columns: Columns, fault: u8) -> Columns {
    for (name, value) in &mut columns {
        match (fault, *name) {
            (0, "expected_status") => *value = "maybe".into(),
            (1, "observe") => *value = "[]".into(),
            (2, "types") => *value = "{}".into(),
            _ => {}
        }
    }
    columns
}

fn break_expression(mut columns: Columns) -> Columns {
    for (name, value) in &mut columns {
        if *name == "python_output" {
            *value = "{}".into();
        }
    }
    columns
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Every input row becomes exactly one generated case or exactly one
    /// recorded generation failure, and exactly the rows made faulty fail.
    /// Each generated case has its model and check script, and the project's
    /// source snapshot equals the input.
    #[test]
    fn no_row_is_dropped(
        statements in prop::collection::vec((arb_spec(), prop::option::of(0u8..3)), 1..5),
        expressions in prop::collection::vec((arb_expression_row(), any::<bool>()), 0..4),
    ) {
        let mut faulty = BTreeSet::new();
        let statement_rows: Vec<Columns> = statements
            .iter()
            .enumerate()
            .map(|(i, (spec, fault))| {
                let id = format!("prop/s{i}");
                let columns = spec.columns(&id);
                match fault {
                    Some(f) => { faulty.insert(id); break_statement(columns, *f) }
                    None => columns,
                }
            })
            .collect();
        let expression_rows: Vec<Columns> = expressions
            .iter()
            .enumerate()
            .map(|(i, (row, broken))| {
                let id = format!("prop/e{i}");
                let columns = row.columns(&id);
                if *broken { faulty.insert(id); break_expression(columns) } else { columns }
            })
            .collect();
        let data = data_directory(&statement_rows, &expression_rows);
        let project = scratch("project");
        let output = generate(&data, &project, &[]);
        prop_assert!(output.is_ok(), "{:?}", output);

        let generated: Vec<BTreeMap<String, String>> = read_csv(&project.join("case-index.csv"));
        let failed: Vec<BTreeMap<String, String>> = read_csv(&project.join("generation-failures.csv"));
        let generated_ids: Vec<&String> = generated.iter().map(|r| &r["id"]).collect();
        let failed_ids: BTreeSet<&String> = failed.iter().map(|r| &r["id"]).collect();
        let all: BTreeSet<String> = statement_rows.iter().chain(&expression_rows).map(|r| r[0].1.clone()).collect();

        prop_assert_eq!(generated_ids.len() + failed.len(), all.len(), "a row was dropped or duplicated");
        let seen: BTreeSet<String> = generated_ids.iter().map(|s| s.to_string()).chain(failed_ids.iter().map(|s| s.to_string())).collect();
        prop_assert_eq!(&seen, &all);
        prop_assert_eq!(failed_ids.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(), faulty);
        for row in &generated {
            let model = std::fs::read_to_string(project.join(&row["model"])).unwrap();
            let package = format!("package {}", row["diagram"]);
            prop_assert!(model.contains(&package), "{} lacks {:?}", row["model"], package);
            let check = project.join("csp-gen").join(format!("check_{}.csp", row["diagram"]));
            prop_assert!(check.is_file(), "missing {}", check.display());
        }
        for file in ["cases.csv", "statement_cases.csv"] {
            prop_assert_eq!(read_csv(&project.join("source-data").join(file)), read_csv(&data.join(file)));
        }
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&project);
    }
}

/// A statement row whose body does nothing, with the expected final state
/// that doing nothing produces and no output events.
fn skip_row(spec: &Spec) -> Spec {
    let mut row = spec.clone();
    row.body = "skip".into();
    let state = spec.final_state_of_skip();
    row.state = spec.observe.iter().map(|n| (n.clone(), state[n].clone())).collect();
    row.trace = Vec::new();
    row
}

/// An expression row whose expression is one of its inputs, expecting that
/// input's value.
fn identity_row(row: &ExpressionRow) -> ExpressionRow {
    let name = row.inputs.keys().find(|n| *n != "domain").cloned().expect("an input");
    ExpressionRow {
        output: row.inputs[&name].clone(),
        robochart: name,
        inputs: row.inputs.clone(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(3),
        max_shrink_iters: 16,
        ..ProptestConfig::default()
    })]

    /// RoboTool translates every generated model, and FDR confirms each
    /// row's expectation when that expectation follows from the fixture
    /// alone: a statement body of `skip`, or an expression that is one of its
    /// inputs. Rows are limited to types whose finite domains have at most
    /// `TRACTABLE` values; larger ones exceed FDR's time limit without
    /// explicit value domains.
    #[test]
    #[ignore = "runs RoboTool and FDR; needs ROBOTOOL_HOME and FDR_REFINES"]
    fn robotool_translates_and_fdr_confirms_trivial_rows(
        statements in prop::collection::vec(arb_spec().prop_filter("tractable", |s| s.is_tractable(TRACTABLE)), 1..5),
        expressions in prop::collection::vec(arb_expression_row(), 0..4),
    ) {
        let robotool = std::env::var("ROBOTOOL_HOME").expect("set ROBOTOOL_HOME to run this test");
        let statement_rows: Vec<Columns> = statements.iter().enumerate()
            .map(|(i, s)| skip_row(s).columns(&format!("prop/s{i}"))).collect();
        let expression_rows: Vec<Columns> = expressions.iter().enumerate()
            .map(|(i, e)| identity_row(e).columns(&format!("prop/e{i}"))).collect();
        let data = data_directory(&statement_rows, &expression_rows);
        let project = scratch("robotool");
        let generated = generate(&data, &project, &[]);
        prop_assert!(generated.is_ok(), "{:?}", generated);
        let mut env = vec![("ROBOTOOL_HOME", robotool)];
        if let Ok(refines) = std::env::var("FDR_REFINES") {
            env.push(("FDR_REFINES", refines));
        }
        runner(&[Path::new("verify"), &project], &env);
        let results = read_csv(&project.join("verification-results.csv"));
        let failures: Vec<String> = results
            .iter()
            .filter(|r| r["category"] != "verified")
            .map(|r| format!("{} {}: {}", r["id"], r["category"], r["reason/log"]))
            .collect();
        let expected = statement_rows.len() + expression_rows.len();
        if failures.is_empty() && results.len() == expected {
            let _ = std::fs::remove_dir_all(&data);
            let _ = std::fs::remove_dir_all(&project);
        }
        prop_assert_eq!(results.len(), expected, "results in {}", project.display());
        prop_assert!(failures.is_empty(), "project kept at {}:\n{}", project.display(), failures.join("\n"));
    }
}


/// Use a recorded Python observation, then corrupt only the FDR
/// check to establish that the graph context exposes a wrong observation.
#[test]
#[ignore = "runs RoboTool and FDR; needs ROBOTOOL_HOME and FDR_REFINES"]
fn lambda_graph_context_distinguishes_wrong_values() {
    let data = vendored_data();
    let project = scratch("lambda");
    generate(&data, &project, &["lambda_False/000"]).unwrap();
    runner(&[Path::new("verify"), &project], &[]);
    let results = read_csv(&project.join("verification-results.csv"));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["category"], "verified", "project: {}: {results:?}", project.display());
    assert!(results[0]["reason/log"].contains("same verdict"), "{results:?}");
    let check = project.join("csp-gen/check_expression_lambda_False_000.csp");
    let original = std::fs::read_to_string(&check).unwrap();
    assert_eq!(original.matches("output.out.{ 0}").count(), 1);
    std::fs::write(&check, original.replace("output.out.{ 0}", "output.out.{ 1}")).unwrap();
    runner(&[Path::new("verify"), &project], &[]);
    let results = read_csv(&project.join("verification-results.csv"));
    assert_eq!(results[0]["category"], "fdr_mismatch", "project: {}: {results:?}", project.display());
    assert!(results[0]["reason/log"].contains("same verdict"), "{results:?}");
    std::fs::remove_dir_all(project).unwrap();
}

/// Keep the recorded fixture and expected value, and use a source mutation as
/// a sizing control. For a = -3, b = -3, (-a) + b has intermediate value 3 even
/// though its final value is 0. Neither is used as a new recorded expectation.
#[test]
#[ignore = "runs RoboTool and FDR; needs ROBOTOOL_HOME and FDR_REFINES"]
fn integer_bounds_include_intermediate_negation() {
    let data = vendored_data();
    let project = scratch("bounds");
    generate(&data, &project, &["add/000"]).unwrap();
    let path = project.join("cases/expressions/add/000.rct");
    let model = std::fs::read_to_string(&path).unwrap();
    assert_eq!(model.matches("output ! (a + b)").count(), 1);
    std::fs::write(&path, model.replace("output ! (a + b)", "output ! ((-a) + b)")).unwrap();
    let compiled = runner(&[Path::new("compile"), &project], &[]);
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    let prepared = runner(&[Path::new("prepare-fdr"), &project], &[]);
    let stdout = String::from_utf8_lossy(&prepared.stdout);
    assert!(prepared.status.success() && stdout.contains("Extended core_int"), "{stdout}");
    let instantiations = std::fs::read_to_string(project.join("csp-gen/instantiations.csp")).unwrap();
    assert!(instantiations.contains("nametype core_int = { -6..3}"), "{instantiations}");
    // Runtime arithmetic remains exactly RoboTool's generated definition.
    assert!(instantiations.contains("Neg(e1,T) = if member(-e1,T) then -e1 else e1"), "{instantiations}");
    std::fs::remove_dir_all(project).unwrap();
}

/// Recorded ExpressionTests observations exercise context encodings without
/// introducing expectations from a substitute evaluator.
#[test]
#[ignore = "runs RoboTool and FDR; needs ROBOTOOL_HOME and FDR_REFINES"]
fn structured_values_and_finite_domains_match_recorded_observations() {
    let project = scratch("structured-context");
    let data = vendored_data();
    let ids = ["tuple/000", "vector/000", "matrix/000", "comprehension_False_False/000", "comprehension_False_True/002", "comprehension_True_True/004", "range_False_True_False/003", "divide/000"];
    generate(&data, &project, &ids).unwrap();
    let output = Command::new(RUNNER).arg("verify").arg(&project).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let results = read_csv(&project.join("verification-results.csv"));
    assert_eq!(results.len(), ids.len());
    for row in results { assert_eq!(row["category"], "verified", "{}", row["id"]); }
    // Corrupt only a temporary expectation within the same finite type.
    let wrong_project = scratch("wrong-vector-context");
    generate(&data, &wrong_project, &["vector/000"]).unwrap();
    let check = wrong_project.join("csp-gen/check_expression_vector_000.csp");
    let original = std::fs::read_to_string(&check).unwrap();
    let corrupt = original.replace("output.out!(1,2)", "output.out!(2,2)");
    assert_ne!(original, corrupt);
    std::fs::write(check, corrupt).unwrap();
    let output = Command::new(RUNNER).arg("verify").arg(&wrong_project).output().unwrap();
    assert!(!output.status.success());
    let results = read_csv(&wrong_project.join("verification-results.csv"));
    let vector = results.iter().find(|r| r["id"] == "vector/000").unwrap();
    assert_eq!(vector["category"], "fdr_mismatch");
    assert!(vector["reason/log"].contains("same verdict"));
    std::fs::remove_dir_all(wrong_project).unwrap();
    std::fs::remove_dir_all(project).unwrap();
}

#[test]
/// A single expression CSV file is a source of its own: generating from it
/// reads neither the `cases.csv` nor the `statement_cases.csv` beside it.
fn supplementary_expression_csv_is_an_independent_corpus() {
    let data = vendored_data();
    let original = read_csv(&data.join("cases.csv"));
    let supplementary = read_csv(&data.join("supplementary_expression_cases.csv"));
    assert!(!original.is_empty() && !supplementary.is_empty());
    assert!(!read_csv(&data.join("statement_cases.csv")).is_empty());
    let original_ids: BTreeSet<&str> = original.iter().map(|r| r["id"].as_str()).collect();
    assert!(supplementary.iter().all(|r| !original_ids.contains(r["id"].as_str())));
    let project = scratch("supplementary-corpus");
    generate(&data.join("supplementary_expression_cases.csv"), &project, &[]).unwrap();
    let generated = read_csv(&project.join("case-index.csv"));
    assert_eq!(generated.len(), supplementary.len());
    assert!(generated.iter().all(|r| r["kind"] == "expression"));
    assert_eq!(read_csv(&project.join("source-data/cases.csv")), supplementary);
    assert!(read_csv(&project.join("source-data/statement_cases.csv")).is_empty());
    std::fs::remove_dir_all(project).unwrap();
}

#[test]
#[ignore = "runs RoboTool and FDR; needs ROBOTOOL_HOME and FDR_REFINES"]
fn core_sequences_work_in_expression_and_statement_contexts() {
    let data = vendored_data();
    let project = scratch("core-sequence-context");
    let ids = ["empty_sequence/000", "sequence/000", "concat/005", "caret_concat/005", "sequence_size/pass", "sequence_concat/pass", "sequence_index/pass", "sequence/pass", "sequence/mismatch"];
    generate(&data, &project, &ids).unwrap();
    for row in read_csv(&project.join("case-index.csv")) {
        let source = std::fs::read_to_string(project.join(&row["model"])).unwrap();
        assert!(source.contains("import sequence_toolkit::*"), "{}", row["id"]);
        assert!(!source.contains("function size("), "{}", row["id"]);
    }
    let output = Command::new(RUNNER).arg("verify").arg(&project).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let results = read_csv(&project.join("verification-results.csv"));
    assert_eq!(results.len(), ids.len());
    assert!(results.iter().all(|r| r["status"] == "pass" && r["category"] == "verified"));
    std::fs::remove_dir_all(project).unwrap();
}

#[test]
#[ignore = "runs RoboTool and FDR; needs ROBOTOOL_HOME and FDR_REFINES"]
fn timed_clock_contexts_sample_elapsed_ticks_and_detect_wrong_observations() {
    // These are harness diagnostics with elapsed time fixed by a CSP driver,
    // not new translator observations or additions to the expression corpus.
    let data = scratch("timed-data");
    let project = scratch("timed-contexts");
    let header = ["id", "feature", "robochart", "inputs", "python_output", "timing"];
    let diagnostics = [
        ("clock_zero", "since(timer)", 0, 0, 5),
        ("clock_three", "since(timer)", 3, 3, 5),
        ("clock_boundary", "since(timer)", 5, 5, 5),
        ("clock_large", "since(timer)", 12, 12, 12),
        ("state_three", "sinceEntry(observing)", 3, 3, 5),
        ("clock_wrong", "since(timer)", 3, 2, 5),
    ];
    let mut rows: Vec<Columns> = diagnostics.iter().map(|(name, expression, ticks, value, bound)| vec![
        ("id", format!("{name}/000")), ("feature", name.to_string()),
        ("robochart", expression.to_string()), ("inputs", "{}".into()),
        ("python_output", format!("{{\"value\":{value}}}")),
        ("timing", format!("{{\"ticks\":{ticks},\"max_ticks\":{bound}}}")),
    ]).collect();
    rows.push(vec![("id", "untimed/000".into()), ("feature", "untimed".into()), ("robochart", "1".into()), ("inputs", "{}".into()), ("python_output", "{\"value\":1}".into()), ("timing", String::new())]);
    write_csv(&data.join("cases.csv"), &header, &rows);
    write_csv(&data.join("statement_cases.csv"), &STATEMENT_HEADER, &[]);
    generate(&data, &project, &[]).unwrap();
    for row in read_csv(&project.join("case-index.csv")) {
        let timed = row["id"] != "untimed/000";
        assert_eq!(row["timed"], timed.to_string());
        let directory = if timed { "csp-gen/timed" } else { "csp-gen" };
        assert!(project.join(directory).join(format!("check_{}.csp", row["diagram"])).is_file());
    }
    let output = Command::new(RUNNER).arg("verify").arg(&project).output().unwrap();
    assert!(!output.status.success(), "the deliberately wrong observation must fail");
    let results = read_csv(&project.join("verification-results.csv"));
    assert_eq!(results.len(), rows.len());
    for row in results {
        let (status, category) = if row["id"] == "clock_wrong/000" { ("translator_defect", "fdr_mismatch") } else { ("pass", "verified") };
        assert_eq!(row["status"], status, "{:?}", row);
        assert_eq!(row["category"], category, "{:?}", row);
        assert!(row["reason/log"].contains("same verdict"), "{:?}", row);
    }
    std::fs::remove_dir_all(data).unwrap();
    std::fs::remove_dir_all(project).unwrap();
}
