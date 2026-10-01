//! The one renderer of `.rct` models.
//!
//! Every model, of any shape, is a module `mod0` with a robotic platform
//! `rp0` and a controller `ctrl0` holding one state machine `stm0`. A
//! [`Machine`] describes such a model as data; [`Machine::render`] writes
//! it, taking no decisions. CSP checks are written per shape.

/// A model: one state machine, and the declarations it uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    /// Comment lines at the top of the model, without `// `.
    pub comments: Vec<String>,
    pub package: String,
    /// A RoboChart library imported with `import <name>::*`.
    pub import: Option<&'static str>,
    /// Events, declared on the platform, the controller and the state
    /// machine, and connected through all three.
    pub events: Vec<Event>,
    /// Constants, variables and clocks of the state machine.
    pub members: Vec<Member>,
    pub nodes: Vec<Node>,
    pub transitions: Vec<Transition>,
    /// Datatypes and functions, declared after the module.
    pub declarations: Vec<Member>,
}

/// An event, and whether it leaves or enters the state machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub ty: Option<String>,
    pub direction: Direction,
    /// The module connection is asynchronous (`( _async )`).
    pub asynchronous: bool,
    /// What decided the event's type, written as a comment in the state machine.
    pub origin: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// From the state machine to the platform.
    Output,
    /// From the platform to the state machine.
    Input,
}

/// A declaration, and what decided it. Consecutive members with the same
/// origin share one comment line.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub text: String,
    pub origin: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Initial(String),
    Final(String),
    State { name: String, entry: Option<String> },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transition {
    pub name: String,
    pub from: String,
    pub to: String,
    pub trigger: Option<String>,
    /// A clock reset, `#clock`.
    pub reset: Option<String>,
    pub condition: Option<String>,
    pub action: Option<String>,
}

impl Event {
    pub fn output(name: &str, ty: Option<&str>) -> Self {
        Event { name: name.into(), ty: ty.map(Into::into), direction: Direction::Output, asynchronous: false, origin: None }
    }

    pub fn input(name: &str, ty: Option<&str>) -> Self {
        Event { direction: Direction::Input, ..Event::output(name, ty) }
    }

    fn declaration(&self) -> String {
        match &self.ty {
            Some(ty) => format!("event {} : {ty}", self.name),
            None => format!("event {}", self.name),
        }
    }
}

impl Member {
    pub fn new(text: impl Into<String>, origin: Option<String>) -> Self {
        Member { text: text.into(), origin }
    }
}

impl Machine {
    /// The model's `.rct` text.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for comment in &self.comments {
            out.push_str(&format!("// {comment}\n"));
        }
        out.push_str(&format!("package {}\n", self.package));
        if let Some(import) = self.import {
            out.push_str(&format!("import {import}::*\n"));
        }
        out.push_str("module mod0 {\n");
        for event in &self.events {
            let name = &event.name;
            let asynchronous = if event.asynchronous { " ( _async )" } else { "" };
            out.push_str(&match event.direction {
                Direction::Output => format!("  connection ctrl0 on {name} to rp0 on {name}{asynchronous}\n"),
                Direction::Input => format!("  connection rp0 on {name} to ctrl0 on {name}{asynchronous}\n"),
            });
        }
        out.push_str("  robotic platform rp0 {\n");
        for event in &self.events {
            out.push_str(&format!("    {}\n", event.declaration()));
        }
        out.push_str("  }\n  controller ctrl0 {\n");
        for event in &self.events {
            out.push_str(&format!("    {}\n", event.declaration()));
        }
        out.push_str("    stm stm0 {\n");
        let events: Vec<Member> = self
            .events
            .iter()
            .map(|event| Member::new(event.declaration(), event.origin.clone()))
            .collect();
        write_members(&mut out, "      ", self.members.iter().chain(&events));
        for node in &self.nodes {
            out.push_str(&match node {
                Node::Initial(name) => format!("      initial {name}\n"),
                Node::Final(name) => format!("      final {name}\n"),
                Node::State { name, entry: None } => format!("      state {name} {{}}\n"),
                Node::State { name, entry: Some(entry) } => format!("      state {name} {{ entry {entry} }}\n"),
            });
        }
        for transition in &self.transitions {
            out.push_str(&format!(
                "      transition {} {{\n        from {}\n        to {}\n",
                transition.name, transition.from, transition.to
            ));
            for (keyword, part) in [
                ("trigger ", &transition.trigger),
                ("#", &transition.reset),
                ("condition ", &transition.condition),
                ("action ", &transition.action),
            ] {
                if let Some(part) = part {
                    out.push_str(&format!("        {keyword}{part}\n"));
                }
            }
            out.push_str("      }\n");
        }
        out.push_str("    }\n");
        for event in &self.events {
            let name = &event.name;
            out.push_str(&match event.direction {
                Direction::Output => format!("    connection stm0 on {name} to ctrl0 on {name}\n"),
                Direction::Input => format!("    connection ctrl0 on {name} to stm0 on {name}\n"),
            });
        }
        out.push_str("  }\n}\n");
        write_members(&mut out, "", self.declarations.iter());
        out.trim_end().to_owned() + "\n"
    }
}

/// Write members, each line indented, with a comment line wherever the
/// origin changes.
fn write_members<'a>(out: &mut String, indent: &str, members: impl Iterator<Item = &'a Member>) {
    let mut origin = None;
    for member in members {
        if member.origin.is_some() && member.origin != origin {
            out.push_str(&format!("{indent}// {}\n", member.origin.as_deref().unwrap_or_default()));
        }
        origin = member.origin.clone();
        for line in member.text.lines() {
            out.push_str(&format!("{indent}{line}\n"));
        }
    }
}

#[cfg(test)]
#[path = "tests/render.rs"]
mod tests;
