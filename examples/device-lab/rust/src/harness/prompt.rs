//! What the model sees (instructions, the task, its linear history within
//! a budget) and what the screen and the transcript show, all from the
//! journal's entries.

use forge_services::ai::ChatMessage;

use super::calls::{example, parse_loose, ToolSpec, FINISH};
use super::evidence::Criterion;
use super::journal::{self, Entry};
use super::{HarnessStep, HarnessTaskRecord};

/// The executor's instructions: the tools with an example call each, and
/// the few rules that matter.
pub fn instructions(tools: &[ToolSpec]) -> String {
    let mut text = String::from(
        "You are Bonsai, a careful Python programmer. You work on a small project on an iPhone, \
         with Python 3 and its standard library only: no network, no pip, no input().\n\
         You act by calling tools, one call per answer. Each answer is one JSON object: a short \
         thought, then the tool and its arguments, exactly like the examples.\n\nTools:\n",
    );
    for tool in tools {
        text += &format!("- {}: {}\n  {}\n", tool.name, tool.summary, example(tool));
    }
    text += "\nRules:\n\
             - Paths are relative to the project, like \"app.py\".\n\
             - write_file replaces the whole file, so always send the complete file. To change a few \
             lines of a file that exists, use edit_file: old is the exact current lines (copied from \
             read_file, without the line numbers), new is what replaces them.\n\
             - Tests use unittest, in files named test_*.py. run_tests runs them all.\n\
             - Read every result. When something fails, fix the cause, then run it again.\n\
             - Call finish only when a result shows the task is done. The harness checks the \
             evidence and tells you if anything is missing.\n\
             - If the task can't be done as written (a file it needs is missing, or it needs \
             something not allowed here, like the network), call give_up with the reason instead \
             of trying again and again.";
    text
}

/// Tokens a text costs, roughly (code tokenizes densely, so 3 characters).
fn tokens(text: &str) -> usize {
    text.chars().count() / 3 + 4
}

/// Tokens the prompt may use: the window, less the answer, less a margin;
/// smaller each time the model said it didn't fit.
pub fn budget(context_tokens: u32, turn_tokens: u32, shrink: u32) -> usize {
    let room = context_tokens
        .saturating_sub(turn_tokens)
        .saturating_sub(96) as usize;
    room * 4usize.saturating_sub(shrink as usize).max(1) / 4
}

/// `text` cut to `limit` characters: the start and the end, with a note on
/// what was left out (mini-swe-agent's head and tail).
pub fn cut(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let head = limit / 3;
    let tail = limit - head;
    // cut: the model's own text, shortened for its prompt with "…" in the middle
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!(
        "{start}\n… {} characters left out. To see less at once, print less or run one thing at a time. …\n{end}",
        count - limit
    )
}

/// One step as the model sees it, in full and shortened.
struct Turn {
    answer: String,
    answer_short: String,
    result: String,
    result_short: String,
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    let more = text.lines().count() > 1;
    if more {
        format!("{line} (rest left out)")
    } else {
        line.to_string()
    }
}

fn turns(history: &[Entry]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for entry in history {
        match entry {
            Entry::Answer { text, .. } => {
                let short = parse_loose(text)
                    .map(|c| c.display(true))
                    .unwrap_or_else(|| text.clone());
                turns.push(Turn {
                    answer: text.clone(),
                    answer_short: short,
                    result: String::new(),
                    result_short: String::new(),
                });
            }
            Entry::Invalid { text, error, .. } => {
                // cut: the model's invalid answer, quoted back to it
                let shown: String = text.chars().take(300).collect();
                let result = format!(
                    "That answer was not a valid call: {error} Answer with one JSON object, exactly like the examples."
                );
                turns.push(Turn {
                    answer: shown.clone(),
                    answer_short: shown,
                    result_short: result.clone(),
                    result,
                });
            }
            Entry::Result { text, .. } => {
                if let Some(turn) = turns.last_mut() {
                    turn.result = format!("Result:\n{text}");
                    turn.result_short = format!("Result: {}", first_line(text));
                }
            }
            Entry::Nudge { text, .. } => {
                if let Some(turn) = turns.last_mut() {
                    turn.result += &format!("\n\n{text}");
                    turn.result_short += &format!(" {text}");
                }
            }
            _ => {}
        }
    }
    turns
}

/// The request for the next step: instructions, the task, and as much of
/// the task's history as fits in `budget` tokens. Older results shrink to
/// one line first, then older file contents, then the oldest steps go.
pub fn messages(
    system: &str,
    request: &str,
    criteria: &[Criterion],
    overview: &str,
    history: &[Entry],
    budget: usize,
) -> Vec<ChatMessage> {
    let done_when = criteria
        .iter()
        .map(Criterion::describe)
        .collect::<Vec<_>>()
        .join("; ");
    let task =
        format!("Task: {request}\nDone when: {done_when}.\n\nProject files now:\n{overview}");
    let turns = turns(history);
    let fixed = tokens(system) + tokens(&task);
    let keep_full = 2;
    // Level 0: everything in full; 1: old results shortened; 2: old calls
    // shortened too; then drop the oldest steps.
    for level in 0..3 {
        let built = build(system, &task, &turns, level, keep_full, 0);
        if fixed + cost(&built) <= budget {
            return built;
        }
    }
    for dropped in 1..turns.len() {
        let built = build(system, &task, &turns, 2, keep_full, dropped);
        if fixed + cost(&built) <= budget {
            return built;
        }
    }
    build(
        system,
        &task,
        &turns,
        2,
        keep_full,
        turns.len().saturating_sub(1),
    )
}

fn cost(messages: &[ChatMessage]) -> usize {
    // The system message and the task are counted as `fixed`.
    messages.iter().skip(2).map(|m| tokens(&m.content)).sum()
}

fn build(
    system: &str,
    task: &str,
    turns: &[Turn],
    level: u8,
    keep_full: usize,
    dropped: usize,
) -> Vec<ChatMessage> {
    let mut task = task.to_string();
    if dropped > 0 {
        task += &format!("\n\n({dropped} earlier steps are left out to save space.)");
    }
    let mut messages = vec![ChatMessage::system(system), ChatMessage::user(task)];
    let recent_from = turns.len().saturating_sub(keep_full);
    for (index, turn) in turns.iter().enumerate().skip(dropped) {
        let old = index < recent_from;
        let answer = if old && level >= 2 {
            &turn.answer_short
        } else {
            &turn.answer
        };
        let result = if old && level >= 1 {
            &turn.result_short
        } else {
            &turn.result
        };
        messages.push(ChatMessage::assistant(answer.clone()));
        if !result.is_empty() {
            messages.push(ChatMessage::user(result.clone()));
        }
    }
    messages
}

/// The task record of a task's entries (from its `Task` line).
pub fn record(task: &[Entry]) -> Option<HarnessTaskRecord> {
    let Some(Entry::Task {
        task: id,
        description,
        criteria,
        model,
        ..
    }) = task.first()
    else {
        return None;
    };
    let (status, reason) = journal::status_of(task)?;
    let attempts = task
        .iter()
        .filter(|e| matches!(e, Entry::Result { tool, .. } if tool == FINISH))
        .count() as u32;
    Some(HarnessTaskRecord {
        id: id.clone(),
        description: description.clone(),
        status,
        dependencies: Vec::new(),
        acceptance: criteria.iter().map(Criterion::describe).collect(),
        attempts,
        result: reason,
        model_id: model.clone(),
    })
}

/// The steps of a task's entries, for the screen.
pub fn steps(task: &[Entry]) -> Vec<HarnessStep> {
    let mut steps: Vec<HarnessStep> = Vec::new();
    for entry in task {
        match entry {
            Entry::Answer {
                step,
                text,
                model_ms,
                ..
            } => {
                let call = parse_loose(text);
                steps.push(HarnessStep {
                    index: *step,
                    thought: call.as_ref().map(|c| c.thought.clone()).unwrap_or_default(),
                    tool: call.as_ref().map(|c| c.tool.clone()).unwrap_or_default(),
                    call: call
                        .as_ref()
                        .map(|c| c.display(true))
                        .unwrap_or_else(|| text.clone()),
                    ok: None,
                    result: String::new(),
                    model_ms: *model_ms,
                    tool_ms: 0,
                });
            }
            Entry::Invalid {
                step,
                text,
                error,
                model_ms,
                ..
            } => steps.push(HarnessStep {
                index: *step,
                thought: String::new(),
                tool: "invalid".into(),
                // cut: the model's invalid answer, shown as a step
                call: text.chars().take(300).collect(),
                ok: Some(false),
                result: error.clone(),
                model_ms: *model_ms,
                tool_ms: 0,
            }),
            Entry::Result {
                step,
                ok,
                text,
                tool_ms,
                ..
            } => {
                if let Some(last) = steps.last_mut().filter(|s| s.index == *step) {
                    last.ok = Some(*ok);
                    last.result = text.clone();
                    last.tool_ms = *tool_ms;
                }
            }
            Entry::Nudge { step, text, .. } => {
                if let Some(last) = steps.last_mut().filter(|s| s.index == *step) {
                    last.result += &format!("\n\n{text}");
                }
            }
            _ => {}
        }
    }
    steps
}

/// The task as Markdown, to copy as evidence.
pub fn transcript(project: &str, task: &[Entry]) -> String {
    let Some(record) = record(task) else {
        return "No task yet.".into();
    };
    let mut text = format!(
        "# Bonsai Harness: {}\n\nProject: {project}\nModel: {}\nTask: {}\nDone when: {}\nStatus: {:?}{}\n",
        record.id,
        record.model_id,
        record.description,
        record.acceptance.join("; "),
        record.status,
        record
            .result
            .as_ref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default(),
    );
    // The model's whole answers, file contents included: the screen
    // shortens them, the transcript is the evidence.
    let answers: std::collections::HashMap<u32, &str> = task
        .iter()
        .filter_map(|entry| match entry {
            Entry::Answer { step, text, .. } => Some((*step, text.as_str())),
            _ => None,
        })
        .collect();
    for step in steps(task) {
        let call = answers
            .get(&step.index)
            .copied()
            .unwrap_or(step.call.as_str());
        text += &format!(
            "\n## Step {} · {} ({} ms model, {} ms tool)\n\n{}\n\n```\n{}\n```\n",
            step.index,
            step.tool,
            step.model_ms,
            step.tool_ms,
            if step.thought.is_empty() {
                String::new()
            } else {
                format!("Thought: {}", step.thought)
            },
            call
        );
        if !step.result.is_empty() {
            text += &format!("\nResult:\n\n```\n{}\n```\n", step.result);
        }
    }
    text
}

/// A plan and each of its tasks as Markdown, to copy as evidence.
pub fn plan_transcript(project: &str, entries: &[Entry]) -> String {
    let Some(view) = super::plan::PlanView::read(entries) else {
        return "No plan yet.".into();
    };
    let mut text = format!(
        "# Bonsai Harness plan: {}\n\nProject: {project}\nModel: {}\nRequest: {}\nStatus: {:?}{}\n",
        view.id,
        view.model,
        view.request,
        view.state,
        view.reason
            .as_ref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default(),
    );
    if !view.criteria.is_empty() {
        text += &format!(
            "Done when: {}\n",
            view.criteria
                .iter()
                .map(Criterion::describe)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    for entry in entries {
        if let Entry::PlanInvalid {
            text: answer,
            error,
            ..
        } = entry
        {
            text += &format!("\nRefused plan ({error}):\n\n```\n{answer}\n```\n");
        }
    }
    if !view.thought.is_empty() {
        text += &format!("\nThought: {}\n", view.thought);
    }
    text += "\n## Plan\n\n";
    for task in &view.tasks {
        let after = if task.after.is_empty() {
            String::new()
        } else {
            format!(" (after {})", task.after.join(", "))
        };
        text += &format!(
            "- {}{after}: {} [{:?}] Done when: {}.\n",
            task.id,
            task.description,
            view.status_of(&task.id),
            task.criteria
                .iter()
                .map(Criterion::describe)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    // Every executor task of the plan, in the order they ran.
    let ids: Vec<&str> = entries
        .iter()
        .filter_map(|e| match e {
            Entry::Task { task, .. } => Some(task.as_str()),
            _ => None,
        })
        .collect();
    for id in ids {
        let task: Vec<Entry> = entries.iter().filter(|e| e.task() == id).cloned().collect();
        text += "\n";
        text += &transcript(project, &task).replacen("# Bonsai Harness:", "## Task", 1);
    }
    text
}
