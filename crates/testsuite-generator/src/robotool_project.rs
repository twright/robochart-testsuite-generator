//! Writing a review project: models, CSP checks, the case index, the
//! generation failures and the source-data snapshot.

use crate::expression::Expression;
use crate::kind::Generation;
use crate::shared_fixtures::{self, SharedFixtures};
use crate::statement::Statement;
use crate::{Result, package_name};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// One row of a project's `case-index.csv`.
#[derive(Deserialize)]
pub struct IndexRow {
    #[serde(default)]
    pub timed: bool,
    pub id: String,
    pub model: String,
    pub diagram: String,
    pub expected_status: String,
}

/// Generate a review project in `destination` from a data directory (the
/// original corpus, with its shared fixtures) or a single expression CSV (an
/// independent corpus). `requested` is empty or `--all` for every row,
/// `--statements` for the statement rows only, or a list of case IDs.
pub fn generate(source: &Path, destination: &Path, requested: Vec<String>) -> Result<()> {
    // A CSV path runs an independent expression corpus. Project snapshots
    // retain the standard filenames used by preparation and verification.
    let expression_source = if source.is_file() { source.to_path_buf() } else { source.join("cases.csv") };
    let statement_source = if source.is_file() { None } else { Some(source.join("statement_cases.csv")) };
    let statements_only = requested == ["--statements"];
    let all = requested.is_empty() || requested == ["--all"] || statements_only;
    let selected = if all { Vec::new() } else { requested };
    // Shared fixtures belong to the original corpus directory; an independent
    // expression CSV is generated without them.
    let shared = if source.is_file() { SharedFixtures::default() } else { SharedFixtures::load(source)? };
    let mut generation = Generation::default();
    let source_ids = generation.run::<Expression>(&expression_source, &shared, |id| {
        !statements_only && (all || selected.iter().any(|s| s == id))
    })?;
    if let Some(statement_source) = &statement_source {
        generation.run::<Statement>(statement_source, &shared, |id| all || selected.iter().any(|s| s == id))?;
    }
    for id in &selected {
        if !generation.found.contains(id) {
            return Err(format!("case {id} was not found in either CSV").into());
        }
    }
    if let Some(id) = shared.test_case_ids().find(|id| !source_ids.contains(*id)) {
        return Err(format!("shared fixtures name {id}, which is not an expression case").into());
    }
    let Generation { mut cases, failures, .. } = generation;
    cases.sort_by(|a, b| a.path.cmp(&b.path));
    fs::create_dir_all(destination)?;
    fs::write(destination.join(".project"), project_file())?;
    let mut generation_report =
        csv::Writer::from_path(destination.join("generation-failures.csv"))?;
    generation_report.write_record(["kind", "feature", "id", "status", "category", "reason"])?;
    for failure in &failures {
        generation_report.write_record([
            failure.kind,
            &failure.test_case.feature,
            &failure.test_case.id,
            "failure",
            failure.rejection.category.as_str(),
            &failure.rejection.reason,
        ])?;
    }
    generation_report.flush()?;
    let mut index = String::from("kind,feature,id,model,diagram,expected_status,timed\n");
    let mut semantic_resources = String::new();
    for case in &cases {
        let target = destination.join(&case.path);
        fs::create_dir_all(target.parent().unwrap())?;
        fs::write(target, &case.model)?;
        semantic_resources.push_str(&format!(
            "    <semanticResources>{}</semanticResources>\n",
            case.path.display()
        ));
        let csp_dir = test_case_csp_dir(destination, case.timed);
        fs::create_dir_all(&csp_dir)?;
        let package = package_name(&case.id, case.kind)?;
        let arguments = if case.fixture_arguments.is_empty() {
            "0".to_owned()
        } else {
            format!("0,{}", case.fixture_arguments.join(","))
        };
        let event_prefix = case.expected_events.join(" -> ");
        let event_prefix = if event_prefix.is_empty() {
            String::new()
        } else {
            format!("{event_prefix} -> ")
        };
        let raw_model = format!("{package}::mod0::D__({arguments})");
        let model_definition = if let Some((driver, channels)) = &case.driver {
            let alphabet = format!("{{|{}|}}", channels.join(","));
            format!("driver = {driver}\nmodel = ({raw_model} [|{alphabet}|] driver) \\ {alphabet}")
        } else {
            format!("model = {raw_model}")
        };
        let check = case.custom_check.clone().unwrap_or_else(|| format!(
            "include \"{package}.csp\"\nexpected = {event_prefix}{package}::mod0::output.out!{} -> (SKIP |~| STOP)\n{model_definition}\nassert expected [FD= model\n",
            case.expected_csp
        ));
        fs::write(csp_dir.join(format!("check_{package}.csp")), check)?;
        let diagram = package_name(&case.id, case.kind)?;
        index.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            case.kind,
            case.feature,
            case.id,
            case.path.display(),
            diagram,
            case.expected_status,
            case.timed
        ));
    }
    fs::write(destination.join("case-index.csv"), index)?;
    let snapshots = destination.join("source-data");
    fs::create_dir_all(&snapshots)?;
    copy_selected_csv(
        &expression_source,
        &snapshots.join("cases.csv"),
        &selected,
        all && !statements_only,
    )?;
    let shared_snapshot = snapshots.join(shared_fixtures::FILE_NAME);
    if source.is_dir() && source.join(shared_fixtures::FILE_NAME).is_file() {
        fs::copy(source.join(shared_fixtures::FILE_NAME), &shared_snapshot)?;
    } else if shared_snapshot.exists() {
        fs::remove_file(&shared_snapshot)?;
    }
    if let Some(statement_source) = &statement_source {
        copy_selected_csv(statement_source, &snapshots.join("statement_cases.csv"), &selected, all)?;
    } else {
        fs::write(snapshots.join("statement_cases.csv"), "id,feature,robochart,inputs,python_output,types,observe,expected_status\n")?;
    }
    let aird = destination.join("representations.aird");
    if !aird.exists() {
        fs::write(
            aird,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xmi:XMI xmi:version=\"2.0\" xmlns:xmi=\"http://www.omg.org/XMI\" xmlns:description=\"http://www.eclipse.org/sirius/description/1.1.0\" xmlns:viewpoint=\"http://www.eclipse.org/sirius/1.1.0\">\n  <viewpoint:DAnalysis uid=\"_TestsuiteProjectAnalysis\" selectedViews=\"_TestsuiteProjectView\" version=\"14.5.1.202106111100\">\n{semantic_resources}    <ownedViews xmi:type=\"viewpoint:DView\" uid=\"_TestsuiteProjectView\">\n      <viewpoint xmi:type=\"description:Viewpoint\" href=\"platform:/plugin/circus.robocalc.robochart.graphical/description/robochart.odesign#//@ownedViewpoints[name='RoboChartID']\"/>\n    </ownedViews>\n  </viewpoint:DAnalysis>\n</xmi:XMI>\n"
            ),
        )?;
    }
    println!(
        "Generated {} models and recorded {} generation failures in {}",
        cases.len(),
        failures.len(),
        destination.display()
    );
    Ok(())
}

pub fn test_case_csp_dir(project: &Path, timed: bool) -> PathBuf {
    if timed { project.join("csp-gen/timed") } else { project.join("csp-gen") }
}

pub fn copy_selected_csv(
    source: &Path,
    destination: &Path,
    selected: &[String],
    all: bool,
) -> Result<()> {
    let mut reader = csv::Reader::from_path(source)?;
    let mut writer = csv::Writer::from_path(destination)?;
    writer.write_record(reader.headers()?)?;
    let id_index = reader
        .headers()?
        .iter()
        .position(|name| name == "id")
        .ok_or("CSV has no id column")?;
    for record in reader.records() {
        let record = record?;
        if all
            || selected
                .iter()
                .any(|id| record.get(id_index) == Some(id.as_str()))
        {
            writer.write_record(&record)?;
        }
    }
    writer.flush()?;
    Ok(())
}

pub fn project_file() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<projectDescription>\n  <name>testsuite-project</name>\n  <comment>Generated RoboChart expression and statement tests</comment>\n  <projects/>\n  <buildSpec><buildCommand><name>org.eclipse.xtext.ui.shared.xtextBuilder</name><arguments/></buildCommand></buildSpec>\n  <natures><nature>org.eclipse.sirius.nature.modelingproject</nature><nature>org.eclipse.xtext.ui.shared.xtextNature</nature></natures>\n</projectDescription>\n"
}
