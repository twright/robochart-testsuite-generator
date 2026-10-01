//! `testsuite-generator SOURCE [PROJECT] [IDS]... [--statements | --all]`
//!
//! Generates a testsuite project; RoboTool and FDR are run by `testsuite-runner`.

use clap::Parser;
use std::path::PathBuf;
use testsuite_generator::{DEFAULT_PROJECT, Result, generate};

/// Generate a testsuite project from the test cases that you supply.
#[derive(Parser)]
#[command(name = "testsuite-generator", version)]
struct Cli {
    /// Directory holding cases.csv and statement_cases.csv (and, optionally,
    /// shared_fixtures.json), or a single expression CSV file
    source: PathBuf,
    /// Project directory to write, relative to the current directory unless
    /// absolute
    #[arg(default_value = DEFAULT_PROJECT)]
    project: PathBuf,
    /// Generate only these test case IDs [default: every test case]
    #[arg(conflicts_with_all = ["statements", "all"])]
    ids: Vec<String>,
    /// Generate only the statement test cases
    #[arg(long, conflicts_with = "all")]
    statements: bool,
    /// Generate every test case (the default)
    #[arg(long)]
    all: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let requested = if cli.statements {
        vec!["--statements".to_string()]
    } else if cli.all {
        vec!["--all".to_string()]
    } else {
        cli.ids
    };
    generate(&cli.source, &cli.project, requested)
}
