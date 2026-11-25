//! Evidence and acceptance: what the environment saw happen, and the rules
//! that decide from it whether a task is complete. The model's own word
//! never counts.

use serde::{Deserialize, Serialize};

/// Something a tool call did, recorded by the environment.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// A file was written; `syntax_ok` is false when Python couldn't parse it.
    Wrote { path: String, syntax_ok: bool },
    /// A program ran; `ok` is false when it raised, or ran out of time.
    Ran {
        path: String,
        ok: bool,
        output: String,
    },
    /// The tests ran.
    Tests {
        ran: u32,
        failures: u32,
        errors: u32,
    },
    /// A run made or changed an image file (PNG size from its header).
    Produced {
        path: String,
        format: String,
        bytes: u64,
        width: Option<u32>,
        height: Option<u32>,
    },
}

impl Evidence {
    fn is_change(&self) -> bool {
        matches!(self, Evidence::Wrote { .. })
    }
}

/// One acceptance criterion, checked by the harness.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Criterion {
    /// The last test run after the last change ran at least one test, and
    /// all passed.
    TestsPass,
    /// Running `path` after the last change printed `text` and ended
    /// without an error.
    RunPrints { path: String, text: String },
    /// `path` still holds exactly `content` (a fix must not edit the test
    /// that caught the bug).
    FileUnchanged { path: String, content: String },
    /// For a task in the person's own words: the last run or test run after
    /// the last change succeeded.
    WorkRuns,
    /// A run after the last change made `path` as a `format` image ("png",
    /// "svg" ...), at least `min_width` pixels wide when its size is known.
    /// An SVG must also contain an <svg> element.
    ProducesImage {
        path: String,
        format: String,
        min_width: u32,
    },
    /// The SVG at `path` draws a bar for each of `values`: <rect> elements
    /// whose heights (or widths, for bars across) rank like the values, and
    /// each value as the text of a label. A chart whose bars ignore the
    /// numbers fails.
    SvgBars { path: String, values: Vec<String> },
    /// `path` is a unittest test file with at least one test: what the
    /// first task of a tests-first request makes, before any code.
    TestsWritten { path: String },
}

impl Criterion {
    /// For the screen and the model.
    pub fn describe(&self) -> String {
        match self {
            Criterion::TestsPass => "the tests pass (run after the last change)".into(),
            Criterion::RunPrints { path, text } => {
                format!("running {path} prints \u{201c}{text}\u{201d}")
            }
            Criterion::FileUnchanged { path, .. } => format!("{path} is left unchanged"),
            Criterion::WorkRuns => {
                "the last run or test run after the last change succeeded".into()
            }
            Criterion::ProducesImage {
                path,
                format: kind,
                min_width,
            } => {
                if *min_width > 0 {
                    format!("running it makes {path} ({kind}, at least {min_width} pixels wide)")
                } else {
                    format!("running it makes {path} ({kind})")
                }
            }
            Criterion::SvgBars { path, values } => format!(
                "{path} has a bar for each of {}, sized by its value and labelled with it",
                values.join(", ")
            ),
            Criterion::TestsWritten { path } => format!("{path} has unittest tests"),
        }
    }

    /// Ok with what proves it, or Err with what is missing.
    pub fn check(
        &self,
        evidence: &[Evidence],
        read: &dyn Fn(&str) -> Option<String>,
    ) -> Result<String, String> {
        // Only what happened after the last change counts.
        let since = evidence
            .iter()
            .rposition(Evidence::is_change)
            .map_or(0, |i| i + 1);
        let fresh = &evidence[since..];
        match self {
            Criterion::TestsPass => match fresh
                .iter()
                .rev()
                .find(|e| matches!(e, Evidence::Tests { .. }))
            {
                Some(&Evidence::Tests {
                    ran,
                    failures: 0,
                    errors: 0,
                }) if ran > 0 => Ok(format!("{ran} tests ran and all passed")),
                Some(Evidence::Tests { ran: 0, .. }) => {
                    Err("the last test run found no tests".into())
                }
                Some(Evidence::Tests {
                    failures, errors, ..
                }) => Err(format!(
                    "the last test run had {failures} failures and {errors} errors"
                )),
                _ => Err("the tests have not been run since the last change".into()),
            },
            Criterion::RunPrints { path, text } => {
                let run = fresh.iter().rev().find_map(|e| match e {
                    Evidence::Ran {
                        path: p,
                        ok,
                        output,
                    } if p == path => Some((ok, output)),
                    _ => None,
                });
                match run {
                    Some((true, output)) if output.contains(text.as_str()) => {
                        Ok(format!("{path} printed \u{201c}{text}\u{201d}"))
                    }
                    Some((true, _)) => Err(format!(
                        "{path} ran but did not print \u{201c}{text}\u{201d}"
                    )),
                    Some((false, _)) => Err(format!("{path} ended with an error")),
                    None => Err(format!("{path} has not been run since the last change")),
                }
            }
            Criterion::FileUnchanged { path, content } => match read(path) {
                Some(now) if &now == content => Ok(format!("{path} is unchanged")),
                Some(_) => Err(format!("{path} was changed; put it back as it was")),
                None => Err(format!("{path} is missing")),
            },
            Criterion::WorkRuns => match fresh
                .iter()
                .rev()
                .find(|e| matches!(e, Evidence::Ran { .. } | Evidence::Tests { .. }))
            {
                Some(Evidence::Ran { path, ok: true, .. }) => {
                    Ok(format!("{path} ran without an error"))
                }
                Some(&Evidence::Tests {
                    ran,
                    failures: 0,
                    errors: 0,
                }) if ran > 0 => Ok(format!("{ran} tests ran and all passed")),
                Some(_) => Err("the last run after the last change failed".into()),
                None => Err("nothing has been run since the last change".into()),
            },
            Criterion::ProducesImage {
                path,
                format: kind,
                min_width,
            } => {
                let made = fresh.iter().rev().find_map(|e| match e {
                    Evidence::Produced {
                        path: p,
                        format: k,
                        width,
                        ..
                    } if p == path => Some((k, *width)),
                    _ => None,
                });
                match made {
                    None => Err(format!(
                        "no run since the last change has made {path}; run the program that saves it"
                    )),
                    Some((k, _)) if k != kind => {
                        Err(format!("{path} is not a {kind} file (it looks like {k})"))
                    }
                    Some((_, Some(width))) if width < *min_width => Err(format!(
                        "{path} is {width} pixels wide; it should be at least {min_width}"
                    )),
                    Some((_, None)) if kind == "png" && *min_width > 0 => {
                        Err(format!("{path} is not a valid PNG (no size in its header)"))
                    }
                    Some(_) if kind == "svg"
                        && !read(path).is_some_and(|text| text.contains("<svg")) =>
                    {
                        Err(format!("{path} does not contain an <svg> element"))
                    }
                    Some((_, width)) => Ok(match width {
                        Some(width) => format!("running it made {path} ({kind}, {width} pixels wide)"),
                        None => format!("running it made {path} ({kind})"),
                    }),
                }
            }
            Criterion::SvgBars { path, values } => {
                let Some(svg) = read(path) else {
                    return Err(format!("{path} is missing"));
                };
                svg_bars(&svg, values).map(|()| {
                    format!(
                        "{path} has bars sized like {} and labelled with them",
                        values.join(", ")
                    )
                })
            }
            Criterion::TestsWritten { path } => {
                let Some(text) = read(path) else {
                    return Err(format!("{path} is missing: write the tests there"));
                };
                let tests = text
                    .lines()
                    .filter(|line| line.trim_start().starts_with("def test"))
                    .count();
                if tests == 0 || !text.contains("unittest") {
                    return Err(format!(
                        "{path} has no unittest tests yet (a unittest.TestCase with methods named test_...)"
                    ));
                }
                Ok(format!("{path} has {tests} tests"))
            }
        }
    }
}

/// The checks of `Criterion::SvgBars` on the SVG's text.
fn svg_bars(svg: &str, values: &[String]) -> Result<(), String> {
    // What's inside <defs> isn't drawn by itself.
    let svg = &without_defs(svg);
    let labels = svg_texts(svg);
    let missing: Vec<&str> = values
        .iter()
        .filter(|value| {
            !labels.iter().any(|label| {
                label
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
                    .any(|word| word == value.as_str())
            })
        })
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "no <text> label shows {}; label each bar with its value",
            missing.join(", ")
        ));
    }
    let numbers: Vec<f64> = values.iter().filter_map(|v| v.parse().ok()).collect();
    let rects = svg_rects(svg);
    if rects.len() < numbers.len() {
        return Err(format!(
            "it has {} <rect> elements; it needs one bar for each of the {} values",
            rects.len(),
            numbers.len()
        ));
    }
    let ranked = |sizes: &[f64]| {
        (0..numbers.len()).all(|i| {
            (i + 1..numbers.len())
                .all(|j| numbers[i].total_cmp(&numbers[j]) == sizes[i].total_cmp(&sizes[j]))
        })
    };
    // Some rects in document order (after a background, say) are the bars.
    let heights: Vec<f64> = rects.iter().map(|r| r.0).collect();
    let widths: Vec<f64> = rects.iter().map(|r| r.1).collect();
    if ordered_pick(&heights, numbers.len(), &ranked)
        || ordered_pick(&widths, numbers.len(), &ranked)
    {
        Ok(())
    } else {
        Err(
            "the bars' sizes don't follow the values; make each bar's height (or width) \
             proportional to its value"
                .into(),
        )
    }
}

/// Whether some `n` of `sizes`, kept in order, pass `ok`. Gives up past 40
/// rects, where a chart this small has no business going.
fn ordered_pick(sizes: &[f64], n: usize, ok: &dyn Fn(&[f64]) -> bool) -> bool {
    fn go(
        sizes: &[f64],
        n: usize,
        from: usize,
        picked: &mut Vec<f64>,
        ok: &dyn Fn(&[f64]) -> bool,
    ) -> bool {
        if picked.len() == n {
            return ok(picked);
        }
        (from..sizes.len()).any(|i| {
            picked.push(sizes[i]);
            let found = go(sizes, n, i + 1, picked, ok);
            picked.pop();
            found
        })
    }
    sizes.len() <= 40 && go(sizes, n, 0, &mut Vec::new(), ok)
}

/// The SVG with every <defs> element's content left out.
fn without_defs(svg: &str) -> String {
    let mut out = String::new();
    let mut rest = svg;
    while let Some(start) = rest.find("<defs") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let tag_end = rest.find('>').map_or(rest.len(), |i| i + 1);
        if rest[..tag_end].ends_with("/>") {
            rest = &rest[tag_end..];
        } else {
            // `</defs>`, with any space XML allows before the `>`.
            rest = rest
                .find("</defs")
                .and_then(|close| rest[close..].find('>').map(|end| close + end + 1))
                .map_or("", |end| &rest[end..]);
        }
    }
    out.push_str(rest);
    out
}

/// (height, width) of each <rect> that would show: both sizes positive.
fn svg_rects(svg: &str) -> Vec<(f64, f64)> {
    svg.match_indices("<rect")
        .map(|(at, _)| {
            let tag = &svg[at..];
            let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
            (svg_attribute(tag, "height"), svg_attribute(tag, "width"))
        })
        .filter(|(height, width)| {
            height.is_finite() && width.is_finite() && *height > 0.0 && *width > 0.0
        })
        .collect()
}

fn svg_attribute(tag: &str, name: &str) -> f64 {
    tag.match_indices(name)
        .filter(|(at, _)| tag[..*at].ends_with(char::is_whitespace))
        .find_map(|(at, _)| {
            let rest = tag[at + name.len()..]
                .trim_start()
                .strip_prefix('=')?
                .trim_start();
            let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let rest = &rest[1..];
            let value = &rest[..rest.find(quote)?];
            // The number of a length ("40", "40px", "10%", "1e2", "10em"):
            // the longest start that reads as a number. Only order
            // matters, and bars share a unit.
            let value = value.trim();
            (1..=value.len())
                .rev()
                .filter(|&end| value.is_char_boundary(end))
                .find_map(|end| value[..end].parse::<f64>().ok())
        })
        .unwrap_or(0.0)
}

/// The text inside each <text> element (tags inside it dropped).
fn svg_texts(svg: &str) -> Vec<String> {
    svg.match_indices("<text")
        .filter_map(|(at, _)| {
            let rest = &svg[at..];
            let open = rest.find('>')? + 1;
            let close = rest.find("</text>")?;
            let inner = rest.get(open..close)?;
            let mut out = String::new();
            let mut in_tag = false;
            for c in inner.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    c if !in_tag => out.push(c),
                    _ => {}
                }
            }
            Some(out)
        })
        .collect()
}

/// A seed file a task starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeedFile {
    pub path: &'static str,
    pub content: &'static str,
}

/// One task of the ladder: the same request every time, so runs compare.
#[derive(Clone, Debug)]
pub struct LadderTask {
    pub id: &'static str,
    pub title: &'static str,
    pub request: &'static str,
    pub seed: Vec<SeedFile>,
    pub criteria: Vec<Criterion>,
    /// Bonsai plans it as several tasks first (H4); `criteria` are then
    /// checked once every task is verified.
    pub planned: bool,
    /// It can't succeed: it passes by ending Blocked with a clear question
    /// instead of looping (H5).
    pub ends_blocked: bool,
}

/// T4's finished project, T11's seed.
const SHAPES_PY: &str = "import math\n\n\ndef rectangle_area(width, height):\n    return width * height\n\n\ndef circle_area(radius):\n    return math.pi * radius ** 2\n";

const SHAPES_MAIN_PY: &str = "import shapes\n\nprint(\"Rectangle:\", round(shapes.rectangle_area(3, 5), 2))\nprint(\"Circle:\", round(shapes.circle_area(2), 2))\n";

/// T11's seed journal: the earlier task that made the project, so the
/// harness can tell the model what was done before.
const SHAPES_JOURNAL: &str = "{\"event\":\"task\",\"task\":\"task-1\",\"description\":\"Write shapes.py with rectangle_area(width, height) and circle_area(radius), and main.py that prints the area of a 3 by 5 rectangle and of a circle of radius 2.\",\"criteria\":[],\"model\":\"earlier\",\"at_ms\":1}\n{\"event\":\"status\",\"task\":\"task-1\",\"status\":\"completed\",\"reason\":\"Verified: main.py printed 15; main.py printed 12.57.\",\"at_ms\":2}\n";

/// T8's seed: it needs a module nobody has written.
const STATS_REPORT_PY: &str =
    "from stats import mean\n\nnumbers = [2, 4, 6]\nprint(\"Average:\", mean(numbers))\n";

const AVERAGE_PY: &str = "def average(numbers):\n    \"\"\"The mean of a list of numbers.\"\"\"\n    return sum(numbers) / (len(numbers) + 1)\n";

const TEST_AVERAGE_PY: &str = "import unittest\n\nfrom average import average\n\n\nclass AverageTest(unittest.TestCase):\n    def test_three_numbers(self):\n        self.assertEqual(average([2, 4, 6]), 4)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n";

const INVENTORY_PY: &str = "stock = {\"apples\": 3, \"pears\": 0, \"plums\": 5}\n\n\ndef report(stock):\n    lines = []\n    for name, count in stock.items():\n        lines.append(name + \": \" + count)\n    return lines\n\n\nfor line in report(stock):\n    print(line)\n";

/// The ladder (docs/bonsai-harness-plan.md): T1 to T3 are H1's, T4 and
/// T5 are H2's, T6 and T7 (planned) are H4's, T8 and T9 are H5's, T11 is
/// H6's (T10, killing the app mid-plan, is a phone step).
pub fn ladder() -> Vec<LadderTask> {
    vec![
        LadderTask {
            id: "t1-hello",
            title: "T1 Hello",
            request: "Write hello.py that prints Hello from Bonsai, then run it.",
            seed: vec![],
            criteria: vec![Criterion::RunPrints {
                path: "hello.py".into(),
                text: "Hello from Bonsai".into(),
            }],
            planned: false,
            ends_blocked: false,
        },
        LadderTask {
            id: "t2-one-test",
            title: "T2 One file, one test",
            request: "Write mathutil.py with a function add(a, b) that returns the sum, and \
                      test_mathutil.py with a unittest test for it. Then run the tests.",
            seed: vec![],
            criteria: vec![Criterion::TestsPass],
            planned: false,
            ends_blocked: false,
        },
        LadderTask {
            id: "t3-one-fix",
            title: "T3 One fix",
            request: "This project has average.py and a test for it that fails. Run the tests, \
                      find the bug in average.py and fix it. Do not change the test.",
            seed: vec![
                SeedFile {
                    path: "average.py",
                    content: AVERAGE_PY,
                },
                SeedFile {
                    path: "test_average.py",
                    content: TEST_AVERAGE_PY,
                },
            ],
            criteria: vec![
                Criterion::TestsPass,
                Criterion::FileUnchanged {
                    path: "test_average.py".into(),
                    content: TEST_AVERAGE_PY.into(),
                },
            ],
            planned: false,
            ends_blocked: false,
        },
        LadderTask {
            id: "t4-two-modules",
            title: "T4 Two modules",
            request: "Write shapes.py with rectangle_area(width, height) and circle_area(radius) \
                      (use math.pi), and main.py that imports shapes and prints the area of a 3 by 5 \
                      rectangle and of a circle of radius 2, each rounded to 2 decimals. Then run main.py.",
            seed: vec![],
            criteria: vec![
                Criterion::RunPrints {
                    path: "main.py".into(),
                    text: "15".into(),
                },
                Criterion::RunPrints {
                    path: "main.py".into(),
                    text: "12.57".into(),
                },
            ],
            planned: false,
            ends_blocked: false,
        },
        LadderTask {
            id: "tp1-svg-chart",
            title: "T-P1 An SVG chart",
            request: "Write chart.py that draws a bar chart of the numbers [3, 7, 5] as chart.svg, \
                      using only the standard library (write the SVG as text), with each bar's value \
                      as a label. Then run chart.py.",
            seed: vec![],
            criteria: vec![
                Criterion::ProducesImage {
                    path: "chart.svg".into(),
                    format: "svg".into(),
                    min_width: 0,
                },
                Criterion::SvgBars {
                    path: "chart.svg".into(),
                    values: vec!["3".into(), "7".into(), "5".into()],
                },
            ],
            planned: false,
            ends_blocked: false,
        },
        LadderTask {
            id: "t5-fix-from-traceback",
            title: "T5 Fix from a traceback",
            request: "Run inventory.py. It crashes. Fix the bug so it prints each item and its count, \
                      like \"apples: 3\". There are no tests: the traceback is your guide. Change only \
                      what is broken.",
            seed: vec![SeedFile {
                path: "inventory.py",
                content: INVENTORY_PY,
            }],
            criteria: vec![
                Criterion::RunPrints {
                    path: "inventory.py".into(),
                    text: "apples: 3".into(),
                },
                Criterion::RunPrints {
                    path: "inventory.py".into(),
                    text: "plums: 5".into(),
                },
            ],
            planned: false,
            ends_blocked: false,
        },
        LadderTask {
            id: "t6-two-functions",
            title: "T6 Two functions, two tests (a plan)",
            request: "Write textutil.py with two functions: shout(text) returns the text in capitals \
                      followed by \"!\", and whisper(text) returns it in lower case followed by \"...\". \
                      Give each its own unittest file, test_shout.py and test_whisper.py. Plan it as two \
                      tasks, one per function with its test.",
            seed: vec![],
            criteria: vec![Criterion::TestsPass],
            planned: true,
            ends_blocked: false,
        },
        LadderTask {
            id: "t7-plan-with-dependency",
            title: "T7 Three tasks with a dependency (a plan)",
            request: "Write grades.py with letter(score): A for 90 and up, B for 80 and up, C for 70 and \
                      up, otherwise F. Test it in test_grades.py. Then write report.py that uses \
                      grades.letter to print each student of {\"Ana\": 93, \"Ben\": 78, \"Cy\": 85} like \
                      \"Ana: A\", and run it. Plan it as three tasks: the function, its tests, the report.",
            seed: vec![],
            criteria: vec![
                Criterion::RunPrints {
                    path: "report.py".into(),
                    text: "Ana: A".into(),
                },
                Criterion::RunPrints {
                    path: "report.py".into(),
                    text: "Ben: C".into(),
                },
                Criterion::RunPrints {
                    path: "report.py".into(),
                    text: "Cy: B".into(),
                },
            ],
            planned: true,
            ends_blocked: false,
        },
        LadderTask {
            id: "t8-replan",
            title: "T8 A plan that must change (recovery)",
            request: "Run report.py, which prints the average of some numbers, and make it work: it \
                      should print \"Average: 4\". Plan it as small tasks.",
            seed: vec![SeedFile {
                path: "report.py",
                content: STATS_REPORT_PY,
            }],
            criteria: vec![Criterion::RunPrints {
                path: "report.py".into(),
                text: "Average: 4".into(),
            }],
            planned: true,
            ends_blocked: false,
        },
        LadderTask {
            id: "t9-blocked",
            title: "T9 A task that can't succeed (blocked)",
            request: "Write weather.py that downloads today's weather for Edinburgh from \
                      https://wttr.in/Edinburgh?format=3 and prints it, then run it.",
            seed: vec![],
            criteria: vec![],
            planned: true,
            ends_blocked: true,
        },
        LadderTask {
            id: "t11-second-request",
            title: "T11 A second request on an earlier project",
            request: "Now add square_area(side) to shapes.py, and make main.py also print the area of a \
                      4 by 4 square. Keep what main.py prints already.",
            seed: vec![
                SeedFile {
                    path: "shapes.py",
                    content: SHAPES_PY,
                },
                SeedFile {
                    path: "main.py",
                    content: SHAPES_MAIN_PY,
                },
                SeedFile {
                    path: ".harness/journal.jsonl",
                    content: SHAPES_JOURNAL,
                },
            ],
            criteria: vec![
                Criterion::RunPrints {
                    path: "main.py".into(),
                    text: "16".into(),
                },
                Criterion::RunPrints {
                    path: "main.py".into(),
                    text: "15".into(),
                },
                Criterion::RunPrints {
                    path: "main.py".into(),
                    text: "12.57".into(),
                },
            ],
            planned: false,
            ends_blocked: false,
        },
    ]
}

/// One answer of a known-good run: the JSON a model would write.
fn answer(thought: &str, tool: &str, args: &[(&str, &str)]) -> String {
    let mut text = format!(
        "{{\"thought\": {}, \"tool\": {}",
        serde_json::to_string(thought).unwrap_or_default(),
        serde_json::to_string(tool).unwrap_or_default()
    );
    for (name, value) in args {
        text.push_str(&format!(
            ", {}: {}",
            serde_json::to_string(name).unwrap_or_default(),
            serde_json::to_string(value).unwrap_or_default()
        ));
    }
    text.push('}');
    text
}

/// Scripted T-P1's chart.py: three labelled bars as SVG text.
const SCRIPTED_CHART_PY: &str = "values = [3, 7, 5]\nbars = []\nfor i, v in enumerate(values):\n    x = 20 + i * 60\n    h = v * 20\n    bars.append(f'<rect x=\"{x}\" y=\"{160 - h}\" width=\"40\" height=\"{h}\" fill=\"teal\"/>')\n    bars.append(f'<text x=\"{x + 20}\" y=\"175\" text-anchor=\"middle\">{v}</text>')\nsvg = '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"180\">' + ''.join(bars) + '</svg>'\nwith open('chart.svg', 'w') as f:\n    f.write(svg)\nprint('Wrote chart.svg')\n";

/// A probe the scripted T1 runs first: on the phone it shows the guard
/// blocking a write outside the project and a network connection.
const GUARD_PROBE: &str = "import socket\n\nattempts = [\n    (\"write outside the project\", lambda: open(\"../escape.txt\", \"w\")),\n    (\"network\", lambda: socket.create_connection((\"example.com\", 80), 2)),\n]\nfor name, attempt in attempts:\n    try:\n        attempt()\n        print(name, \"ALLOWED\")\n    except Exception as error:\n        print(name, \"blocked:\", type(error).__name__)\n";

/// The answers of a known-good run of a ladder task, for the Scripted
/// model: it proves the tools, the guard, the evidence checks and the
/// journal on the phone apart from any real model (an idea from
/// Terrarium's self-test). T3 claims done too early once, to show the
/// harness sending it back.
pub fn script(task_id: &str) -> Vec<String> {
    let finish = |summary: &str| answer("Done.", super::calls::FINISH, &[("summary", summary)]);
    match task_id {
        "t1-hello" => vec![
            answer(
                "Check the guard first.",
                "write_file",
                &[("path", "probe.py"), ("content", GUARD_PROBE)],
            ),
            answer("Run the probe.", "run_python", &[("path", "probe.py")]),
            answer(
                "Write the program.",
                "write_file",
                &[
                    ("path", "hello.py"),
                    ("content", "print(\"Hello from Bonsai\")\n"),
                ],
            ),
            answer("Run it.", "run_python", &[("path", "hello.py")]),
            finish("hello.py prints Hello from Bonsai."),
        ],
        "t2-one-test" => vec![
            answer(
                "Write the function.",
                "write_file",
                &[
                    ("path", "mathutil.py"),
                    ("content", "def add(a, b):\n    return a + b\n"),
                ],
            ),
            answer(
                "Write its test.",
                "write_file",
                &[
                    ("path", "test_mathutil.py"),
                    (
                        "content",
                        "import unittest\n\nfrom mathutil import add\n\n\nclass AddTest(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(2, 3), 5)\n",
                    ),
                ],
            ),
            answer("Run the tests.", "run_tests", &[]),
            finish("add and its test pass."),
        ],
        "t3-one-fix" => vec![
            finish("Fixed the bug."),
            answer("See what fails.", "run_tests", &[]),
            answer(
                "Divide by the count.",
                "write_file",
                &[
                    ("path", "average.py"),
                    (
                        "content",
                        "def average(numbers):\n    \"\"\"The mean of a list of numbers.\"\"\"\n    return sum(numbers) / len(numbers)\n",
                    ),
                ],
            ),
            answer("Check the fix.", "run_tests", &[]),
            finish("average divides by the count; the test passes."),
        ],
        "t4-two-modules" => vec![
            answer(
                "Write the module.",
                "write_file",
                &[
                    ("path", "shapes.py"),
                    ("content", SHAPES_PY),
                ],
            ),
            answer(
                "Write the program that uses it.",
                "write_file",
                &[
                    ("path", "main.py"),
                    ("content", SHAPES_MAIN_PY),
                ],
            ),
            answer("Run it.", "run_python", &[("path", "main.py")]),
            finish("main.py prints both areas."),
        ],
        "tp1-svg-chart" => vec![
            answer(
                "Write the chart as SVG text.",
                "write_file",
                &[
                    ("path", "chart.py"),
                    ("content", SCRIPTED_CHART_PY),
                ],
            ),
            answer("Run it to make chart.svg.", "run_python", &[("path", "chart.py")]),
            finish("chart.py writes chart.svg with three labelled bars."),
        ],
        // The first edit misremembers the line (a name that isn't there), to
        // show the "did you mean" answer; the second copies it exactly.
        "t5-fix-from-traceback" => vec![
            answer("See the crash.", "run_python", &[("path", "inventory.py")]),
            answer(
                "Convert the amount to text.",
                "edit_file",
                &[
                    ("path", "inventory.py"),
                    ("old", "        lines.append(name + ': ' + amount)\n"),
                    ("new", "        lines.append(name + ': ' + str(amount))\n"),
                ],
            ),
            answer(
                "Copy the line exactly.",
                "edit_file",
                &[
                    ("path", "inventory.py"),
                    ("old", "        lines.append(name + \": \" + count)\n"),
                    ("new", "        lines.append(name + \": \" + str(count))\n"),
                ],
            ),
            answer("Run it again.", "run_python", &[("path", "inventory.py")]),
            finish("count is converted with str(); every item prints."),
        ],
        // A plan, then each task with a fresh context.
        "t6-two-functions" => vec![
            plan_answer(
                "One function and its test at a time.",
                &[
                    ("shout", "Write textutil.py with shout(text) returning text.upper() + '!', and test_shout.py testing it with unittest. Run the tests.", &[], "tests_pass"),
                    ("whisper", "Add whisper(text) to textutil.py returning text.lower() + '...', keeping shout, and write test_whisper.py testing it. Run the tests.", &["shout"], "tests_pass"),
                ],
            ),
            answer(
                "Write shout.",
                "write_file",
                &[
                    ("path", "textutil.py"),
                    ("content", "def shout(text):\n    return text.upper() + \"!\"\n"),
                ],
            ),
            answer(
                "Test it.",
                "write_file",
                &[("path", "test_shout.py"), ("content", TEST_SHOUT_PY)],
            ),
            answer("Run the tests.", "run_tests", &[]),
            finish("shout and its test pass."),
            answer(
                "Add whisper after shout.",
                "edit_file",
                &[
                    ("path", "textutil.py"),
                    ("old", "    return text.upper() + \"!\"\n"),
                    (
                        "new",
                        "    return text.upper() + \"!\"\n\n\ndef whisper(text):\n    return text.lower() + \"...\"\n",
                    ),
                ],
            ),
            answer(
                "Test whisper.",
                "write_file",
                &[("path", "test_whisper.py"), ("content", TEST_WHISPER_PY)],
            ),
            answer("Run all the tests.", "run_tests", &[]),
            finish("Both functions and their tests pass."),
        ],
        "t7-plan-with-dependency" => vec![
            plan_answer(
                "The function first; its tests and the report both need it.",
                &[
                    ("letter", "Write grades.py with letter(score): 'A' for 90 and up, 'B' for 80 and up, 'C' for 70 and up, otherwise 'F'. Run grades.py.", &[], "work_runs"),
                    ("tests", "Write test_grades.py testing grades.letter at 95, 85, 75 and 50 with unittest. Run the tests.", &["letter"], "tests_pass"),
                    ("report", "Write report.py that prints each student of {'Ana': 93, 'Ben': 78, 'Cy': 85} as 'Ana: A' using grades.letter. Run report.py.", &["letter"], "report"),
                ],
            ),
            answer(
                "Write letter.",
                "write_file",
                &[("path", "grades.py"), ("content", GRADES_PY)],
            ),
            answer("Check it runs.", "run_python", &[("path", "grades.py")]),
            finish("grades.py runs."),
            answer(
                "Test each band.",
                "write_file",
                &[("path", "test_grades.py"), ("content", TEST_GRADES_PY)],
            ),
            answer("Run the tests.", "run_tests", &[]),
            finish("letter's tests pass."),
            answer(
                "Write the report.",
                "write_file",
                &[("path", "report.py"), ("content", REPORT_PY)],
            ),
            answer("Run it.", "run_python", &[("path", "report.py")]),
            finish("report.py prints every student."),
        ],
        // Reads one function instead of the file, then adds beside it.
        "t11-second-request" => vec![
            answer(
                "See how the areas are written.",
                "read_symbol",
                &[("path", "shapes.py"), ("name", "circle_area")],
            ),
            answer(
                "Add square_area after circle_area.",
                "edit_file",
                &[
                    ("path", "shapes.py"),
                    ("old", "    return math.pi * radius ** 2\n"),
                    (
                        "new",
                        "    return math.pi * radius ** 2\n\n\ndef square_area(side):\n    return side * side\n",
                    ),
                ],
            ),
            answer(
                "Print the square too.",
                "edit_file",
                &[
                    ("path", "main.py"),
                    ("old", "print(\"Circle:\", round(shapes.circle_area(2), 2))\n"),
                    (
                        "new",
                        "print(\"Circle:\", round(shapes.circle_area(2), 2))\nprint(\"Square:\", shapes.square_area(4))\n",
                    ),
                ],
            ),
            answer("Run it.", "run_python", &[("path", "main.py")]),
            finish("main.py prints all three areas."),
        ],
        // The plan misses a module; the executor gives up, Recovery replans.
        "t8-replan" => vec![
            plan_answer(
                "Run it first and see.",
                &[("run-report", "Run report.py and make sure it prints Average: 4.", &[], "average")],
            ),
            answer("Run it.", "run_python", &[("path", "report.py")]),
            answer(
                "Missing module.",
                super::calls::GIVE_UP,
                &[("reason", "report.py imports stats, but there is no stats.py in the project.")],
            ),
            recovery_answer(
                "No task makes the stats module report.py needs.",
                "replan",
                "stats.py with mean(numbers) is missing.",
            ),
            plan_answer(
                "Write the missing module and its test, then run the report.",
                &[
                    ("stats", "Write stats.py with mean(numbers) returning sum(numbers) / len(numbers), and test_stats.py testing it with unittest. Run the tests.", &[], "tests_pass"),
                    ("report", "Run report.py; it should print Average: 4.", &["stats"], "average"),
                ],
            ),
            answer(
                "Write mean.",
                "write_file",
                &[
                    ("path", "stats.py"),
                    ("content", "def mean(numbers):\n    return sum(numbers) / len(numbers)\n"),
                ],
            ),
            answer(
                "Test it.",
                "write_file",
                &[("path", "test_stats.py"), ("content", TEST_STATS_PY)],
            ),
            answer("Run the tests.", "run_tests", &[]),
            finish("mean and its test pass."),
            answer("Run the report.", "run_python", &[("path", "report.py")]),
            finish("report.py prints Average: 4.0."),
        ],
        // The network is off: it ends Blocked with a question; answered,
        // it goes on with a sample.
        "t9-blocked" => vec![
            plan_answer(
                "One small program.",
                &[("weather", "Write weather.py that downloads https://wttr.in/Edinburgh?format=3 with urllib and prints it. Run it.", &[], "work_runs")],
            ),
            answer(
                "Download and print.",
                "write_file",
                &[("path", "weather.py"), ("content", WEATHER_PY)],
            ),
            answer("Run it.", "run_python", &[("path", "weather.py")]),
            answer(
                "The network is blocked.",
                super::calls::GIVE_UP,
                &[("reason", "Python's network is blocked here, so wttr.in can't be reached.")],
            ),
            recovery_answer(
                "Only the person can say what to do without the network.",
                "blocked",
                "Python can't reach the internet on this phone. Should I print a sample forecast instead?",
            ),
            answer(
                "Print a sample, as the person said.",
                "write_file",
                &[
                    ("path", "weather.py"),
                    ("content", "print(\"Edinburgh: (sample) cloudy, 9°C\")\n"),
                ],
            ),
            answer("Run it.", "run_python", &[("path", "weather.py")]),
            finish("weather.py prints a sample forecast."),
        ],
        _ => vec![],
    }
}

/// A recovery answer in Recovery mode's grammar: `decision` is retry,
/// replan or blocked, and `text` its diagnosis or question.
fn recovery_answer(thought: &str, decision: &str, text: &str) -> String {
    let field = if decision == "blocked" {
        "question"
    } else {
        "diagnosis"
    };
    format!(
        "{{\"thought\": {}, \"decision\": {}, \"{field}\": {}}}",
        serde_json::to_string(thought).unwrap_or_default(),
        serde_json::to_string(decision).unwrap_or_default(),
        serde_json::to_string(text).unwrap_or_default()
    )
}

const TEST_STATS_PY: &str = "import unittest\n\nfrom stats import mean\n\n\nclass MeanTest(unittest.TestCase):\n    def test_mean(self):\n        self.assertEqual(mean([2, 4, 6]), 4)\n";

const WEATHER_PY: &str = "import urllib.request\n\nwith urllib.request.urlopen(\"https://wttr.in/Edinburgh?format=3\", timeout=5) as reply:\n    print(reply.read().decode())\n";

/// A plan answer in the planner grammar's shape: tasks as (id, what to do,
/// after, check), where check is "tests_pass", "work_runs", "report"
/// (report.py prints "Ana: A") or "average" (report.py prints "Average: 4").
fn plan_answer(thought: &str, tasks: &[(&str, &str, &[&str], &str)]) -> String {
    let tasks: Vec<serde_json::Value> = tasks
        .iter()
        .map(|(id, what, after, check)| {
            let check = match *check {
                "report" => serde_json::json!({"check": "run_prints", "path": "report.py", "text": "Ana: A"}),
                "average" => serde_json::json!({"check": "run_prints", "path": "report.py", "text": "Average: 4"}),
                other => serde_json::json!({ "check": other }),
            };
            serde_json::json!({"id": id, "do": what, "after": after, "checks": [check]})
        })
        .collect();
    format!(
        "{{\"thought\": {}, \"tasks\": {}}}",
        serde_json::to_string(thought).unwrap_or_default(),
        serde_json::to_string(&tasks).unwrap_or_default()
    )
}

const TEST_SHOUT_PY: &str = "import unittest\n\nfrom textutil import shout\n\n\nclass ShoutTest(unittest.TestCase):\n    def test_shout(self):\n        self.assertEqual(shout(\"hi\"), \"HI!\")\n";

const TEST_WHISPER_PY: &str = "import unittest\n\nfrom textutil import whisper\n\n\nclass WhisperTest(unittest.TestCase):\n    def test_whisper(self):\n        self.assertEqual(whisper(\"HI\"), \"hi...\")\n";

const GRADES_PY: &str = "def letter(score):\n    if score >= 90:\n        return \"A\"\n    if score >= 80:\n        return \"B\"\n    if score >= 70:\n        return \"C\"\n    return \"F\"\n";

const TEST_GRADES_PY: &str = "import unittest\n\nfrom grades import letter\n\n\nclass LetterTest(unittest.TestCase):\n    def test_bands(self):\n        self.assertEqual(letter(95), \"A\")\n        self.assertEqual(letter(85), \"B\")\n        self.assertEqual(letter(75), \"C\")\n        self.assertEqual(letter(50), \"F\")\n";

const REPORT_PY: &str = "from grades import letter\n\nscores = {\"Ana\": 93, \"Ben\": 78, \"Cy\": 85}\nfor name, score in scores.items():\n    print(name + \": \" + letter(score))\n";
