use super::SharedFixtures;

fn parse(text: &str) -> Result<SharedFixtures, String> {
    let fixtures: SharedFixtures = serde_json::from_str(text).map_err(|e| e.to_string())?;
    fixtures.validate().map_err(|e| e.to_string())?;
    Ok(fixtures)
}

#[test]
fn cases_merge_the_fixtures_they_name() {
    let fixtures = parse(
        r#"{"fixtures":{
            "r":{"note":"n","datatypes":{"R":[["v","int"]]},"constants":{"r":{"type":"R","value":{"v":1}}}},
            "f":{"note":"n","functions":["function f(): int { }"],"function_definitions":{"f":{"parameters":[],"body":"1"}}}},
           "cases":{"a/000":{"fixtures":["r","f"],"output_type":"int"}}}"#,
    )
    .unwrap();
    let context = fixtures.context("a/000").unwrap().unwrap();
    assert_eq!(context.datatypes, r#"{"R":[["v","int"]]}"#);
    assert_eq!(context.constants["r"].ty, "R");
    assert_eq!(context.functions, "function f(): int { }");
    assert!(context.function_definitions.contains("\"f\""));
    assert_eq!(context.output_type, "int");
    assert!(fixtures.context("b/000").unwrap().is_none());
}

#[test]
fn unknown_unused_duplicate_and_library_fixtures_are_rejected() {
    let unknown = r#"{"fixtures":{},"cases":{"a/000":{"fixtures":["x"],"output_type":"int"}}}"#;
    assert!(parse(unknown).unwrap_err().contains("unknown shared fixture"));
    let unused = r#"{"fixtures":{"x":{"note":"n"}},"cases":{}}"#;
    assert!(parse(unused).unwrap_err().contains("not used"));
    let duplicate = r#"{"fixtures":{
        "x":{"note":"n","constants":{"c":{"type":"int","value":1}}},
        "y":{"note":"n","constants":{"c":{"type":"int","value":2}}}},
        "cases":{"a/000":{"fixtures":["x","y"],"output_type":"int"}}}"#;
    assert!(parse(duplicate).unwrap_err().contains("twice"));
    let library = r#"{"fixtures":{"s":{"note":"n","function_definitions":{"size":{"parameters":["s"],"body":"0"}}}},
        "cases":{"a/000":{"fixtures":["s"],"output_type":"int"}}}"#;
    assert!(parse(library).unwrap_err().contains("library name size"));
}
