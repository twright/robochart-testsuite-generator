use super::*;

#[test]
fn failures_are_attributed_only_with_evidence() {
    let model = "package p\nmodule m {\n  const c : Foo = 1\n        action output ! (obj.value)\n}\n";
    let path = "cases/expressions/attribute/000.rct";
    let error = |line: usize, column: usize| format!("ERROR:Couldn't resolve. (file:<test-case-project>/{path} line : {line} column : {column})\n");
    let attribute = |category: &str, log: &str| attribute_check_failure(category, log, path, model, Some("obj.value")).map(|(status, _)| status);
    // The first error lies in the expression: the case is not valid RoboChart.
    assert_eq!(attribute("robochart_validation_error", &(error(4, 30) + &error(5, 1))), Some("unsupported_case"));
    // The first error lies in harness-generated context: not attributed.
    assert_eq!(attribute("robochart_validation_error", &(error(4, 26) + &error(3, 13))), None);
    assert_eq!(attribute("robochart_validation_error", &error(4, 25)), Some("unsupported_case"));
    assert_eq!(attribute("robochart_validation_error", &error(4, 24)), None);
    assert_eq!(attribute("robochart_validation_error", "ERROR:no location\n"), None);
    // Reproduced RoboTool defects and uncompiled constructs.
    assert_eq!(attribute("fdr_no_result", "defs/x.csp:214:89-91: to is not in scope\n"), Some("unsupported_robotool"));
    assert_eq!(attribute("fdr_no_result", "    from is not in scope\n"), Some("unsupported_robotool"));
    assert_eq!(attribute("fdr_no_result", "defs/x.csp:1:1-5: proto is not in scope\n"), None);
    assert_eq!(attribute("fdr_no_result", "Couldn't match expected type Bool with actual type Int\n"), None);
    let uncompiled = "LOG: Case not treated by expression compiler: class circus.robocalc.robochart.impl.NewImpl\n";
    assert_eq!(attribute_check_failure("csp_generator_exception", uncompiled, path, model, None).unwrap().1, " (RoboTool cannot compile circus.robocalc.robochart.impl.NewImpl)");
    assert_eq!(attribute("fdr_timeout", "from is not in scope"), None);
    assert_eq!(attribute_generation_failure("unsupported_robochart_syntax").0, "unsupported_case");
    assert_eq!(attribute_generation_failure("oracle_representation_limit").0, "unsupported_robotool");
    assert_eq!(attribute_generation_failure("missing_fixture_context").0, "failure");
}
