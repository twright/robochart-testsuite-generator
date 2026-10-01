//! The Markdown report of `verify` results.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use testsuite_generator::{IndexRow, Result};

#[derive(Deserialize)]
struct VerificationReportRow {
    id: String,
    status: String,
    category: String,
    #[serde(rename = "reason/log")]
    reason_log: String,
}

/// Statuses of cases that did not pass, in report order, with their meaning.
const STATUS_MEANINGS: &[(&str, &str)] = &[
    ("translator_defect", "FDR refuted the recorded Python observation of a supported case: evidence of a translator defect."),
    ("unsupported_case", "The case expression is not valid RoboChart, even with the best context: a likely error in the case specification."),
    ("unsupported_robotool", "The case is valid RoboChart, but a RoboTool defect or limitation prevents a verdict."),
    ("failure", "The harness could not reach a verdict; not attributed to the case, RoboTool or the translator."),
];

pub fn write_markdown_report(project: &Path) -> Result<()> {
    let rows: Vec<VerificationReportRow> =
        csv::Reader::from_path(project.join("verification-results.csv"))?
            .deserialize()
            .collect::<std::result::Result<_, _>>()?;
    let index: BTreeMap<String, (String, String)> =
        csv::Reader::from_path(project.join("case-index.csv"))?
            .deserialize::<IndexRow>()
            .map(|row| {
                let row = row?;
                Ok((row.id, (row.model, row.expected_status)))
            })
            .collect::<std::result::Result<_, csv::Error>>()?;
    let mut source_ids = BTreeSet::new();
    let mut source_counts = BTreeMap::new();
    for (kind, name) in [
        ("Expression", "cases.csv"),
        ("Statement", "statement_cases.csv"),
    ] {
        let mut reader = csv::Reader::from_path(project.join("source-data").join(name))?;
        let id_column = reader
            .headers()?
            .iter()
            .position(|header| header == "id")
            .ok_or("Source CSV has no id column")?;
        let mut count = 0usize;
        for row in reader.records() {
            source_ids.insert(
                row?.get(id_column)
                    .ok_or("Source row has no id")?
                    .to_owned(),
            );
            count += 1;
        }
        source_counts.insert(kind, count);
    }
    let passes = rows.iter().filter(|row| row.status == "pass").count();
    let reported_ids: BTreeSet<_> = rows.iter().map(|row| row.id.as_str()).collect();
    let complete = reported_ids.len() == source_ids.len()
        && source_ids
            .iter()
            .all(|id| reported_ids.contains(id.as_str()));
    let mut by_category: BTreeMap<(usize, &str), Vec<&VerificationReportRow>> = BTreeMap::new();
    for row in rows.iter().filter(|row| row.status != "pass") {
        let rank = STATUS_MEANINGS.iter().position(|(status, _)| *status == row.status).unwrap_or(STATUS_MEANINGS.len());
        by_category.entry((rank, &row.category)).or_default().push(row);
    }
    let status_count = |status: &str| rows.iter().filter(|row| row.status == status).count();
    let mut status_rows = format!("| `pass` | FDR's result matched the case's expected status, including deliberate mismatch tests. | {passes} |\n");
    for (status, meaning) in STATUS_MEANINGS {
        status_rows.push_str(&format!("| `{status}` | {meaning} | {} |\n", status_count(status)));
    }
    let other = rows.iter().filter(|row| row.status != "pass" && !STATUS_MEANINGS.iter().any(|(s, _)| *s == row.status)).count();
    if other > 0 {
        status_rows.push_str(&format!("| other | Unrecognised status. | {other} |\n"));
    }

    let mut markdown = format!(
        "# Testsuite verification\n\nThis report is generated from [verification-results.csv](verification-results.csv). The source cases are [expressions](source-data/cases.csv) and [statements](source-data/statement_cases.csv).\n\n## Summary\n\n| Measure | Count |\n| --- | ---: |\n| Expression source cases | {} |\n| Statement source cases | {} |\n| Report rows | {} |\n| Distinct reported IDs | {} |\n\nSource coverage: **{}**. Each case gets the best context the generator can supply, including the shared fixtures. An unsupported case still has an entry here, with the evidence for its attribution.\n\n## Outcomes\n\n| Status | Meaning | Cases |\n| --- | --- | ---: |\n{status_rows}\n## Categories of cases that did not pass\n\n| Status | Category | Cases |\n| --- | --- | ---: |\n",
        source_counts["Expression"],
        source_counts["Statement"],
        rows.len(),
        reported_ids.len(),
        if complete { "complete" } else { "incomplete" },
    );
    for ((_, category), cases) in &by_category {
        markdown.push_str(&format!("| `{}` | `{category}` | {} |\n", cases[0].status, cases.len()));
    }
    markdown.push_str("\n## Cases that did not pass\n\n");
    for ((_, category), cases) in &by_category {
        markdown.push_str(&format!(
            "### `{}`: `{category}` ({})\n\n| Case | Reason | Log |\n| --- | --- | --- |\n",
            cases[0].status,
            cases.len()
        ));
        for row in cases {
            let (reason, log) = split_reason_log(&row.reason_log);
            markdown.push_str(&format!(
                "| {} | {} | {} |\n",
                markdown_test_case_link(&row.id, &index),
                markdown_cell(reason),
                log.map(|path| format!("[log]({path})"))
                    .unwrap_or_else(|| "—".into()),
            ));
        }
        markdown.push('\n');
    }
    markdown.push_str(
        "## Passing cases\n\n| Case | Expected FDR result | Model |\n| --- | --- | --- |\n",
    );
    for row in rows.iter().filter(|row| row.status == "pass") {
        let model = index
            .get(&row.id)
            .map(|(path, _)| format!("[model]({path})"))
            .unwrap_or_else(|| "—".into());
        let expected = index
            .get(&row.id)
            .map(|(_, expected)| format!("`{expected}`"))
            .unwrap_or_else(|| "—".into());
        markdown.push_str(&format!(
            "| {} | {expected} | {model} |\n",
            markdown_cell(&row.id)
        ));
    }
    fs::write(project.join("verification-results.md"), markdown)?;
    Ok(())
}

fn split_reason_log(value: &str) -> (&str, Option<&str>) {
    value
        .rsplit_once("; log=")
        .map(|(reason, log)| (reason, Some(log)))
        .unwrap_or((value, None))
}

fn markdown_test_case_link(id: &str, index: &BTreeMap<String, (String, String)>) -> String {
    index
        .get(id)
        .map(|(path, _)| format!("[{}]({path})", markdown_cell(id)))
        .unwrap_or_else(|| format!("`{}`", markdown_cell(id)))
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}
