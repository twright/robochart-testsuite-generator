//! Attribution of cases that did not pass: to the case itself, to a
//! reproduced RoboTool defect or limitation (`docs/robotool-bugs.md`), or to
//! neither.

/// RoboTool defects reproduced in `docs/robotool-bugs`, recognised by a log
/// message. Each was reproduced on a model that RoboTool itself validates. A
/// `whole` signature must be the entire message after FDR's location prefix,
/// so that `to is not in scope` does not match another name ending in `to`.
const ROBOTOOL_BUG_SIGNATURES: &[(&str, &str, bool)] = &[
    ("Case not treated by expression compiler: class circus.robocalc.robochart.impl.ForallImpl", "B6", false),
    ("Case not treated by expression compiler: class circus.robocalc.robochart.impl.ExistsImpl", "B6", false),
    ("Couldn't match expected type {(Int, Int)} with actual type Int", "B8", false),
    ("Case not treated by expression compiler: class circus.robocalc.robochart.impl.StringExpImpl", "B9", false),
    ("Case not treated by expression compiler: class circus.robocalc.robochart.impl.ResultExpImpl", "B10", false),
    ("Couldn't match expected type Int with actual type <Char>", "B12", false),
    ("from is not in scope", "B11", true),
    ("to is not in scope", "B11", true),
];

/// Attribute a case the generator could not model. Inputs outside RoboChart's
/// syntax are faults of the case; fractional reals are RoboTool's limitation.
pub(crate) fn attribute_generation_failure(category: &str) -> (&'static str, String) {
    match category {
        "unsupported_robochart_syntax" => {
            ("unsupported_case", " (the case expression is not valid RoboChart)".into())
        }
        "oracle_representation_limit" => {
            ("unsupported_robotool", " (RoboTool limitation R1: integer-valued reals)".into())
        }
        _ => ("failure", String::new()),
    }
}

/// Attribute a test case whose check stopped before an FDR verdict, given the
/// best context the generator can supply. A validation error counts against
/// the case only when RoboTool's first error in the model lies inside the
/// case's own expression; a CSP generation or loading error counts against
/// RoboTool only when it matches a reproduced defect, or when RoboTool's
/// expression compiler has no case for a construct it validated.
pub(crate) fn attribute_check_failure(
    category: &str,
    log: &str,
    model_path: &str,
    model: &str,
    expression: Option<&str>,
) -> Option<(&'static str, String)> {
    let signature = |line: &str| {
        let message = line.trim();
        let message = message.rsplit_once(": ").map_or(message, |(_, text)| text);
        ROBOTOOL_BUG_SIGNATURES
            .iter()
            .find(|(text, _, whole)| if *whole { message == *text } else { line.contains(text) })
            .map(|(_, bug, _)| *bug)
    };
    match category {
        "robochart_validation_error" => {
            let expression = expression?;
            let (line, column) = first_model_error(log, model_path)?;
            let text = model.lines().nth(line.checked_sub(1)?)?;
            let mut start = text.find(expression)?;
            let mut end = start + expression.len();
            // The parser can report an expression that fails to parse at
            // the parentheses the harness wraps around it.
            if text[..start].ends_with('(') && text[end..].starts_with(')') {
                start -= 1;
                end += 1;
            }
            (column > start && column <= end).then(|| {
                ("unsupported_case", " (RoboTool rejects the case expression itself: it is not valid RoboChart)".into())
            })
        }
        "csp_generator_exception" | "fdr_no_result" => {
            if let Some(bug) = log.lines().find_map(signature) {
                return Some(("unsupported_robotool", format!(" (RoboTool bug {bug})")));
            }
            let construct = log
                .lines()
                .find_map(|line| line.split("Case not treated by expression compiler: class ").nth(1))?;
            Some(("unsupported_robotool", format!(" (RoboTool cannot compile {})", construct.trim())))
        }
        _ => None,
    }
}

/// The earliest RoboTool validation error located in the case's model file.
fn first_model_error(log: &str, model_path: &str) -> Option<(usize, usize)> {
    let marker = format!("/{model_path} line : ");
    log.lines()
        .filter(|line| line.starts_with("ERROR:"))
        .filter_map(|line| {
            let rest = &line[line.find(&marker)? + marker.len()..];
            let (line_number, rest) = rest.split_once(" column : ")?;
            let column: String = rest.chars().take_while(char::is_ascii_digit).collect();
            Some((line_number.trim().parse().ok()?, column.parse().ok()?))
        })
        .min()
}

#[cfg(test)]
#[path = "tests/attribution.rs"]
mod tests;
