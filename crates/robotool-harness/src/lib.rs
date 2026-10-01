//! Generic RoboTool and FDR primitives: running RoboTool's headless CSP
//! generator, running FDR with a time limit, and rewriting the CSP that
//! RoboTool generates (bound sections, sequence lengths, binder and value
//! domains, timed bounds). Nothing here reads case data: [`prepare`] applies
//! an [`FdrPreparation`] that its caller builds.

#[cfg(test)]
#[path = "tests/robotool_harness.rs"]
mod tests;

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Everything the FDR preparation of a project's CSP needs, as data. The
/// caller derives it from its cases; [`prepare`] applies it.
#[derive(Clone, Debug, PartialEq)]
pub struct FdrPreparation {
    /// The range of `core_int`.
    pub core_int: (i64, i64),
    /// The largest value of `core_nat`.
    pub core_nat_max: i64,
    /// The length bound of every `LSeq` type (RoboTool writes 2).
    pub sequence_bound: usize,
    /// Constants with one value, which the bound computation uses in place of
    /// ranging them over the whole type (see [`extend_bounds_from_expressions`]).
    pub sizing_constants: BTreeMap<String, i64>,
    /// The bound of the timed clock type, `core_clock_type`.
    pub clock_max: u32,
    /// Explicit bodies for RoboTool's placeholder function equations.
    pub functions: Vec<FunctionInstance>,
    /// Explicit finite domains of record and sequence types.
    pub value_domains: Vec<ValueDomain>,
    /// Finite domains of quantifier binders.
    pub binder_domains: Vec<BinderDomain>,
}

/// The CSP body of a function that RoboTool declares in `instantiations.csp`
/// as `-- generate {package}_{name}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInstance {
    pub package: String,
    pub name: String,
    pub parameters: Vec<String>,
    pub body: String,
}

/// A type of a package and the CSP values it ranges over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueDomain {
    pub package: String,
    pub ty: DomainType,
    /// The CSP literal of each value.
    pub values: Vec<String>,
}

/// Where a [`ValueDomain`]'s type is written in RoboTool's CSP.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainType {
    /// A `nametype` of the package's `defs/{package}_defs.csp`, such as
    /// `statement_case_R`.
    Datatype(String),
    /// A sequence type as written in the package's CSP files, such as
    /// `LSeq(core_int,3)`.
    Sequence(String),
}

/// The values that a quantifier binder `binder : ty` of a package ranges over,
/// in place of `core_{ty}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinderDomain {
    pub package: String,
    pub binder: String,
    pub ty: String,
    /// The CSP literal of each value.
    pub values: Vec<String>,
}

/// Make the CSP that RoboTool generated in `project/csp-gen` checkable by
/// FDR, as `preparation` describes: set the integer and sequence bounds,
/// instantiate the fixture functions, apply the value domains, extend the
/// bounds to the values the model's expressions reach, restrict the binder
/// domains and set the timed bounds. `refines` is FDR's `refines`
/// executable, which evaluates the model's bound expressions. Preparing a
/// project twice gives the same CSP as preparing it once.
pub fn prepare(project: &Path, preparation: &FdrPreparation, refines: &Path) -> Result<()> {
    let directory = project.join("csp-gen");
    let path = directory.join("instantiations.csp");
    let (minimum, maximum) = preparation.core_int;
    let nat_maximum = preparation.core_nat_max;
    let content = fs::read_to_string(&path)?;
    let content = set_bound_section(&content, "int", minimum, maximum)?;
    let content = set_bound_section(&content, "nat", 0, nat_maximum)?;
    fs::write(&path, content)?;
    println!(
        "Set FDR core_int to {{{minimum}..{maximum}}} and core_nat to at least {{0..{nat_maximum}}}"
    );
    if preparation.sequence_bound > 2 {
        widen_csp_sequences(&directory, preparation.sequence_bound)?;
        println!("Set FDR sequence bound to {}", preparation.sequence_bound);
    }
    instantiate_functions(&path, &preparation.functions)?;
    apply_value_domains(&directory, &preparation.value_domains)?;
    extend_bounds_from_expressions(
        refines,
        project,
        preparation.core_int,
        nat_maximum,
        &preparation.sizing_constants,
    )?;
    restrict_binder_domains(&directory, &preparation.binder_domains)?;
    prepare_timed_bounds(project, preparation.clock_max)?;
    Ok(())
}

/// Each package named by `items`, once, in order of first appearance.
fn packages<'a, T>(items: &'a [T], package: impl Fn(&'a T) -> &'a str) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for item in items {
        let name = package(item);
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

fn instantiate_functions(path: &Path, functions: &[FunctionInstance]) -> Result<()> {
    let mut content = fs::read_to_string(path)?;
    for function in functions {
        instantiate_function(&mut content, function)?;
    }
    fs::write(path, content)?;
    Ok(())
}

/// Replace RoboTool's placeholder equation for a fixture function with the
/// fixture's explicit CSP body.
fn instantiate_function(content: &mut String, function: &FunctionInstance) -> Result<()> {
    let qualified = format!("{}_{}", function.package, function.name);
    let kept = format!("-- generate {qualified} not\n");
    let marker = [format!("-- generate {qualified}\n"), kept.clone()]
        .into_iter()
        .find(|m| content.matches(m.as_str()).count() == 1)
        .ok_or_else(|| format!("expected exactly one generated function fixture {qualified}"))?;
    let header = content
        .find(&marker)
        .ok_or_else(|| format!("missing generated function fixture {qualified}"))?;
    // Annotate the section so that RoboTool keeps the fixture.
    content.replace_range(header..header + marker.len(), &kept);
    let start = header + kept.len();
    let end = content[start..].find('\n').map(|n| start + n).unwrap_or(content.len());
    let signature = format!("{qualified}({})", function.parameters.join(","));
    let existing = content[start..end]
        .split_once('=')
        .ok_or("generated function has no equation")?
        .0;
    if existing.chars().filter(|c| !c.is_whitespace()).collect::<String>() != signature {
        return Err(format!("generated function signature differs from fixture {signature}").into());
    }
    content.replace_range(start..end, &format!("{signature} = {}", function.body));
    Ok(())
}

/// Replace each datatype's `nametype` with its finite domain, and each
/// sequence type, in its package's CSP, with its domain. Longer sequence
/// types are replaced first, so that a nested type is replaced whole.
fn apply_value_domains(directory: &Path, domains: &[ValueDomain]) -> Result<()> {
    for package in packages(domains, |domain| domain.package.as_str()) {
        let domains: Vec<&ValueDomain> = domains.iter().filter(|domain| domain.package == package).collect();
        let set = |domain: &ValueDomain| format!("{{{}}}", domain.values.join(","));
        let datatypes: Vec<(&str, String)> = domains
            .iter()
            .filter_map(|domain| match &domain.ty {
                DomainType::Datatype(name) => Some((name.as_str(), set(domain))),
                DomainType::Sequence(_) => None,
            })
            .collect();
        if !datatypes.is_empty() {
            let definitions_path = directory.join("defs").join(format!("{package}_defs.csp"));
            let mut definitions = fs::read_to_string(&definitions_path)?;
            for (name, domain) in datatypes {
                let prefix = format!("nametype {name} = ");
                let start = definitions
                    .find(&prefix)
                    .ok_or_else(|| format!("missing generated datatype {name}"))?
                    + prefix.len();
                let end = definitions[start..].find('\n').map(|offset| start + offset).unwrap_or(definitions.len());
                definitions.replace_range(start..end, &domain);
            }
            fs::write(definitions_path, definitions)?;
        }
        let mut sequences: Vec<(String, String)> = domains
            .iter()
            .filter_map(|domain| match &domain.ty {
                DomainType::Sequence(ty) => Some((ty.clone(), set(domain))),
                DomainType::Datatype(_) => None,
            })
            .collect();
        sequences.sort_by_key(|(ty, _)| std::cmp::Reverse(ty.len()));
        replace_sequence_domains(directory, &sequences, package)?;
    }
    Ok(())
}

/// Restrict each binder, in its package's CSP, to its domain.
fn restrict_binder_domains(directory: &Path, binders: &[BinderDomain]) -> Result<()> {
    for package in packages(binders, |binder| binder.package.as_str()) {
        let replacements: Vec<(&str, &str, String)> = binders
            .iter()
            .filter(|binder| binder.package == package)
            .map(|binder| (binder.binder.as_str(), binder.ty.as_str(), format!("{{ {} }}", binder.values.join(","))))
            .collect();
        rewrite_package_csp(directory, package, &|source| {
            replacements
                .iter()
                .fold(source.to_owned(), |text, (name, ty, domain)| restrict_binder_domain(&text, name, ty, domain))
        })?;
    }
    Ok(())
}

/// Widen `core_int` and `core_nat` in a prepared project: by half their range
/// on each side, or by one value for models with set types, whose sizes grow
/// exponentially with the bounds. `clock_max` is the timed clock bound of the
/// preparation, as for [`prepare_timed_bounds`]; the widened clock bound is
/// two more.
pub fn widen_bounds(project: &Path, model: &str, clock_max: u32) -> Result<String> {
    let path = project.join("csp-gen/instantiations.csp");
    let content = fs::read_to_string(&path)?;
    let range = |name: &str| -> Result<(i64, i64)> {
        let prefix = format!("nametype core_{name} = {{ ");
        let line = content
            .lines()
            .find(|l| l.starts_with(&prefix))
            .ok_or_else(|| format!("no literal core_{name} range"))?;
        let (low, high) = line[prefix.len()..]
            .trim_end_matches('}')
            .split_once("..")
            .ok_or_else(|| format!("unexpected core_{name} range"))?;
        Ok((low.trim().parse()?, high.trim().parse()?))
    };
    let (low, high) = range("int")?;
    let (_, nat_high) = range("nat")?;
    let sets = fs::read_to_string(project.join(model))?.contains("Set(");
    let margin = if sets { 1 } else { ((high - low) / 2).max(2) };
    let (low, high, nat_high) = (low - margin, high + margin, nat_high + margin);
    let content = set_bound_section(&content, "int", low, high)?;
    let content = set_bound_section(&content, "nat", 0, nat_high)?;
    fs::write(&path, content)?;
    prepare_timed_bounds(project, clock_max)?;
    let timed = project.join("csp-gen/timed/instantiations.csp");
    if timed.is_file() {
        let mut text = fs::read_to_string(&timed)?;
        if let Some((start, end)) = generated_section(&text, "core_clock_type") {
            let section = &text[start..end];
            if let Some(high) = section.split("{0..").nth(1).and_then(|s| s.split('}').next()).and_then(|s| s.parse::<u32>().ok()) {
                text.replace_range(start..end, &format!("-- generate core_clock_type not\nnametype core_clock_type = {{0..{}}}\n\n", high + 2));
            }
        }
        fs::write(timed, text)?;
    }
    Ok(format!("core_int {{{low}..{high}}}"))
}

pub fn run_csp_generator(project: &Path, plugins: &Path) -> std::io::Result<Output> {
    let classpath = format!("{}/*", plugins.display());
    Command::new("java")
        .args([
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "-Djava.awt.headless=true",
            "-cp",
            &classpath,
            "circus.robocalc.robochart.generator.csp.Main",
            "true",
            "false",
        ])
        .arg(project.canonicalize()?)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
}

pub fn generator_output_log(output: &Output) -> Vec<u8> {
    let mut log = format!("process status: {}\n\n--- stdout ---\n", output.status).into_bytes();
    log.extend_from_slice(&output.stdout);
    if !output.stdout.ends_with(b"\n") {
        log.push(b'\n');
    }
    log.extend_from_slice(b"\n--- stderr ---\n");
    log.extend_from_slice(&output.stderr);
    log
}

/// Run FDR's `refines` executable, `refines`, on a check script in
/// `directory`, with a 90-second limit. On success, gives FDR's standard output
/// and the log: `generator_log` followed by FDR's standard output and error. A
/// failure is `(category, reason, log)`.
pub fn run_fdr_check(
    refines: &Path,
    directory: &Path,
    check_file: &str,
    generator_log: &[u8],
) -> std::result::Result<(String, Vec<u8>), (&'static str, String, Vec<u8>)> {
    let mut child = Command::new(refines)
        .args([
            "--quiet",
            "--brief",
            "--format",
            "plain",
            "--refinement-bfs-workers",
            "1",
            check_file,
        ])
        .current_dir(directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            let reason = format!("could not start FDR executable {}: {error}", refines.display());
            let mut log = generator_log.to_vec();
            log.extend_from_slice(format!("\n{reason}\n").as_bytes());
            ("fdr_error", reason, log)
        })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        let reason = "FDR stdout was not captured".to_owned();
        ("fdr_error", reason.clone(), generator_log.to_vec())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        let reason = "FDR stderr was not captured".to_owned();
        ("fdr_error", reason.clone(), generator_log.to_vec())
    })?;
    let stdout_thread = std::thread::spawn(move || {
        let mut text = String::new();
        let mut reader = BufReader::new(stdout);
        let result = std::io::Read::read_to_string(&mut reader, &mut text);
        (text, result)
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut text = String::new();
        let mut reader = BufReader::new(stderr);
        let result = std::io::Read::read_to_string(&mut reader, &mut text);
        (text, result)
    });

    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let reason = format!("could not wait for FDR: {error}");
                let mut log = generator_log.to_vec();
                log.extend_from_slice(format!("\n{reason}\n").as_bytes());
                return Err(("fdr_error", reason, log));
            }
        }
    };
    let (stdout, stdout_read) = stdout_thread.join().unwrap_or_else(|_| {
        (
            String::new(),
            Err(std::io::Error::other("FDR stdout reader panicked")),
        )
    });
    let (stderr, stderr_read) = stderr_thread.join().unwrap_or_else(|_| {
        (
            String::new(),
            Err(std::io::Error::other("FDR stderr reader panicked")),
        )
    });
    let mut log = generator_log.to_vec();
    log.extend_from_slice(b"\n--- FDR stdout ---\n");
    log.extend_from_slice(stdout.as_bytes());
    log.extend_from_slice(b"\n--- FDR stderr ---\n");
    log.extend_from_slice(stderr.as_bytes());
    if status.is_none() {
        return Err(("fdr_timeout", "FDR timed out after 90 seconds".into(), log));
    }
    if let Err(error) = stdout_read.and(stderr_read) {
        return Err(("fdr_read_error", error.to_string(), log));
    }
    Ok((stdout, log))
}

// A section of RoboTool's instantiations.csp starts with a `-- generate <name>`
// header. RoboTool regenerates such sections every time it runs, except those
// whose header is annotated `-- generate <name> not`, which it keeps. The FDR
// preparation annotates every section it rewrites, so that regenerating the
// CSP (for example from the RoboTool IDE) does not undo it.
//
// `clock_max` is the bound of the timed clock type, used only when the project
// has timed CSP.
pub fn prepare_timed_bounds(project: &Path, clock_max: u32) -> Result<()> {
    let timed = project.join("csp-gen/timed/instantiations.csp");
    if !timed.is_file() { return Ok(()); }
    let prepared = fs::read_to_string(project.join("csp-gen/instantiations.csp"))?;
    let mut content = fs::read_to_string(&timed)?;
    for name in ["int", "nat"] {
        let (start, end) = generated_section(&prepared, name).ok_or("missing prepared type section")?;
        let (old_start, old_end) = generated_section(&content, name).ok_or("missing timed type section")?;
        content.replace_range(old_start..old_end, &prepared[start..end]);
    }
    if let Some((start, end)) = generated_section(&content, "core_clock_type") {
        content.replace_range(start..end, &format!("-- generate core_clock_type not\nnametype core_clock_type = {{0..{clock_max}}}\n\n"));
    }
    fs::write(timed, content)?;
    Ok(())
}

fn generated_section(content: &str, name: &str) -> Option<(usize, usize)> {
    let start = [format!("-- generate {name}\n"), format!("-- generate {name} not\n")]
        .iter()
        .filter_map(|header| content.find(header.as_str()))
        .min()?;
    let end = content[start + 1..]
        .find("\n-- generate ")
        .map(|offset| start + 1 + offset)
        .unwrap_or(content.len());
    Some((start, end))
}

const BOUND_COMMENT: &str = "-- RoboTool bound: ";

/// Set a bound section (`int` or `nat`) of instantiations.csp to a literal
/// range. RoboTool's own bound expression for the section is kept, as comment
/// lines, so that the preparation can evaluate it (see
/// `extend_bounds_from_expressions`) now and on later runs.
pub fn set_bound_section(content: &str, name: &str, low: i64, high: i64) -> Result<String> {
    let (start, end) =
        generated_section(content, name).ok_or_else(|| format!("CSP {name} section not found"))?;
    let section = &content[start..end];
    let (header, body) = section.split_once('\n').unwrap_or((section, ""));
    let original: String = if header.ends_with(" not") {
        body.lines()
            .filter(|l| l.starts_with(BOUND_COMMENT))
            .map(|l| format!("{l}\n"))
            .collect()
    } else {
        body.trim_end()
            .lines()
            .map(|l| format!("{BOUND_COMMENT}{l}\n"))
            .collect()
    };
    Ok(format!(
        "{}-- generate {name} not\n{original}nametype core_{name} = {{ {low}..{high}}}\n{}",
        &content[..start],
        &content[end..]
    ))
}

/// RoboTool's bound expression for a section: the sets of values its
/// expressions can take, as the argument of `calc_type_min(Union(...))`.
fn bound_sets(content: &str, name: &str) -> Option<String> {
    let (start, end) = generated_section(content, name)?;
    let section = &content[start..end];
    let stored: Vec<&str> = section.lines().filter_map(|l| l.strip_prefix(BOUND_COMMENT)).collect();
    // A section the preparation has not rewritten (such as `real`) still holds
    // RoboTool's expression itself.
    let original = if stored.is_empty() {
        section.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        stored.join("\n")
    };
    let open = original.find("calc_type_min(Union(")? + "calc_type_min(Union(".len();
    let mut depth = 1;
    for (offset, ch) in original[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(original[open..open + offset].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

/// Rename RoboTool's saturating operators to plain-arithmetic ones, for use in
/// the bound computation only.
fn bound_operators(expression: &str) -> String {
    let mut out = expression.to_owned();
    for op in ["Plus", "Minus", "Mult", "Div", "Modulus", "Neg"] {
        let call = format!("{op}(");
        let mut result = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(i) = rest.find(&call) {
            let before = rest[..i].chars().last();
            result.push_str(&rest[..i]);
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                result.push_str(&call);
            } else {
                result.push_str(&format!("Bound{call}"));
            }
            rest = &rest[i + call.len()..];
        }
        result.push_str(rest);
        out = result;
    }
    out
}


/// Split `text` at `separator` where it is not nested in brackets.
fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut from) = (0i32, 0);
    for (i, ch) in text.char_indices() {
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            c if c == separator && depth == 0 => {
                parts.push(&text[from..i]);
                from = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[from..]);
    parts
}

/// The sets of RoboTool's bound expression, each `{E | generators}` or
/// `{elements}`, together with one set per operator application inside them:
/// every application of a saturating operator checks its own result against
/// the type, so the type must contain intermediate results as well as final
/// ones. RoboTool's own computation collects final values only.
fn bound_value_sets(sets: &str, constants: &BTreeMap<String, i64>) -> Vec<String> {
    let sets = sets.trim();
    let Some(inner) = sets.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for set in split_top_level(inner, ',') {
        let set = set.trim();
        let Some(body) = set.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
            continue;
        };
        let parts = split_top_level(body, '|');
        let (elements, generators) = match parts.as_slice() {
            [elements] => (elements.to_string(), None),
            [elements, generators] => {
                // A constant has one value; RoboTool nevertheless ranges it
                // over the whole type.
                let mut generators = generators.to_string();
                for (name, value) in constants {
                    for prefix in ["", " "] {
                        generators = generators
                            .replace(&format!("{prefix}{name} <- {{ -2..2}}"), &format!("{prefix}{name} <- {{ {value}}}"));
                    }
                }
                (elements.to_string(), Some(generators))
            }
            _ => continue,
        };
        let elements = bound_operators(&elements.replace("{ -2..2}", "core_int").replace("{0..2}", "core_nat"));
        let generators = generators.map(|g| g.replace("{ -2..2}", "core_int").replace("{0..2}", "core_nat"));
        // Enumerate only the sequences long enough for the indices the
        // expression reads, as the model itself requires.
        let guards = sequence_guards(&elements);
        let generators = match (generators, guards.is_empty()) {
            (g, true) => g,
            (Some(g), false) => Some(format!("{g}, {}", guards.join(", "))),
            (None, false) => Some(format!("bound_unused <- {{0}}, {}", guards.join(", "))),
        };
        let wrap = |element: &str| match &generators {
            Some(g) => format!("{{{element} | {g}}}"),
            None => format!("{{{element}}}"),
        };
        out.push(wrap(&elements));
        let mut rest = 0;
        while let Some(i) = elements[rest..].find("Bound") {
            let start = rest + i;
            rest = start + 1;
            let Some(open) = elements[start..].find('(').map(|o| start + o) else { break };
            if !elements[start + 5..open].chars().all(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            let mut depth = 0;
            let mut close = None;
            for (offset, ch) in elements[open..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(open + offset);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(close) = close {
                out.push(wrap(&elements[start..=close]));
            }
        }
    }
    out
}

/// For each `access_(s, i)` in an expression, the guard that `i` indexes `s`.
fn sequence_guards(expression: &str) -> Vec<String> {
    let mut guards = Vec::new();
    let mut rest = 0;
    while let Some(i) = expression[rest..].find("access_(") {
        let open = rest + i + "access_".len();
        rest = open;
        if expression[..open - "access_".len()]
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        let mut depth = 0;
        let mut close = None;
        for (offset, ch) in expression[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { break };
        if let [sequence, index] = split_top_level(&expression[open + 1..close], ',').as_slice() {
            guards.push(format!("{index} >= 1 and {index} <= #({sequence})", index = index.trim(), sequence = sequence.trim()));
        }
    }
    guards
}

/// Extend `core_int` and `core_nat` to contain every value the model's
/// expressions reach, including intermediate results. RoboTool computes such a
/// bound itself, but with its saturating operators over its default range
/// { -2..2}, and from final values only, so the result misses values the model
/// computes (RoboTool bug B4). Here its bound expressions are evaluated by FDR
/// with plain arithmetic, one set per operator application, with constants at
/// their fixture values and variables over the case's range. Only the size of
/// the types changes: RoboTool's operators are left as generated. If the
/// evaluation is not possible, the case range is kept; the wider-bounds
/// recheck of `testsuite-runner verify` then reports any case whose verdict
/// depends on the bounds. `refines` is FDR's `refines` executable.
pub fn extend_bounds_from_expressions(
    refines: &Path,
    project: &Path,
    test_case_ints: (i64, i64),
    test_case_nat_high: i64,
    constants: &BTreeMap<String, i64>,
) -> Result<()> {
    let directory = project.join("csp-gen");
    let path = directory.join("instantiations.csp");
    let content = fs::read_to_string(&path)?;
    let mut sets = Vec::new();
    for name in ["int", "nat", "real"] {
        if let Some(found) = bound_sets(&content, name) {
            sets.extend(bound_value_sets(&found, constants));
        }
    }
    if sets.is_empty() {
        return Ok(());
    }
    let packages: Vec<String> = fs::read_dir(&directory)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.ends_with(".csp")
                && !n.starts_with("check_")
                && !n.ends_with("_coreassertions.csp")
                && n != "instantiations.csp"
                && n != "bound_evaluation.csp"
        })
        .collect();
    let [package] = packages.as_slice() else {
        println!("Kept the case bounds: the bound computation handles one package at a time");
        return Ok(());
    };
    let script = format!(
        "include \"{package}\"\n\
         BoundPlus(e1,e2,T) = e1+e2\n\
         BoundMinus(e1,e2,T) = e1-e2\n\
         BoundMult(e1,e2,T) = e1*e2\n\
         BoundDiv(e1,e2,T) = if e2 == 0 then e1 else e1/e2\n\
         BoundModulus(e1,e2,T) = if e2 == 0 then e1 else e1%e2\n\
         BoundNeg(e1,T) = -e1\n\
         bound_all = union(core_int, Union({{{}}}))\n\
         bound_values = (calc_type_min(bound_all), calc_type_max(bound_all))\n\
         channel bound : {{bound_values}}\n\
         assert STOP [T= bound!bound_values -> STOP\n",
        sets.join(",\n  ")
    );
    let script_path = directory.join("bound_evaluation.csp");
    fs::write(&script_path, script)?;
    let child = Command::new(refines)
        .args(["--quiet", "--format", "plain", "bound_evaluation.csp"])
        .current_dir(&directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&script_path);
            println!("Kept the case bounds: could not run {}: {error}", refines.display());
            return Ok(());
        }
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            println!("Kept the case bounds: evaluating the bound expression took over 60 seconds");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut text = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        std::io::Read::read_to_string(&mut stdout, &mut text)?;
    }
    if let Some(mut stderr) = child.stderr.take() {
        std::io::Read::read_to_string(&mut stderr, &mut text)?;
    }
    let Some(event) = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Error Event: bound."))
    else {
        let reason = text
            .lines()
            .map(str::trim)
            .skip_while(|l| !l.contains(".csp:"))
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let reason = if reason.is_empty() { "no result".to_owned() } else { reason };
        println!("Kept the case bounds: the bound expression could not be evaluated ({reason})");
        return Ok(());
    };
    let _ = fs::remove_file(&script_path);
    let numbers: Vec<i64> = event
        .split(|c: char| !(c.is_ascii_digit() || c == '-'))
        .filter_map(|n| n.parse().ok())
        .collect();
    let [bound_low, bound_high] = numbers.as_slice() else {
        println!("Kept the case bounds: unexpected bound result {event}");
        return Ok(());
    };
    let (low, high) = (test_case_ints.0.min(*bound_low), test_case_ints.1.max(*bound_high));
    let nat = test_case_nat_high.max(high);
    let content = set_bound_section(&content, "int", low, high)?;
    let content = set_bound_section(&content, "nat", 0, nat)?;
    fs::write(&path, content)?;
    println!("Extended core_int to {{{low}..{high}}} and core_nat to {{0..{nat}}} from the model's expressions");
    Ok(())
}

pub fn restrict_binder_domain(source: &str, binder: &str, ty: &str, domain: &str) -> String {
    let pattern = format!("{binder} <- core_{ty}");
    let mut text = String::new();
    let mut offset = 0;
    for (start, _) in source.match_indices(&pattern) {
        let end = start + pattern.len();
        let identifier = |c: char| c.is_ascii_alphanumeric() || c == '_';
        if source[..start].chars().next_back().is_some_and(identifier) || source[end..].chars().next().is_some_and(identifier) { continue; }
        text.push_str(&source[offset..start]);
        text.push_str(&format!("{binder} <- {domain}"));
        offset = end;
    }
    text.push_str(&source[offset..]);
    text
}

pub fn replace_sequence_domains(directory: &Path, domains: &[(String, String)], package: &str) -> Result<()> {
    rewrite_package_csp(directory, package, &|source| {
        domains.iter().fold(source.to_owned(), |text, (ty, values)| text.replace(ty, values))
    })
}

pub fn rewrite_package_csp(directory: &Path, package: &str, rewrite: &impl Fn(&str) -> String) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            rewrite_package_csp(&path, package, rewrite)?;
        } else if path.extension().is_some_and(|extension| extension == "csp")
            && path.file_stem().is_some_and(|name| {
                name.to_string_lossy().strip_prefix(package).is_some_and(|suffix| {
                    let suffix = suffix.strip_suffix("_coreassertions").unwrap_or(suffix);
                    matches!(suffix, "" | "_defs" | "_mod0" | "_mod0_ctrl0" | "_mod0_ctrl0_stm0" | "_mod1" | "_mod1_ctrl0" | "_mod1_ctrl0_stm0")
                })
            }) {
            let original = fs::read_to_string(&path)?;
            let fixed = rewrite(&original);
            if fixed != original { fs::write(path, fixed)?; }
        }
    }
    Ok(())
}

pub fn widen_csp_sequences(directory: &Path, maximum: usize) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            widen_csp_sequences(&path, maximum)?;
        } else if path.extension().is_some_and(|extension| extension == "csp") {
            let original = fs::read_to_string(&path)?;
            let widened = widen_sequence_calls(&original, maximum);
            if widened != original {
                fs::write(path, widened)?;
            }
        }
    }
    Ok(())
}

// Match the length argument of every LSeq call, including nested element types.
fn widen_sequence_calls(source: &str, maximum: usize) -> String {
    let mut replacements = Vec::new();
    for (start, _) in source.match_indices("LSeq(") {
        if start > 0 && source.as_bytes()[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let mut depth = 1;
        let mut comma = None;
        for (offset, ch) in source[start + 5..].char_indices() {
            let position = start + 5 + offset;
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(comma) = comma
                            && source[comma + 1..position]
                                .trim()
                                .parse::<usize>()
                                .is_ok_and(|n| n == 2 && n < maximum)
                        {
                            replacements.push((comma + 1, position));
                        }
                        break;
                    }
                }
                ',' if depth == 1 => comma = Some(position),
                _ => {}
            }
        }
    }
    let mut widened = source.to_owned();
    replacements.sort_unstable();
    for (start, end) in replacements.into_iter().rev() {
        widened.replace_range(start..end, &maximum.to_string());
    }
    widened
}
