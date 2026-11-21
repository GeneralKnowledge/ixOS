//! The executor's tool calls: one JSON object per model turn, shaped by a
//! grammar the harness generates from the environment's tools, so the
//! grammar and this parser can't drift apart.

use std::collections::BTreeMap;

use serde_json::Value;

/// What an argument holds. Paths get a narrow character set in the grammar
/// and are checked again by the environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    Path,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolArg {
    pub name: &'static str,
    pub kind: ArgKind,
}

/// One tool an environment offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub args: Vec<ToolArg>,
    /// One line for the model's instructions.
    pub summary: &'static str,
}

impl ToolSpec {
    pub fn new(
        name: &'static str,
        args: &[(&'static str, ArgKind)],
        summary: &'static str,
    ) -> Self {
        Self {
            name,
            args: args
                .iter()
                .map(|&(name, kind)| ToolArg { name, kind })
                .collect(),
            summary,
        }
    }
}

/// The tool every executor has: it ends the task, and the harness then
/// checks the evidence.
pub const FINISH: &str = "finish";

fn finish_spec() -> ToolSpec {
    ToolSpec::new(
        FINISH,
        &[("summary", ArgKind::Text)],
        "say the task is done; the harness then checks the evidence",
    )
}

/// The environment's tools plus `finish`, in prompt and grammar order.
pub fn with_finish(tools: &[ToolSpec]) -> Vec<ToolSpec> {
    let mut all = tools.to_vec();
    all.push(finish_spec());
    all
}

/// The longest thought the grammar allows, in characters.
pub const THOUGHT_MAX: usize = 200;

/// The longest path the grammar allows.
pub const PATH_MAX: usize = 64;

/// A parsed call: the tool and its arguments by name, plus the model's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// The model's one line of reasoning before the action (may be empty
    /// on a backend without the grammar).
    pub thought: String,
    pub tool: String,
    pub args: BTreeMap<String, String>,
    pub raw: String,
}

impl ToolCall {
    pub fn arg(&self, name: &str) -> &str {
        self.args.get(name).map(String::as_str).unwrap_or("")
    }

    /// The call as the model should see it in its history: long arguments
    /// (a file's content) shortened when `compact`.
    pub fn display(&self, compact: bool) -> String {
        let mut object = serde_json::Map::new();
        object.insert("tool".into(), Value::String(self.tool.clone()));
        for (name, value) in &self.args {
            let shown = if compact && value.chars().count() > 120 {
                format!("…({} characters)", value.chars().count())
            } else {
                value.clone()
            };
            object.insert(name.clone(), Value::String(shown));
        }
        // In the grammar's order: thought, tool, then the arguments.
        let mut text = format!(
            "{{\"thought\": {}, \"tool\": {}",
            Value::String(self.thought.clone()),
            Value::String(self.tool.clone())
        );
        for (name, value) in object.iter().filter(|(k, _)| k.as_str() != "tool") {
            text += &format!(", {}: {}", Value::String(name.clone()), value);
        }
        text + "}"
    }
}

/// A GBNF grammar (llama.cpp) that only admits one call to one of `tools`,
/// with its arguments in order and exactly the spacing the instructions
/// show.
pub fn grammar(tools: &[ToolSpec]) -> String {
    let mut rules = Vec::new();
    let mut names = Vec::new();
    for (index, tool) in tools.iter().enumerate() {
        let rule = format!("call{index}");
        let mut body = format!("\"\\\"{}\\\"\"", tool.name);
        for arg in &tool.args {
            body += &format!(" \", \\\"{}\\\": \" ", arg.name);
            body += match arg.kind {
                ArgKind::Path => "path",
                ArgKind::Text => "string",
            };
        }
        rules.push(format!("{rule} ::= {body}"));
        names.push(rule);
    }
    let mut text = format!(
        "root ::= \"{{\\\"thought\\\": \" thought \", \\\"tool\\\": \" ( {} ) \"}}\"\n",
        names.join(" | ")
    );
    for rule in rules {
        text += &rule;
        text.push('\n');
    }
    text += &format!(
        "path ::= \"\\\"\" [A-Za-z0-9_] [A-Za-z0-9_./-]{{0,{}}} \"\\\"\"\n",
        PATH_MAX - 1
    );
    text += &format!("thought ::= \"\\\"\" char{{0,{THOUGHT_MAX}}} \"\\\"\"\n");
    text += "string ::= \"\\\"\" char* \"\\\"\"\n";
    text += "char ::= [^\"\\\\\\x00-\\x1F] | \"\\\\\" ([\"\\\\/bfnrt] | \"u\" [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F])\n";
    text
}

/// An example call for the instructions, in the grammar's exact shape.
pub fn example(tool: &ToolSpec) -> String {
    let mut text = format!(
        "{{\"thought\": \"why this step\", \"tool\": \"{}\"",
        tool.name
    );
    for arg in &tool.args {
        let value = match (tool.name, arg.name, arg.kind) {
            (_, "content", _) => "…the whole file…",
            (_, _, ArgKind::Path) => "name.py",
            (_, "summary", _) => "what you did",
            (_, "old", _) => "    return a - b\\n",
            (_, "new", _) => "    return a + b\\n",
            _ => "…",
        };
        text += &format!(", \"{}\": \"{}\"", arg.name, value);
    }
    text + "}"
}

/// Parses the model's answer as one call to one of `tools`. The grammar
/// makes a malformed answer impossible on llama.cpp; other backends (and a
/// stopped answer) can still produce one, so this checks everything.
pub fn parse(text: &str, tools: &[ToolSpec]) -> Result<ToolCall, String> {
    let trimmed = text.trim();
    let value: Value = serde_json::from_str(trimmed)
        .map_err(|err| format!("The answer was not one JSON object ({err})."))?;
    let Value::Object(object) = value else {
        return Err("The answer was not a JSON object.".into());
    };
    let Some(Value::String(name)) = object.get("tool") else {
        return Err("The object has no \"tool\".".into());
    };
    let Some(spec) = tools.iter().find(|t| t.name == name) else {
        return Err(format!("There is no tool called {name:?}."));
    };
    let thought = match object.get("thought") {
        Some(Value::String(thought)) => thought.trim().to_string(),
        None => String::new(),
        Some(_) => return Err("\"thought\" must be a string.".into()),
    };
    let mut args = BTreeMap::new();
    for arg in &spec.args {
        match object.get(arg.name) {
            Some(Value::String(value)) => {
                if arg.kind == ArgKind::Path {
                    check_path(value)?;
                }
                args.insert(arg.name.to_string(), value.clone());
            }
            _ => return Err(format!("{} needs \"{}\" as a string.", spec.name, arg.name)),
        }
    }
    if let Some(extra) = object.keys().find(|k| {
        k.as_str() != "tool"
            && k.as_str() != "thought"
            && !spec.args.iter().any(|a| a.name == k.as_str())
    }) {
        return Err(format!("{} has no argument {extra:?}.", spec.name));
    }
    Ok(ToolCall {
        thought,
        tool: name.clone(),
        args,
        raw: trimmed.to_string(),
    })
}

/// A project-relative path: no absolute paths, no `..`, no hidden files
/// (the harness keeps its records in `.harness/`), at most [`PATH_MAX`].
pub fn check_path(path: &str) -> Result<(), String> {
    let ok_chars = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c));
    if path.is_empty() || path.len() > PATH_MAX || !ok_chars {
        return Err(format!(
            "{path:?} is not a project path (letters, digits, _ . / - only, at most {PATH_MAX})."
        ));
    }
    if path.starts_with('/') {
        return Err(format!("{path:?} must be relative to the project."));
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.starts_with('.') {
            return Err(format!(
                "{path:?} must stay inside the project (no .., no hidden files)."
            ));
        }
    }
    Ok(())
}

/// A call the journal already accepted, read back without the tool specs
/// (for the screen and for shortening the history).
pub fn parse_loose(text: &str) -> Option<ToolCall> {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(text.trim()) else {
        return None;
    };
    let Some(Value::String(tool)) = object.get("tool") else {
        return None;
    };
    let mut args = BTreeMap::new();
    let mut thought = String::new();
    for (name, value) in &object {
        let Value::String(value) = value else {
            continue;
        };
        match name.as_str() {
            "tool" => {}
            "thought" => thought = value.clone(),
            _ => {
                args.insert(name.clone(), value.clone());
            }
        }
    }
    Some(ToolCall {
        thought,
        tool: tool.clone(),
        args,
        raw: text.trim().to_string(),
    })
}
