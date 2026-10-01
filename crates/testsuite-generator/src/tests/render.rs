//! The one model renderer.

use super::*;

#[test]
fn a_machine_renders_every_part_in_a_fixed_layout() {
    let origin = |text: &str| Some(text.to_owned());
    let machine = Machine {
        comments: vec!["A model".into(), String::new()],
        package: "p".into(),
        import: Some("sequence_toolkit"),
        events: vec![
            Event { asynchronous: true, origin: origin("rule: out"), ..Event::output("output", Some("int")) },
            Event::input("sample", None),
        ],
        members: vec![
            Member::new("const a : int = 1", origin("rule: inputs")),
            Member::new("const b : int = 2", origin("rule: inputs")),
            Member::new("clock c", None),
        ],
        nodes: vec![
            Node::Initial("i0".into()),
            Node::State { name: "s".into(), entry: Some("ready".into()) },
            Node::State { name: "t".into(), entry: None },
            Node::Final("f0".into()),
        ],
        transitions: vec![Transition {
            name: "t0".into(),
            from: "i0".into(),
            to: "s".into(),
            trigger: Some("sample".into()),
            reset: Some("c".into()),
            condition: Some("a < b".into()),
            action: Some("output ! (a)".into()),
        }],
        declarations: vec![
            Member::new("datatype R { x : int }", origin("rule: record")),
            Member::new("function f() : int {\n  postcondition result == 1\n}", None),
        ],
    };
    assert_eq!(machine.render(), "\
// A model
// 
package p
import sequence_toolkit::*
module mod0 {
  connection ctrl0 on output to rp0 on output ( _async )
  connection rp0 on sample to ctrl0 on sample
  robotic platform rp0 {
    event output : int
    event sample
  }
  controller ctrl0 {
    event output : int
    event sample
    stm stm0 {
      // rule: inputs
      const a : int = 1
      const b : int = 2
      clock c
      // rule: out
      event output : int
      event sample
      initial i0
      state s { entry ready }
      state t {}
      final f0
      transition t0 {
        from i0
        to s
        trigger sample
        #c
        condition a < b
        action output ! (a)
      }
    }
    connection stm0 on output to ctrl0 on output
    connection ctrl0 on sample to stm0 on sample
  }
}
// rule: record
datatype R { x : int }
function f() : int {
  postcondition result == 1
}
");
}

#[test]
fn an_empty_machine_has_no_import_comments_or_declarations() {
    let text = Machine { package: "q".into(), ..Machine::default() }.render();
    assert_eq!(text, "package q\nmodule mod0 {\n  robotic platform rp0 {\n  }\n  controller ctrl0 {\n    stm stm0 {\n    }\n  }\n}\n");
}
