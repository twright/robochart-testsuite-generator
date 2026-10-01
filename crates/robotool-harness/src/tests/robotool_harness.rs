use super::*;
use proptest::prelude::*;

#[test]
fn fixture_domains_cover_both_equivalence_modules_and_leave_other_packages_alone() {
    let directory = std::env::temp_dir().join(format!("robochart-domain-rewrite-{}", std::process::id()));
    fs::create_dir_all(directory.join("timed/defs")).unwrap();
    for file in ["case.csp", "case_mod0.csp", "case_mod1.csp", "case_mod1_ctrl0_stm0.csp", "case_other.csp", "instantiations.csp", "timed/defs/case_mod1.csp"] {
        fs::write(directory.join(file), "LSeq(core_int,4)").unwrap();
    }
    replace_sequence_domains(&directory, &[("LSeq(core_int,4)".into(), "{<1,2,3>}".into())], "case").unwrap();
    for file in ["case.csp", "case_mod0.csp", "case_mod1.csp", "case_mod1_ctrl0_stm0.csp", "timed/defs/case_mod1.csp"] {
        assert_eq!(fs::read_to_string(directory.join(file)).unwrap(), "{<1,2,3>}", "{file}");
    }
    for file in ["case_other.csp", "instantiations.csp"] { assert_eq!(fs::read_to_string(directory.join(file)).unwrap(), "LSeq(core_int,4)", "{file}"); }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn bound_constant_domains_do_not_open_csp_comments() {
    let constants = BTreeMap::from([("a".to_owned(), -3)]);
    let sets = bound_value_sets("{{Neg(a, { -2..2}) | a <- { -2..2}}}", &constants);
    assert!(!sets.is_empty());
    for set in &sets {
        assert!(!set.contains("{-"), "negative domain opens a CSP comment: {set}");
        assert!(set.contains("a <- { -3}"), "lost the fixture constant: {set}");
    }
}

#[test]
fn sequence_bounds_preserve_library_equations_and_widen_nested_types() {
    let source = "LSeq(X,0) = {<>}\nLSeq(X,n) = LSeq(X,n-1)\nnametype A = LSeq(LSeq(core_boolean,2),2)\n";
    assert_eq!(
        widen_sequence_calls(source, 4),
        "LSeq(X,0) = {<>}\nLSeq(X,n) = LSeq(X,n-1)\nnametype A = LSeq(LSeq(core_boolean,4),4)\n"
    );
}

/// Moved from the generator's `finite_binder_domains_are_metadata_not_constants`.
#[test]
fn binder_domains_replace_only_the_named_binder() {
    assert_eq!(restrict_binder_domain("{x + 1 | x <- core_int, true} + {xx | xx <- core_int}", "x", "int", "{ 0 }"), "{x + 1 | x <- { 0 }, true} + {xx | xx <- core_int}");
}

const INSTANTIATIONS: &str = "-- generate int\nnametype core_int = { -2..2}\n\n-- generate nat\nnametype core_nat = {0..2}\n\n-- generate pkg_double\npkg_double(n) = 0\n\n-- generate boolean\nnametype core_boolean = Bool\n";

/// A stand-in for the CSP that RoboTool generates for package `pkg`, with a
/// timed copy, in a new project directory.
fn stand_in_project(label: &str) -> std::path::PathBuf {
    let project = std::env::temp_dir().join(format!("robotool-harness-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&project);
    let csp = project.join("csp-gen");
    fs::create_dir_all(csp.join("defs")).unwrap();
    fs::create_dir_all(csp.join("timed")).unwrap();
    fs::write(csp.join("instantiations.csp"), INSTANTIATIONS).unwrap();
    fs::write(
        csp.join("timed/instantiations.csp"),
        format!("{INSTANTIATIONS}\n-- generate core_clock_type\nnametype core_clock_type = {{0..2}}\n"),
    )
    .unwrap();
    fs::write(csp.join("defs/pkg_defs.csp"), "nametype pkg_R = { (a) | a <- core_int }\nnametype pkg_S = Bool\n").unwrap();
    fs::write(csp.join("pkg.csp"), "A = LSeq(LSeq(core_int,2),2)\nB = {x | x <- core_int}\nC = {y | y <- core_int}\n").unwrap();
    fs::write(csp.join("other.csp"), "A = LSeq(core_int,2)\nB = {x | x <- core_int}\n").unwrap();
    project
}

fn preparation() -> FdrPreparation {
    FdrPreparation {
        core_int: (-3, 7),
        core_nat_max: 7,
        sequence_bound: 3,
        sizing_constants: BTreeMap::new(),
        clock_max: 12,
        functions: vec![FunctionInstance {
            package: "pkg".into(),
            name: "double".into(),
            parameters: vec!["n".into()],
            body: "2 * n".into(),
        }],
        value_domains: vec![
            ValueDomain { package: "pkg".into(), ty: DomainType::Datatype("pkg_R".into()), values: vec!["(1)".into(), "(2)".into()] },
            ValueDomain { package: "pkg".into(), ty: DomainType::Sequence("LSeq(core_int,3)".into()), values: vec!["<1>".into()] },
            ValueDomain { package: "pkg".into(), ty: DomainType::Sequence("LSeq(LSeq(core_int,3),3)".into()), values: vec!["<<1>>".into()] },
        ],
        binder_domains: vec![BinderDomain { package: "pkg".into(), binder: "x".into(), ty: "int".into(), values: vec!["0".into(), "1".into()] }],
    }
}

/// The preparation needs no case data: everything it applies is in the
/// `FdrPreparation`. FDR is not run, because the stand-in has no bound
/// expressions to evaluate.
#[test]
fn prepare_applies_every_part_of_the_preparation_and_is_idempotent() {
    let project = stand_in_project("prepare");
    let missing_refines = project.join("no-refines");
    let read = |name: &str| fs::read_to_string(project.join("csp-gen").join(name)).unwrap();
    prepare(&project, &preparation(), &missing_refines).unwrap();
    let instantiations = read("instantiations.csp");
    assert!(instantiations.contains("-- generate int not\n-- RoboTool bound: nametype core_int = { -2..2}\nnametype core_int = { -3..7}\n"), "{instantiations}");
    assert!(instantiations.contains("-- generate nat not\n-- RoboTool bound: nametype core_nat = {0..2}\nnametype core_nat = { 0..7}\n"), "{instantiations}");
    assert!(instantiations.contains("-- generate pkg_double not\npkg_double(n) = 2 * n\n"), "{instantiations}");
    assert_eq!(read("defs/pkg_defs.csp"), "nametype pkg_R = {(1),(2)}\nnametype pkg_S = Bool\n");
    // The nested sequence type is replaced whole; only `x` is restricted.
    assert_eq!(read("pkg.csp"), "A = {<<1>>}\nB = {x | x <- { 0,1 }}\nC = {y | y <- core_int}\n");
    // Another package is widened but keeps its domains.
    assert_eq!(read("other.csp"), "A = LSeq(core_int,3)\nB = {x | x <- core_int}\n");
    let timed = read("timed/instantiations.csp");
    assert!(timed.contains("nametype core_int = { -3..7}\n"), "{timed}");
    assert!(timed.contains("-- generate core_clock_type not\nnametype core_clock_type = {0..12}\n"), "{timed}");

    let files = ["instantiations.csp", "timed/instantiations.csp", "defs/pkg_defs.csp", "pkg.csp", "other.csp"];
    let first: Vec<String> = files.iter().map(|name| read(name)).collect();
    prepare(&project, &preparation(), &missing_refines).unwrap();
    let second: Vec<String> = files.iter().map(|name| read(name)).collect();
    assert_eq!(first, second);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn prepare_rejects_a_function_that_robotool_did_not_generate() {
    let project = stand_in_project("missing-function");
    let mut preparation = preparation();
    preparation.functions[0].name = "triple".into();
    let error = prepare(&project, &preparation, &project.join("no-refines")).unwrap_err();
    assert_eq!(error.to_string(), "expected exactly one generated function fixture pkg_triple");
    preparation.functions[0].name = "double".into();
    preparation.functions[0].parameters = vec!["m".into()];
    let error = prepare(&project, &preparation, &project.join("no-refines")).unwrap_err();
    assert_eq!(error.to_string(), "generated function signature differs from fixture pkg_double(m)");
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn widened_bounds_extend_the_prepared_clock_bound() {
    let project = stand_in_project("widen");
    fs::write(project.join("model.rct"), "stm s {}\n").unwrap();
    prepare(&project, &preparation(), &project.join("no-refines")).unwrap();
    assert_eq!(widen_bounds(&project, "model.rct", 12).unwrap(), "core_int {-8..12}");
    let timed = fs::read_to_string(project.join("csp-gen/timed/instantiations.csp")).unwrap();
    assert!(timed.contains("nametype core_int = { -8..12}\n"), "{timed}");
    assert!(timed.contains("nametype core_clock_type = {0..14}\n"), "{timed}");
    fs::remove_dir_all(project).unwrap();
}

/// A CSP-like text of type expressions: `LSeq(T,n)`, plain names, and names
/// that merely end in `LSeq`.
#[derive(Clone, Debug)]
enum Term {
    Name(&'static str),
    LSeq(Box<Term>, usize),
    Suffixed(&'static str, Box<Term>, usize),
}

impl Term {
    fn render(&self) -> String {
        match self {
            Term::Name(n) => (*n).to_owned(),
            Term::LSeq(item, n) => format!("LSeq({},{n})", item.render()),
            Term::Suffixed(prefix, item, n) => format!("{prefix}LSeq({},{n})", item.render()),
        }
    }
}

fn arb_term() -> impl Strategy<Value = Term> {
    let leaf = prop::sample::select(vec!["core_int", "core_boolean", "X"]).prop_map(Term::Name);
    leaf.prop_recursive(3, 8, 1, |inner| {
        prop_oneof![
            3 => (inner.clone(), 0usize..4).prop_map(|(t, n)| Term::LSeq(Box::new(t), n)),
            1 => (prop::sample::select(vec!["my", "B2"]), inner, 0usize..4)
                .prop_map(|(p, t, n)| Term::Suffixed(p, Box::new(t), n)),
        ]
    })
}

proptest! {
    /// Widening only rewrites numerals, is idempotent, and leaves the text
    /// unchanged when the bound does not exceed the default of 2.
    #[test]
    fn sequence_widening_rewrites_only_bounds_and_is_idempotent(
        terms in prop::collection::vec(arb_term(), 1..4),
        max in 1usize..6,
    ) {
        let source: String = terms.iter().map(|t| format!("T = {}\n", t.render())).collect();
        let widened = widen_sequence_calls(&source, max);
        let letters = |s: &str| s.chars().filter(|c| !c.is_ascii_digit()).collect::<String>();
        prop_assert_eq!(letters(&widened), letters(&source));
        prop_assert_eq!(widen_sequence_calls(&widened, max), widened.clone());
        if max <= 2 {
            prop_assert_eq!(widened, source);
        }
    }
}
