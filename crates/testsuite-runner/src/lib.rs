//! Runs the oracle checks of a generated testsuite project: compiles its
//! models to CSP with RoboTool, prepares the CSP for FDR, checks each test case
//! with FDR in a temporary project of its own (`verify`), attributes failures
//! to reproduced RoboTool defects, and writes the reports. It does not run the
//! RoboChart-to-Python translator under test.

mod attribution;
mod compile;
mod prepare;
#[cfg(test)]
#[path = "tests/preparation_properties.rs"]
mod preparation_properties;
mod report;
mod verify;

pub use compile::compile_csp;
pub use prepare::prepare_fdr;
pub use report::write_markdown_report;
pub use verify::verify;

pub(crate) fn safe_log_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
