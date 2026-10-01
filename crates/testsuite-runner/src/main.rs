//! `testsuite-runner <COMMAND> [PROJECT]`, with commands `compile`,
//! `prepare-fdr`, `verify` and `report-markdown`.
//!
//! `compile` and `verify` need the RoboTool directory, `--robotool-home` or
//! `ROBOTOOL_HOME`. `prepare-fdr` and `verify` run FDR's `refines`, given by
//! `--fdr-refines` or `FDR_REFINES`, and otherwise found on `PATH`.

use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};
use testsuite_generator::{DEFAULT_PROJECT, Result};
use testsuite_runner::{compile_csp, prepare_fdr, verify, write_markdown_report};

/// Run the oracle checks of a testsuite project with RoboTool and FDR, and
/// report on them.
#[derive(Parser)]
#[command(name = "testsuite-runner", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Project {
    /// Project directory, relative to the current directory unless absolute
    #[arg(default_value = DEFAULT_PROJECT)]
    project: PathBuf,
}

impl Project {
    fn path(&self) -> &Path {
        &self.project
    }
}

#[derive(Args)]
struct RoboTool {
    /// Installed RoboTool directory
    #[arg(long, env = "ROBOTOOL_HOME", value_name = "DIR")]
    robotool_home: Option<PathBuf>,
}

impl RoboTool {
    fn required(&self) -> Result<PathBuf> {
        self.robotool_home
            .clone()
            .ok_or_else(|| "Set --robotool-home or ROBOTOOL_HOME to the installed RoboTool directory".into())
    }
}

#[derive(Args)]
struct Fdr {
    /// FDR's `refines` executable (a bare name is looked up on PATH)
    #[arg(long, env = "FDR_REFINES", value_name = "FILE", default_value = "refines")]
    fdr_refines: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    /// Compile each model to CSP with RoboTool
    Compile {
        #[command(flatten)]
        project: Project,
        #[command(flatten)]
        robotool: RoboTool,
    },
    /// Prepare the compiled CSP for FDR
    PrepareFdr {
        #[command(flatten)]
        project: Project,
        #[command(flatten)]
        fdr: Fdr,
    },
    /// Compile and check each test case in a temporary project of its own
    ///
    /// Writes verification-results.csv, verification-results.md and
    /// verification-logs/ into the project.
    Verify {
        #[command(flatten)]
        project: Project,
        #[command(flatten)]
        robotool: RoboTool,
        #[command(flatten)]
        fdr: Fdr,
        /// Check this many test cases at once; the results do not depend on it
        #[arg(long, short = 'j', value_name = "N", default_value_t = 1,
              value_parser = clap::value_parser!(u16).range(1..))]
        jobs: u16,
    },
    /// Write verification-results.md from verification-results.csv
    ReportMarkdown {
        #[command(flatten)]
        project: Project,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Compile { project, robotool } => compile_csp(project.path(), &robotool.required()?),
        Command::PrepareFdr { project, fdr } => prepare_fdr(project.path(), &fdr.fdr_refines),
        Command::Verify { project, robotool, fdr, jobs } => {
            verify(project.path(), &robotool.required()?, &fdr.fdr_refines, jobs.into())
        }
        Command::ReportMarkdown { project } => write_markdown_report(project.path()),
    }
}
