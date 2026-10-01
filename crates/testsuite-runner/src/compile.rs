//! `compile`: RoboTool's CSP generator over a whole project, with a
//! per-case diagnosis when the bulk run fails.

use crate::safe_log_name;
use robotool_harness::{generator_output_log, run_csp_generator};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Output;
use testsuite_generator::{IndexRow, Result, project_file};

pub fn compile_csp(project: &Path, robotool_home: &Path) -> Result<()> {
    let plugins = robotool_home.join("plugins");
    if !plugins.is_dir() {
        return Err(format!(
            "RoboTool plugins directory does not exist: {}",
            plugins.display()
        )
        .into());
    }
    let csp_output = project.join("csp-gen");
    fs::create_dir_all(&csp_output)?;
    let report_path = csp_output.join("generator-failures.csv");
    let logs_path = csp_output.join("generator-diagnostics");
    if logs_path.exists() {
        fs::remove_dir_all(&logs_path)?;
    }
    let bulk_log_path = csp_output.join("generator-bulk.log");
    if bulk_log_path.exists() {
        fs::remove_file(&bulk_log_path)?;
    }

    let mut report = csv::Writer::from_path(&report_path)?;
    report.write_record(["id", "model", "status", "category", "outcome", "log"])?;

    let output = run_csp_generator(project, &plugins)?;
    if !output.status.success() {
        fs::create_dir_all(&logs_path)?;
        fs::write(&bulk_log_path, generator_output_log(&output))?;

        let mut failures = Vec::new();
        let mut cases = csv::Reader::from_path(project.join("case-index.csv"))?;
        for (index, result) in cases.deserialize::<IndexRow>().enumerate() {
            let case = result?;
            let log_name = format!("{:04}_{}.log", index + 1, safe_log_name(&case.diagram));
            let log_path = logs_path.join(&log_name);
            let outcome = diagnose_test_case(project, robotool_home, &case, index, &log_path);
            if outcome != "passed" {
                let relative_log = Path::new("generator-diagnostics").join(log_name);
                let category = if fs::read_to_string(&log_path)
                    .is_ok_and(|log| log.contains("Generator failed with exception"))
                {
                    "csp_generator_exception"
                } else if outcome.starts_with("generator_failed") {
                    "robochart_validation_error"
                } else {
                    "diagnostic_error"
                };
                report.write_record([
                    case.id.as_str(),
                    case.model.as_str(),
                    "failure",
                    category,
                    outcome.as_str(),
                    &relative_log.to_string_lossy(),
                ])?;
                failures.push(case.id);
            }
        }

        if failures.is_empty() {
            report.write_record([
                "<bulk>",
                "",
                "failure",
                "csp_generator_bulk_failure",
                "bulk_failure_not_reproduced_per_case",
                "generator-bulk.log",
            ])?;
        }
        report.flush()?;

        let details = if failures.is_empty() {
            "No single model reproduced the bulk failure".to_owned()
        } else {
            format!("failing cases: {}", failures.join(", "))
        };
        return Err(format!(
            "RoboTool CSP generator failed with {}; {details}. See {} and {}",
            output.status,
            report_path.display(),
            bulk_log_path.display()
        )
        .into());
    }
    report.flush()?;
    println!(
        "Compiled RoboChart models to CSP in {}",
        csp_output.display()
    );
    Ok(())
}

fn diagnose_test_case(
    project: &Path,
    robotool_home: &Path,
    case: &IndexRow,
    index: usize,
    log_path: &Path,
) -> String {
    let temp_project = env::temp_dir().join(format!(
        "robochart-csp-diagnostic-{}-{}",
        std::process::id(),
        index + 1
    ));
    let result = (|| -> Result<Output> {
        let relative_model = Path::new(&case.model);
        if relative_model.is_absolute()
            || relative_model
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(format!("model path is not project-relative: {}", case.model).into());
        }
        if temp_project.exists() {
            fs::remove_dir_all(&temp_project)?;
        }
        fs::create_dir_all(temp_project.join(relative_model.parent().unwrap_or(Path::new("."))))?;
        fs::write(temp_project.join(".project"), project_file())?;
        fs::copy(
            project.join(relative_model),
            temp_project.join(relative_model),
        )?;

        run_csp_generator(&temp_project, &robotool_home.join("plugins")).map_err(Into::into)
    })();

    let (outcome, log) = match result {
        Ok(output) => (
            if output.status.success() {
                "passed".to_owned()
            } else {
                format!("generator_failed ({})", output.status)
            },
            generator_output_log(&output),
        ),
        Err(error) => (
            "project_setup_error".to_owned(),
            format!("Could not set up the temporary project for {}: {error}\n", case.id).into_bytes(),
        ),
    };

    if let Err(error) = fs::remove_dir_all(&temp_project)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "Could not remove temporary project for {}: {error}",
            case.id
        );
    }
    if let Err(error) = fs::write(log_path, log) {
        eprintln!("Could not write diagnostic log for {}: {error}", case.id);
        return "log_write_error".to_owned();
    }
    outcome
}
