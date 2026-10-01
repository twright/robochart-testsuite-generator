use super::Fixtures;
use serde_json::json;

#[test]
fn products_vectors_and_matrices_validate_shape_and_order() {
    let fixtures = Fixtures::default();
    assert_eq!(fixtures.csp_literal("int * boolean", &json!({"type":"tuple", "items":[1,true]})).unwrap(), "(1,true)");
    assert_eq!(fixtures.robo_literal("vector(int,2)", &json!([-1,2])).unwrap(), "[|-1,2|]");
    assert_eq!(fixtures.csp_literal("matrix(int,2,2)", &json!([[1,2],[3,4]])).unwrap(), "(1,2).(3,4)");
    for (ty, value) in [("int * boolean", json!([1])), ("vector(int,2)", json!([1])), ("matrix(int,2,2)", json!([[1,2],[3]])), ("matrix(int,0,2)", json!([]))] {
        assert!(fixtures.csp_literal(ty, &value).is_err(), "accepted invalid {ty}: {value}");
    }
    assert_eq!(fixtures.csp_literal("Seq(Set(boolean))", &json!([[true],[]])).unwrap(), "<{ true },{  }>");
}

#[test]
fn integral_real_observations_do_not_become_event_fields() {
    assert_eq!(super::exact_real_literal(&json!(1.0)).unwrap(), "1");
    assert_eq!(super::exact_real_literal(&json!(-3.0)).unwrap(), "-3");
    assert!(super::exact_real_literal(&json!(1.5)).unwrap_err().to_string().contains("cannot represent fractional"));
}

#[test]
fn nested_records_and_sequences_keep_declared_order() {
    let fixtures = Fixtures::new(
        r#"{"Point":[["x","int"],["y","nat"]],"Shape":[["points","Seq( Point )"],["enabled","boolean"]]}"#,
    )
    .unwrap();
    let value = json!({
        "enabled": true,
        "points": [{ "y": 2, "x": -1 }]
    });

    assert_eq!(
        fixtures.declarations(),
        "datatype Point { x : int y : nat }\ndatatype Shape { points : Seq(Point) enabled : boolean }\n"
    );
    assert_eq!(
        fixtures.robo_literal(" Shape ", &value).unwrap(),
        "Shape(| points = <Point(| x = -1, y = 2 |)>, enabled = true |)"
    );
    assert_eq!(
        fixtures.csp_literal("Shape", &value).unwrap(),
        "(<(-1,2)>,true)"
    );
}

#[test]
fn empty_boolean_sequence_uses_empty_sequence_literal() {
    let fixtures = Fixtures::new("").unwrap();
    assert_eq!(fixtures.declarations(), "");
    assert_eq!(
        fixtures
            .robo_literal("Seq ( boolean )", &json!([]))
            .unwrap(),
        "<>"
    );
    assert_eq!(
        fixtures.csp_literal("Seq(boolean)", &json!([])).unwrap(),
        "<>"
    );
}

#[test]
fn records_reject_missing_or_extra_fields_and_negative_naturals() {
    let fixtures = Fixtures::new(r#"{"R":[["count","nat"],["ok","boolean"]]}"#).unwrap();
    assert!(
        fixtures
            .robo_literal("R", &json!({ "count": 1 }))
            .unwrap_err()
            .to_string()
            .contains("missing field ok")
    );
    assert!(
        fixtures
            .csp_literal("R", &json!({ "count": 1, "ok": true, "other": 3 }))
            .unwrap_err()
            .to_string()
            .contains("extra field other")
    );
    assert!(
        fixtures
            .robo_literal("R", &json!({ "count": -1, "ok": true }))
            .unwrap_err()
            .to_string()
            .contains("non-negative")
    );
}

#[test]
fn enumerations_declare_literals_and_qualify_csp_names() {
    let fixtures = Fixtures::new(r#"{"Color":{"enumeration":["red","green"]},"R":[["c","Color"]]}"#)
        .unwrap()
        .in_package("p");
    assert_eq!(fixtures.robo_literal("Color", &json!("red")).unwrap(), "Color::red");
    assert_eq!(fixtures.csp_literal("Color", &json!("green")).unwrap(), "p_Color_green");
    assert_eq!(fixtures.robo_literal("R", &json!({"c": "red"})).unwrap(), "R(| c = Color::red |)");
    assert!(fixtures.declarations().contains("enumeration Color { red green }\n"));
    assert!(fixtures.csp_literal("Color", &json!("blue")).is_err());
    assert!(Fixtures::new(r#"{"E":{"enumeration":[]}}"#).is_err());
    assert!(Fixtures::new(r#"{"E":{"enumeration":["a","a"]}}"#).is_err());
    assert!(Fixtures::new(r#"{"Color":{"enumeration":["red"]}}"#).unwrap().csp_literal("Color", &json!("red")).is_err());
}

#[test]
fn direct_and_mutual_recursive_records_are_rejected() {
    assert!(Fixtures::new(r#"{"Node":[["next","Seq(Node)"]]}"#).is_err());
    assert!(Fixtures::new(r#"{"A":[["b","B"]],"B":[["a","A"]]}"#).is_err());
}

#[test]
fn strings_intervals_and_recorded_records_have_literals() {
    let fixtures = Fixtures::new(r#"{"R":[["x","int"],["y","int"]]}"#).unwrap();
    assert_eq!(fixtures.csp_literal("string", &json!("a b")).unwrap(), "\"a b\"");
    assert!(fixtures.csp_literal("string", &json!(1)).is_err());
    let interval = json!({"type": "interval", "start": 0, "end": 2, "left_closed": true, "right_closed": false});
    assert_eq!(
        fixtures.csp_literal("Set(int)", &interval).unwrap(),
        "{ interval_value | interval_value <- core_int, 0 <= interval_value, interval_value < 2 }"
    );
    assert_eq!(fixtures.csp_literal("Set(int)", &json!({"type": "set", "items": [-1, 2]})).unwrap(), "{ -1,2 }");
    let recorded = json!({"type": "record", "fields": {"y": 2, "x": 1}});
    assert_eq!(fixtures.csp_literal("R", &recorded).unwrap(), "(1,2)");
    assert_eq!(fixtures.robo_literal("R", &recorded).unwrap(), "R(| x = 1, y = 2 |)");
}

#[test]
fn types_parse_and_display_as_robochart() {
    use super::Type;
    for text in ["int * boolean", "(int * boolean) * nat", "Set(int * boolean)", "vector(real,2)", "string", "Seq(Seq(R))"] {
        assert_eq!(Type::parse(text).unwrap().to_string(), text);
    }
    assert_eq!(Type::parse(" Seq( int ) ").unwrap().to_string(), "Seq(int)");
    for malformed in ["int extra", "Seq(", "vector(int, 0)", "1x"] {
        assert_eq!(Type::parse(malformed).unwrap_err().category, crate::rejection::Category::UnsupportedFixtureType, "{malformed}");
    }
}
