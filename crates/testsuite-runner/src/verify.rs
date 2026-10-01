//! `verify`: each test case is compiled and checked in a temporary project of
//! its own, so that one test case cannot affect another's verdict.

use crate::attribution::{attribute_check_failure, attribute_generation_failure};
use crate::prepare::fdr_preparation;
use crate::report::write_markdown_report;
use crate::safe_log_name;
use robotool_harness::{generator_output_log, run_csp_generator};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use testsuite_generator::facts::Snapshot;
use testsuite_generator::{IndexRow, Result, project_file, test_case_csp_dir};

#[derive(Deserialize)]
struct GenerationFailureRow {
    id: String,
    category: String,
    reason: String,
}

struct TestCaseResult {
    id: String,
    status: &'static str,
    category: String,
    reason: String,
    log: Vec<u8>,
}

/// One row of `verification-results.csv`, with the line printed for it.
struct ReportRow {
    record: [String; 4],
    printed: Option<String>,
}

/// A row of the results, in the order in which it is reported. Its log
/// name is fixed before any check runs, so the results do not depend on the
/// order in which parallel checks finish.
enum Job {
    /// A case that could not be generated; its log is written at once.
    Generation { row: GenerationFailureRow, duplicate: bool, relative_log: PathBuf },
    /// A generated case, checked in a temporary project of its own.
    Check { case: IndexRow, index: usize, duplicate: bool, relative_log: PathBuf },
}

/// Verify every test case of `project` with RoboTool (`robotool_home`) and
/// FDR's `refines` executable, and write `verification-results.csv`,
/// `verification-results.md` and `verification-logs/` into the project.
///
/// `jobs` test cases are checked at once (at least one). The results, their
/// order, the log names and the logs' contents do not depend on `jobs`.
pub fn verify(project: &Path, robotool_home: &Path, refines: &Path, jobs: usize) -> Result<()> {
    if !robotool_home.join("plugins").is_dir() {
        return Err(format!(
            "RoboTool plugins directory does not exist: {}",
            robotool_home.join("plugins").display()
        )
        .into());
    }

    let logs_dir = project.join("verification-logs");
    if logs_dir.exists() {
        fs::remove_dir_all(&logs_dir)?;
    }
    fs::create_dir_all(&logs_dir)?;
    let report_path = project.join("verification-results.csv");
    let mut report = csv::Writer::from_path(&report_path)?;
    report.write_record(["id", "status", "category", "reason/log"])?;
    let mut failures = 0usize;
    let mut log_number = 0usize;
    let mut seen_ids = BTreeSet::new();
    let mut log_path = |name: &str| {
        log_number += 1;
        Path::new("verification-logs").join(format!("{:04}_{}.log", log_number, safe_log_name(name)))
    };

    // The source rows, from which each case's FDR preparation is built.
    let snapshot = Snapshot::load(&project.join("source-data"))?;
    let expressions: BTreeMap<String, String> =
        snapshot.expressions.iter().map(|case| (case.id.clone(), case.robochart.clone())).collect();

    let mut work = Vec::new();
    let generation_failures = project.join("generation-failures.csv");
    if generation_failures.is_file() {
        for row in
            csv::Reader::from_path(generation_failures)?.deserialize::<GenerationFailureRow>()
        {
            let row = row?;
            let duplicate = !seen_ids.insert(row.id.clone());
            let relative_log = log_path(&row.id);
            work.push(Job::Generation { row, duplicate, relative_log });
        }
    }
    let mut cases = csv::Reader::from_path(project.join("case-index.csv"))?;
    for (index, record) in cases.deserialize::<IndexRow>().enumerate() {
        let case = record?;
        let duplicate = !seen_ids.insert(case.id.clone());
        let relative_log = log_path(&case.diagram);
        work.push(Job::Check { case, index, duplicate, relative_log });
    }

    let run = |job: &Job| -> Vec<ReportRow> {
        match job {
            Job::Generation { row, duplicate, relative_log } => {
                let mut rows = duplicate_row(&row.id, *duplicate);
                rows.push(generation_failure_row(project, row, relative_log));
                rows
            }
            Job::Check { case, index, duplicate, relative_log } => {
                let mut rows = duplicate_row(&case.id, *duplicate);
                rows.push(check_row(project, robotool_home, refines, &snapshot, &expressions, case, *index, relative_log));
                rows
            }
        }
    };
    // Rows are written in work order as soon as every earlier row is known.
    let mut write = |rows: Vec<ReportRow>| -> Result<()> {
        for row in rows {
            if row.record[1] != "pass" {
                failures += 1;
            }
            report.write_record(&row.record)?;
            report.flush()?;
            if let Some(line) = row.printed {
                println!("{line}");
            }
        }
        Ok(())
    };
    let jobs = jobs.max(1).min(work.len().max(1));
    if jobs == 1 {
        for job in &work {
            write(run(job))?;
        }
    } else {
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| -> Result<()> {
            for _ in 0..jobs {
                let sender = sender.clone();
                let (next, work, run) = (&next, &work, &run);
                scope.spawn(move || {
                    loop {
                        let position = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = work.get(position) else { break };
                        if sender.send((position, run(job))).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let mut pending = BTreeMap::new();
            let mut written = 0;
            for (position, rows) in receiver {
                pending.insert(position, rows);
                while let Some(rows) = pending.remove(&written) {
                    write(rows)?;
                    written += 1;
                }
            }
            Ok(())
        })?;
    }

    let expected_ids = snapshot.test_case_ids();
    for id in expected_ids.difference(&seen_ids) {
        failures += 1;
        report.write_record([
            id.as_str(),
            "failure",
            "coverage_gap",
            "CSV case was not generated or classified",
        ])?;
    }
    for id in seen_ids.difference(&expected_ids) {
        failures += 1;
        report.write_record([
            id.as_str(),
            "failure",
            "unknown_test_case_id",
            "Generated report has no matching source CSV row",
        ])?;
    }
    report.flush()?;
    write_markdown_report(project)?;

    if failures == 0 {
        println!(
            "All test cases passed; results are in {}",
            report_path.display()
        );
        Ok(())
    } else {
        Err(format!(
            "{failures} test case(s) did not pass; see {}",
            report_path.display()
        )
        .into())
    }
}

/// The extra row that reports a test case ID seen before, if it was.
fn duplicate_row(id: &str, duplicate: bool) -> Vec<ReportRow> {
    if !duplicate {
        return Vec::new();
    }
    vec![ReportRow {
        record: [
            id.to_owned(),
            "failure".into(),
            "duplicate_test_case_id".into(),
            "Case appears more than once in generated reports".into(),
        ],
        printed: None,
    }]
}

/// Report a case that could not be generated, and write its log.
fn generation_failure_row(project: &Path, row: &GenerationFailureRow, relative_log: &Path) -> ReportRow {
    let log = format!(
        "Unsupported model input could not be generated.\nCategory: {}\nReason: {}\n",
        row.category, row.reason
    );
    let (status, category, reason) = if let Err(error) = fs::write(project.join(relative_log), &log) {
        (
            "failure",
            "log_write_error",
            format!("{}; could not write log: {error}", row.reason),
        )
    } else {
        let (status, note) = attribute_generation_failure(&row.category);
        (
            status,
            row.category.as_str(),
            format!("{}{note}; log={}", row.reason, relative_log.display()),
        )
    };
    ReportRow { record: [row.id.clone(), status.into(), category.into(), reason], printed: None }
}

/// Check a generated case, attribute it if it did not pass, and write its log.
#[allow(clippy::too_many_arguments)]
fn check_row(
    project: &Path,
    robotool_home: &Path,
    refines: &Path,
    snapshot: &Snapshot,
    expressions: &BTreeMap<String, String>,
    case: &IndexRow,
    index: usize,
    relative_log: &Path,
) -> ReportRow {
    let mut result = verify_test_case(project, robotool_home, refines, snapshot, case, index);
    if result.status == "failure" {
        let model = fs::read_to_string(project.join(&case.model)).unwrap_or_default();
        let expression = expressions.get(&case.id).map(String::as_str);
        if let Some((status, note)) = attribute_check_failure(
            &result.category,
            &String::from_utf8_lossy(&result.log),
            &case.model,
            &model,
            expression,
        ) {
            result.status = status;
            result.reason.push_str(&note);
        }
    }
    let reason = match fs::write(project.join(relative_log), &result.log) {
        Ok(()) => format!("{}; log={}", result.reason, relative_log.display()),
        Err(error) => {
            result.status = "failure";
            result.category = "log_write_error".into();
            format!("{}; could not write log: {error}", result.reason)
        }
    };
    let printed = format!("{}: {} ({})", result.id, result.status, result.category);
    ReportRow {
        record: [result.id, result.status.into(), result.category, reason],
        printed: Some(printed),
    }
}

fn verify_test_case(
    project: &Path,
    robotool_home: &Path,
    refines: &Path,
    snapshot: &Snapshot,
    case: &IndexRow,
    index: usize,
) -> TestCaseResult {
    let temp_project = env::temp_dir().join(format!(
        "testsuite-runner-verify-{}-{}",
        std::process::id(),
        index + 1
    ));
    let mut result = (|| {
        let relative_model = Path::new(&case.model);
        if relative_model.is_absolute()
            || relative_model
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return failed_result(
                &case.id,
                "invalid_model_path",
                format!("model path is not project-relative: {}", case.model),
                Vec::new(),
            );
        }

        if temp_project.exists()
            && let Err(error) = fs::remove_dir_all(&temp_project)
        {
            return failed_result(
                &case.id,
                "project_setup_error",
                format!("could not clear temporary project: {error}"),
                Vec::new(),
            );
        }
        let model_path = temp_project.join(relative_model);
        if let Some(parent) = model_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            return failed_result(
                &case.id,
                "project_setup_error",
                format!("could not create temporary model directory: {error}"),
                Vec::new(),
            );
        }
        if let Err(error) = fs::write(temp_project.join(".project"), project_file()) {
            return failed_result(
                &case.id,
                "project_setup_error",
                format!("could not write temporary project metadata: {error}"),
                Vec::new(),
            );
        }
        if let Err(error) = fs::copy(project.join(relative_model), &model_path) {
            return failed_result(
                &case.id,
                "project_setup_error",
                format!("could not copy model {}: {error}", case.model),
                Vec::new(),
            );
        }

        let check_name = format!("check_{}.csp", case.diagram);
        let check_source = test_case_csp_dir(project, case.timed).join(&check_name);
        let check_destination = test_case_csp_dir(&temp_project, case.timed).join(&check_name);
        if let Err(error) = fs::create_dir_all(check_destination.parent().unwrap()) {
            return failed_result(
                &case.id,
                "project_setup_error",
                format!("could not create temporary CSP directory: {error}"),
                Vec::new(),
            );
        }
        if let Err(error) = fs::copy(&check_source, &check_destination) {
            return failed_result(
                &case.id,
                "missing_check_csp",
                format!("could not copy {}: {error}", check_source.display()),
                Vec::new(),
            );
        }

        let output = match run_csp_generator(&temp_project, &robotool_home.join("plugins")) {
            Ok(output) => output,
            Err(error) => {
                return failed_result(
                    &case.id,
                    "csp_generator_error",
                    format!("could not start RoboTool CSP generator: {error}"),
                    Vec::new(),
                );
            }
        };
        let generator_log = generator_output_log(&output);
        if !output.status.success() {
            let generator_text = String::from_utf8_lossy(&generator_log);
            let category = if generator_text.contains("Generator failed with exception")
                || (generator_text.contains("Exception") && generator_text.contains("Generator"))
            {
                "csp_generator_exception"
            } else if generator_text.to_ascii_lowercase().contains("unsupported") {
                "unsupported_input"
            } else {
                "robochart_validation_error"
            };
            return failed_result(
                &case.id,
                category,
                format!("RoboTool generator exited with {}", output.status),
                generator_log,
            );
        }

        // The case is prepared from its own rows, as if its project held
        // only them.
        let preparation = fdr_preparation(&snapshot.only(&case.id)).and_then(|preparation| {
            robotool_harness::prepare(&temp_project, &preparation, refines)?;
            Ok(preparation)
        });
        let preparation = match preparation {
            Ok(preparation) => preparation,
            Err(error) => {
                let mut log = generator_log;
                log.extend_from_slice(format!("\nprepare_fdr failed: {error}\n").as_bytes());
                return failed_result(&case.id, "fdr_prepare_error", error.to_string(), log);
            }
        };

        match run_fdr(refines, &temp_project, case, &generator_log) {
            Ok(result) => recheck_with_wider_bounds(refines, &temp_project, case, preparation.clock_max, &generator_log, result),
            Err((category, reason, log)) => failed_result(&case.id, category, reason, log),
        }
    })();

    if let Err(error) = fs::remove_dir_all(&temp_project)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        result.log.extend_from_slice(
            format!("\nCould not remove temporary project: {error}\n").as_bytes(),
        );
    }
    let replacements = [
        (temp_project.as_path(), "<test-case-project>"),
        (robotool_home, "<ROBOTOOL_HOME>"),
        (project, "<testsuite-project>"),
    ];
    result.reason = replace_local_paths(&result.reason, &replacements);
    result.log =
        replace_local_paths(&String::from_utf8_lossy(&result.log), &replacements).into_bytes();
    result
}

fn replace_local_paths(value: &str, replacements: &[(&Path, &str)]) -> String {
    replacements
        .iter()
        .fold(value.to_owned(), |text, (path, label)| {
            text.replace(path.to_string_lossy().as_ref(), label)
        })
}

fn run_fdr(
    refines: &Path,
    project: &Path,
    case: &IndexRow,
    generator_log: &[u8],
) -> std::result::Result<TestCaseResult, (&'static str, String, Vec<u8>)> {
    let generated = test_case_csp_dir(project, case.timed)
        .join(format!("{}.csp", case.diagram));
    if !generated.is_file() {
        let reason = format!("generator did not produce {}", generated.display());
        let mut log = generator_log.to_vec();
        log.extend_from_slice(format!("\n{reason}\n").as_bytes());
        return Err(("missing_generated_csp", reason, log));
    }
    let check_file = format!("check_{}.csp", case.diagram);
    let (stdout, log) = robotool_harness::run_fdr_check(
        refines,
        &test_case_csp_dir(project, case.timed),
        &check_file,
        generator_log,
    )?;

    let actual = if stdout.contains("expected [FD= model: Passed") {
        "pass"
    } else if stdout.contains("expected [FD= model: Failed") {
        "mismatch"
    } else {
        return Err((
            "fdr_no_result",
            "FDR ended without a refinement result".into(),
            log,
        ));
    };
    let okay = actual == case.expected_status;
    let result = if okay {
        TestCaseResult {
            id: case.id.clone(),
            status: "pass",
            category: "verified".into(),
            reason: format!("FDR reported {actual}, expected {}", case.expected_status),
            log,
        }
    } else {
        TestCaseResult {
            id: case.id.clone(),
            // FDR refuting a recorded observation of a supported case is
            // evidence of a translator defect, not of an unsupported input.
            status: if case.expected_status == "pass" { "translator_defect" } else { "failure" },
            category: "fdr_mismatch".into(),
            reason: format!("FDR reported {actual}, expected {}", case.expected_status),
            log,
        }
    };
    Ok(result)
}

/// Rerun a case's check with wider integer bounds. A verdict is only
/// meaningful if no value was clamped by RoboTool's saturating operators, and
/// then it does not depend on the bounds; a verdict that changes when the
/// bounds widen is reported as `bounds_too_narrow`.
fn recheck_with_wider_bounds(
    refines: &Path,
    project: &Path,
    case: &IndexRow,
    clock_max: u32,
    generator_log: &[u8],
    first: TestCaseResult,
) -> TestCaseResult {
    let widened = match robotool_harness::widen_bounds(project, &case.model, clock_max) {
        Ok(widened) => widened,
        Err(error) => {
            let reason = format!("{}; bounds not rechecked: {error}", first.reason);
            return TestCaseResult { reason, ..first };
        }
    };
    let mut log = first.log.clone();
    log.extend_from_slice(format!("\n--- recheck with {widened} ---\n").as_bytes());
    match run_fdr(refines, project, case, generator_log) {
        Ok(second) if second.category == first.category => TestCaseResult {
            reason: format!("{}; same verdict with {widened}", first.reason),
            log: [log, second.log].concat(),
            ..first
        },
        Ok(second) => TestCaseResult {
            status: "failure",
            category: "bounds_too_narrow".into(),
            reason: format!(
                "{} with the case bounds, but {} with {widened}",
                first.reason, second.reason
            ),
            log: [log, second.log].concat(),
            ..first
        },
        Err((category, _, second_log)) => TestCaseResult {
            reason: format!("{}; bounds not rechecked ({category} with {widened})", first.reason),
            log: [log, second_log].concat(),
            ..first
        },
    }
}

fn failed_result(id: &str, category: &str, reason: String, log: Vec<u8>) -> TestCaseResult {
    TestCaseResult {
        id: id.to_owned(),
        status: "failure",
        category: category.to_owned(),
        reason,
        log,
    }
}
