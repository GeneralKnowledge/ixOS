use super::*;
use HighlightKind::{Comment, Decorator, Definition, Keyword, Number};
const STR: HighlightKind = HighlightKind::String;

/// The coloured pieces of `source` as (text, kind), for readable tests.
fn pieces(source: &str) -> Vec<(String, HighlightKind)> {
    let chars: Vec<char> = source.chars().collect();
    tokens(&chars)
        .into_iter()
        .map(|t| (chars[t.start..t.end].iter().collect(), t.kind))
        .collect()
}

fn p(text: &str, kind: HighlightKind) -> (String, HighlightKind) {
    (text.to_string(), kind)
}

#[test]
fn keywords_names_and_definitions() {
    assert_eq!(
        pieces(
            "def greet(name):\n    return None if name is not True else name\nclass Box: pass\n"
        ),
        vec![
            p("def", Keyword),
            p("greet", Definition),
            p("return", Keyword),
            p("None", Keyword),
            p("if", Keyword),
            p("is", Keyword),
            p("not", Keyword),
            p("True", Keyword),
            p("else", Keyword),
            p("class", Keyword),
            p("Box", Definition),
            p("pass", Keyword),
        ]
    );
    // Names that only contain a keyword, and soft keywords, stay plain.
    assert!(pieces("format = iffy + definitely; match = case; print(_)").is_empty());
}

#[test]
fn strings_of_every_kind() {
    assert_eq!(
        pieces(r#"a = 'one' + "two" + b'\x00' + rb"\d" + u'x'"#),
        vec![
            p("'one'", STR),
            p("\"two\"", STR),
            p("b'\\x00'", STR),
            p("rb\"\\d\"", STR),
            p("u'x'", STR),
        ]
    );
    assert_eq!(
        pieces("s = '''a\n'b' \"c\"\n''' # done"),
        vec![p("'''a\n'b' \"c\"\n'''", STR), p("# done", Comment)]
    );
    assert_eq!(pieces(r#"'it\'s' x"#), vec![p(r#"'it\'s'"#, STR)]);
    // A raw string still can't end on an escaped quote.
    assert_eq!(pieces(r#"r'\'' x"#), vec![p(r#"r'\''"#, STR)]);
    // `ub` is not a prefix, so it's a name and then a string.
    assert_eq!(pieces("ub'x'"), vec![p("'x'", STR)]);
}

#[test]
fn f_strings_hold_expressions_with_quotes_and_braces() {
    assert_eq!(
        pieces(r#"f"{d["key"]:>{width}} {{literal}} {x!r}" + 1"#),
        vec![
            p(r#"f"{d["key"]:>{width}} {{literal}} {x!r}""#, STR),
            p("1", Number)
        ]
    );
    assert_eq!(pieces("f'{\"}\"}' x"), vec![p("f'{\"}\"}'", STR)]);
}

#[test]
fn unclosed_strings_stop_at_the_line_or_the_end() {
    assert_eq!(
        pieces("x = 'open\nif y:"),
        vec![p("'open", STR), p("if", Keyword)]
    );
    assert_eq!(
        pieces("x = \"\"\"open\nif y:"),
        vec![p("\"\"\"open\nif y:", STR)]
    );
}

#[test]
fn numbers() {
    assert_eq!(
        pieces("1 + 2.5 - .5e-3 * 1_000 + 0xFF_FF + 0b10 + 0o7 + 3j + 1e10 + 7."),
        vec![
            p("1", Number),
            p("2.5", Number),
            p(".5e-3", Number),
            p("1_000", Number),
            p("0xFF_FF", Number),
            p("0b10", Number),
            p("0o7", Number),
            p("3j", Number),
            p("1e10", Number),
            p("7.", Number),
        ]
    );
    // Digits inside a name aren't a number.
    assert!(pieces("x2 = y_3").is_empty());
}

#[test]
fn decorators_only_at_the_start_of_a_line() {
    assert_eq!(
        pieces("@functools.cache\n  @property\ndef f(): return a @ b"),
        vec![
            p("@functools.cache", Decorator),
            p("@property", Decorator),
            p("def", Keyword),
            p("f", Definition),
            p("return", Keyword),
        ]
    );
}

#[test]
fn ranges_are_utf16_code_units() {
    // "é" is one unit, "🐍" two.
    let spans = python_highlight("é = '🐍' # 🐍\nif 1: pass".into());
    assert_eq!(
        spans,
        vec![
            HighlightSpan {
                start: 4,
                length: 4,
                kind: STR
            },
            HighlightSpan {
                start: 9,
                length: 4,
                kind: Comment
            },
            HighlightSpan {
                start: 14,
                length: 2,
                kind: Keyword
            },
            HighlightSpan {
                start: 17,
                length: 1,
                kind: Number
            },
            HighlightSpan {
                start: 20,
                length: 4,
                kind: Keyword
            },
        ]
    );
    let text: Vec<u16> = "é = '🐍' # 🐍\nif 1: pass".encode_utf16().collect();
    assert_eq!(String::from_utf16(&text[4..8]).unwrap(), "'🐍'");
}

#[test]
fn the_keyword_list_is_pythons() {
    let Some(out) = python(
        &["-c", "import keyword; print(' '.join(keyword.kwlist))"],
        "",
    ) else {
        return;
    };
    let theirs: Vec<&str> = out.split_whitespace().collect();
    assert_eq!(theirs, KEYWORDS);
}

/// The samples Python Lab offers, plus the awkward cases above, coloured
/// the same as Python's own tokenizer reads them.
#[test]
fn strings_comments_and_numbers_match_python_tokenize() {
    let mut sources: Vec<String> = crate::harness::samples::python_lab_graphics_samples()
        .into_iter()
        .map(|s| s.code)
        .collect();
    sources.push(TRICKY.to_string());
    for source in sources {
        let Some(out) = python(&["-c", TOKENIZE], &source) else {
            return;
        };
        let theirs: Vec<(usize, usize, &str)> = out
            .lines()
            .map(|line| {
                let mut parts = line.split(' ');
                let kind = parts.next().unwrap();
                let start = parts.next().unwrap().parse().unwrap();
                let end = parts.next().unwrap().parse().unwrap();
                (start, end, kind)
            })
            .collect();
        let chars: Vec<char> = source.chars().collect();
        let ours: Vec<(usize, usize, &str)> = tokens(&chars)
            .into_iter()
            .filter_map(|t| {
                let kind = match t.kind {
                    HighlightKind::String => "STRING",
                    Comment => "COMMENT",
                    Number => "NUMBER",
                    _ => return None,
                };
                Some((t.start, t.end, kind))
            })
            .collect();
        assert_eq!(ours, theirs, "in:\n{source}");
    }
}

const TRICKY: &str = r#"
@dataclass
class Point:  # a comment with 'quotes' and "more"
    x: float = 1_000.5e-3
    y: complex = 2j

def show(p, width=10):
    """Triple 'quoted' "docstring"
    over lines."""
    name = f"{p.x!r:>{width}} {{braces}} {'nested'} {p.y:.2f}"
    raw = rb'\d+' + Rb"\'"
    path = r"C:\temp" + '\\'
    total = 0xff + 0o17 + 0b1010 + .5 + 7. + 1e9
    return name, raw, path, total  # done
"#;

/// The tokens Python's `tokenize` finds as (kind, start, end) in chars,
/// with an f-string's pieces joined into one STRING.
const TOKENIZE: &str = r#"
import io, sys, tokenize
source = sys.stdin.read()
starts, at = [], 0
for line in io.StringIO(source).readlines():
    starts.append(at)
    at += len(line)
starts.append(at)
def pos(rc):
    return starts[rc[0] - 1] + rc[1]
names = {getattr(tokenize, n, None) for n in ("FSTRING_START", "TSTRING_START")} - {None}
ends = {getattr(tokenize, n, None) for n in ("FSTRING_END", "TSTRING_END")} - {None}
depth, begin = 0, None
for t in tokenize.generate_tokens(io.StringIO(source).readline):
    if t.type in names:
        if depth == 0:
            begin = pos(t.start)
        depth += 1
    elif t.type in ends:
        depth -= 1
        if depth == 0:
            print("STRING", begin, pos(t.end))
    elif depth == 0 and t.type in (tokenize.STRING, tokenize.COMMENT, tokenize.NUMBER):
        print(tokenize.tok_name[t.type], pos(t.start), pos(t.end))
"#;

/// Runs `python3` with `stdin`, or None (with a note) when it isn't
/// installed, so the comparison runs wherever Python is.
fn python(args: &[&str], stdin: &str) -> Option<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let child = Command::new("python3")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("python3 not found: skipping the comparison with Python's tokenizer");
        return None;
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "python3 failed");
    Some(String::from_utf8(out.stdout).unwrap())
}
