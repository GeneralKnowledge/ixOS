//! Sums: a calculator rebuilt from the outside.
//!
//! Every rule here comes from the Calculator expedition's evidence
//! (`docs/research/expeditions/calculator/`): the findings register and the
//! answers recorded from the reference on an iPhone. Where the evidence ran
//! out the rule is a guess, and it says so with `GUESS:`; the reconstruction
//! record lists them all. The words people see ("Undefined") are the
//! reference's, kept here because the test compares them exactly; Swift
//! decides how they look.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

/// A key on the keypad. The names are the reference's accessibility ids
/// (finding 1), which the recreation reuses so one sequence table drives both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Digit(u8),
    Decimal,
    Add,
    Subtract,
    Multiply,
    Divide,
    Equals,
    Percent,
    Negate,
    Delete,
    /// "C" while there is an entry to clear, "All Clear" otherwise (finding 8).
    Clear,
}

impl Key {
    /// The key for a reference id such as `Seven` or `AllClear`.
    pub fn from_id(id: &str) -> Option<Key> {
        let digit = [
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
        ]
        .iter()
        .position(|name| *name == id);
        if let Some(d) = digit {
            return Some(Key::Digit(d as u8));
        }
        Some(match id {
            "Decimal" => Key::Decimal,
            "Add" => Key::Add,
            "Subtract" => Key::Subtract,
            "Multiply" => Key::Multiply,
            "Divide" => Key::Divide,
            "Equals" => Key::Equals,
            "Percent" => Key::Percent,
            "Negate" => Key::Negate,
            "Delete" => Key::Delete,
            "Clear" | "AllClear" => Key::Clear,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Subtract => "-",
            Op::Multiply => "×",
            Op::Divide => "÷",
        }
    }

    fn binds_tightly(self) -> bool {
        matches!(self, Op::Multiply | Op::Divide)
    }
}

/// A number as it was typed: digits, an optional point, a sign and a
/// percent mark. Kept as text so "3.0" stays "3.0" (finding: trailing zero).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Entry {
    digits: String,
    negative: bool,
    percent: bool,
}

impl Entry {
    /// A result carried into the next sum. The reference's expression line
    /// shows 1 ÷ 999,999,999 as 0.000000001000000001: ten significant
    /// digits, in plain notation (sequence tiny-quotient).
    /// GUESS: the carried value is rounded the same way, not just shown so.
    fn from_value(value: Decimal) -> Entry {
        let carried = value.round_sf(10).unwrap_or(value);
        Entry {
            digits: carried.abs().normalize().to_string(),
            negative: value.is_sign_negative() && !value.is_zero(),
            percent: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.digits.is_empty()
    }

    fn number(&self) -> Decimal {
        let text = if self.digits.is_empty() || self.digits == "." {
            "0"
        } else {
            self.digits.as_str()
        };
        let text = text.strip_suffix('.').unwrap_or(text);
        let value = Decimal::from_str(text).unwrap_or(Decimal::ZERO);
        if self.negative {
            -value
        } else {
            value
        }
    }

    /// What the display shows while typing: grouped digits, "0." for a bare
    /// point, "(-5)" for a negative entry, "50%" with the mark.
    fn shown(&self) -> String {
        let digits = if self.digits.is_empty() {
            "0".to_owned()
        } else {
            group_typed(&self.digits)
        };
        let mut text = if self.negative {
            format!("(-{digits})")
        } else {
            digits
        };
        if self.percent {
            text.push('%');
        }
        text
    }
}

/// Typed digits with thousands separators; "007" shows as "7", ".5" as "0.5".
fn group_typed(digits: &str) -> String {
    let (int, frac) = match digits.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (digits, None),
    };
    let int = int.trim_start_matches('0');
    let int = if int.is_empty() { "0" } else { int };
    let mut out = group(int);
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

fn group(int: &str) -> String {
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A finished result as the display shows it.
///
/// Observed: up to 11 decimal places, last digit rounded (finding 28);
/// thousands separators; scientific notation with up to 10 significant
/// digits for 9.99999998e17 and 1.000000002e-18 (finding 29).
/// GUESS: where plain notation stops (here at 1e16, and below what 11
/// decimal places can show). No recorded sum sits near either edge.
pub fn format_result(value: Decimal) -> String {
    if value.is_zero() {
        return "0".to_owned();
    }
    let abs = value.abs();
    let rounded = abs.round_dp_with_strategy(11, RoundingStrategy::MidpointAwayFromZero);
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if abs >= Decimal::from(10_000_000_000_000_000u64) || rounded.is_zero() {
        return format!("{sign}{}", scientific(abs));
    }
    let text = rounded.normalize().to_string();
    let (int, frac) = match text.split_once('.') {
        Some((i, f)) => (i.to_owned(), Some(f.to_owned())),
        None => (text, None),
    };
    let mut out = format!("{sign}{}", group(&int));
    if let Some(f) = frac {
        out.push('.');
        out.push_str(&f);
    }
    out
}

/// "9.99999998e17": a mantissa of up to 10 significant digits, trailing
/// zeros dropped.
fn scientific(abs: Decimal) -> String {
    let mut exponent: i32 = 0;
    let ten = Decimal::TEN;
    let mut mantissa = abs;
    while mantissa >= ten {
        mantissa /= ten;
        exponent += 1;
    }
    while mantissa < Decimal::ONE {
        mantissa *= ten;
        exponent -= 1;
    }
    let mut mantissa = mantissa
        .round_dp_with_strategy(9, RoundingStrategy::MidpointAwayFromZero)
        .normalize();
    if mantissa >= ten {
        mantissa /= ten;
        exponent += 1;
    }
    format!("{mantissa}e{exponent}")
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(Entry),
    Op(Op),
}

#[derive(Debug, Clone, PartialEq)]
enum Shown {
    /// The number being typed.
    Entry,
    /// A finished result, or an error word.
    Result(String),
}

/// The calculator's whole state. Rust owns it; Swift only shows
/// `display()` and `expression()` and sends key presses.
#[derive(Debug, Clone)]
pub struct Calculator {
    tokens: Vec<Token>,
    entry: Entry,
    shown: Shown,
    /// The line above the display after equals ("12+7").
    last_expression: Option<String>,
    /// For repeated equals: the last operator and its right-hand operand
    /// (finding 32).
    repeat: Option<(Op, Entry)>,
    /// The last result, which an operator carries on from.
    result: Option<Decimal>,
}

impl Default for Calculator {
    fn default() -> Self {
        Calculator::new()
    }
}

impl Calculator {
    pub fn new() -> Calculator {
        Calculator {
            tokens: Vec::new(),
            entry: Entry::default(),
            shown: Shown::Entry,
            last_expression: None,
            repeat: None,
            result: None,
        }
    }

    /// What the main display shows.
    pub fn display(&self) -> String {
        match &self.shown {
            Shown::Result(text) => text.clone(),
            Shown::Entry => {
                if !self.entry.is_empty() || self.entry.negative || self.entry.percent {
                    self.entry.shown()
                } else if let Some(n) = self.tokens.iter().rev().find_map(|t| match t {
                    Token::Number(n) => Some(n),
                    Token::Op(_) => None,
                }) {
                    // GUESS: after an operator the display keeps the number
                    // before it until the next digit. Not recorded.
                    n.shown()
                } else {
                    "0".to_owned()
                }
            }
        }
    }

    /// The "Last Expression" line, when there is one (finding 12).
    pub fn expression(&self) -> Option<String> {
        self.last_expression.clone()
    }

    /// Whether the clear key is "C" (something to clear) or "All Clear".
    pub fn clears_entry(&self) -> bool {
        !self.entry.is_empty() || self.entry.negative || self.entry.percent
    }

    pub fn press(&mut self, key: Key) {
        match key {
            Key::Digit(d) => self.digit(d),
            Key::Decimal => self.point(),
            Key::Add => self.operator(Op::Add),
            Key::Subtract => self.operator(Op::Subtract),
            Key::Multiply => self.operator(Op::Multiply),
            Key::Divide => self.operator(Op::Divide),
            Key::Equals => self.equals(),
            Key::Percent => self.percent(),
            Key::Negate => self.negate(),
            Key::Delete => self.delete(),
            Key::Clear => self.clear(),
        }
    }

    fn after_result(&self) -> bool {
        matches!(self.shown, Shown::Result(_))
    }

    /// A digit after a result starts again (sequence new-number-after-equals).
    fn start_fresh_if_result(&mut self) {
        if self.after_result() {
            *self = Calculator::new();
        }
    }

    fn digit(&mut self, d: u8) {
        self.start_fresh_if_result();
        if self.entry.percent {
            // GUESS: a digit after % is ignored. Not recorded.
            return;
        }
        self.entry.digits.push(char::from(b'0' + d));
        self.shown = Shown::Entry;
    }

    fn point(&mut self) {
        self.start_fresh_if_result();
        // A second point is ignored ("1..5" shows 1.5).
        if !self.entry.digits.contains('.') && !self.entry.percent {
            self.entry.digits.push('.');
        }
        self.shown = Shown::Entry;
    }

    fn operator(&mut self, op: Op) {
        if let (Shown::Result(_), Some(value)) = (&self.shown, self.result) {
            // Carry on from the result (2+3= +1= gives 6, "5+1").
            self.tokens = vec![Token::Number(Entry::from_value(value)), Token::Op(op)];
            self.entry = Entry::default();
            self.shown = Shown::Entry;
            self.last_expression = None;
            return;
        }
        self.shown = Shown::Entry;
        if !self.entry.is_empty() || self.entry.negative || self.entry.percent {
            self.tokens
                .push(Token::Number(std::mem::take(&mut self.entry)));
            self.tokens.push(Token::Op(op));
        } else if let Some(Token::Op(last)) = self.tokens.last_mut() {
            // A second operator replaces the first (5 + × 2 = 10).
            *last = op;
        } else if self.tokens.is_empty() {
            // An operator first starts from 0 ("+3=" shows "0+3").
            self.tokens.push(Token::Number(Entry {
                digits: "0".to_owned(),
                ..Entry::default()
            }));
            self.tokens.push(Token::Op(op));
        }
    }

    fn percent(&mut self) {
        if self.after_result() {
            // GUESS: % on a result is not recorded; treat the result as typed.
            if let Some(value) = self.result {
                *self = Calculator::new();
                self.entry = Entry::from_value(value);
            }
        }
        if self.entry.is_empty() {
            return;
        }
        self.entry.percent = true;
        self.shown = Shown::Entry;
    }

    fn negate(&mut self) {
        if self.after_result() {
            // GUESS: ± on a result negates it. Not recorded.
            if let Some(value) = self.result {
                *self = Calculator::new();
                self.entry = Entry::from_value(-value);
            }
            return;
        }
        // ± on nothing stays 0: no minus zero (finding 35).
        if self.entry.is_empty() {
            return;
        }
        self.entry.negative = !self.entry.negative;
        self.shown = Shown::Entry;
    }

    fn delete(&mut self) {
        if self.after_result() {
            // After a result, Delete clears it to 0 (finding 37).
            *self = Calculator::new();
            return;
        }
        if self.entry.percent {
            self.entry.percent = false;
        } else if !self.entry.digits.is_empty() {
            self.entry.digits.pop();
            if self.entry.digits.is_empty() {
                self.entry.negative = false;
            }
        } else if let Some(Token::Op(_)) = self.tokens.last() {
            // Delete removes a pending operator and the number before it is
            // being typed again: 7 + ⌫ 5 = gives 75 (finding 37).
            self.tokens.pop();
            if let Some(Token::Number(n)) = self.tokens.pop() {
                self.entry = n;
            }
        }
        self.shown = Shown::Entry;
    }

    fn clear(&mut self) {
        if self.clears_entry() {
            self.entry = Entry::default();
            self.shown = Shown::Entry;
        } else {
            *self = Calculator::new();
        }
    }

    fn equals(&mut self) {
        let mut tokens = self.tokens.clone();
        if self.after_result() {
            // Repeated equals repeats the last operation (2+3=== gives 11).
            let (Some((op, operand)), Some(value)) = (self.repeat.clone(), self.result) else {
                return;
            };
            tokens = vec![
                Token::Number(Entry::from_value(value)),
                Token::Op(op),
                Token::Number(operand),
            ];
        } else {
            if !self.entry.is_empty() || self.entry.negative || self.entry.percent {
                tokens.push(Token::Number(self.entry.clone()));
            } else if let Some(Token::Op(_)) = tokens.last() {
                // GUESS: "2 + =" drops the dangling operator. Not recorded.
                tokens.pop();
            }
            if tokens.is_empty() {
                // "=" on its own shows 0, with no expression line.
                return;
            }
        }
        let has_operator = tokens.iter().any(|t| matches!(t, Token::Op(_)));
        let expression = render(&tokens);
        let outcome = evaluate(&tokens);
        if let (Some(Token::Op(op)), Some(Token::Number(n))) =
            (tokens.iter().rev().nth(1), tokens.last())
        {
            self.repeat = Some((*op, n.clone()));
        }
        self.tokens.clear();
        self.entry = Entry::default();
        self.last_expression = if has_operator { Some(expression) } else { None };
        match outcome {
            Ok(value) => {
                self.result = Some(value);
                self.shown = Shown::Result(format_result(value));
            }
            Err(word) => {
                // "Undefined" and "Indeterminate" come with no expression line.
                self.last_expression = None;
                self.result = None;
                self.repeat = None;
                self.shown = Shown::Result(word.to_owned());
            }
        }
    }
}

/// The expression line: operands as typed, grouped, no spaces ("(-3)+5").
fn render(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|t| match t {
            Token::Number(n) => n.shown(),
            Token::Op(op) => op.symbol().to_owned(),
        })
        .collect()
}

/// Precedence-respecting evaluation (2+3×4 = 14, finding 31), with percent
/// relative to the left operand of + and - (200+10% = 220) and a plain
/// hundredth after × and ÷ (50×10% = 5) (finding 34).
fn evaluate(tokens: &[Token]) -> Result<Decimal, &'static str> {
    // Split into terms joined by + and -, each a product of factors.
    let mut sum = Decimal::ZERO;
    let mut term: Option<Decimal> = None;
    let mut term_sign = Op::Add;
    let mut pending: Option<Op> = None;
    for token in tokens {
        match token {
            Token::Op(op) if op.binds_tightly() => pending = Some(*op),
            Token::Op(op) => {
                sum = add(sum, term_sign, term.unwrap_or(Decimal::ZERO))?;
                term = None;
                term_sign = *op;
                pending = None;
            }
            Token::Number(n) => {
                let mut value = n.number();
                if n.percent {
                    let hundredth = value / Decimal::ONE_HUNDRED;
                    value = match (term, pending) {
                        // After + or -: a percent of what came before.
                        (None, None) if !sum.is_zero() || term_sign != Op::Add => sum * hundredth,
                        _ => hundredth,
                    };
                }
                term = Some(match (term, pending) {
                    (Some(left), Some(Op::Multiply)) => left.checked_mul(value).ok_or("Error")?,
                    (Some(left), Some(Op::Divide)) => {
                        if value.is_zero() {
                            return Err(if left.is_zero() {
                                "Indeterminate"
                            } else {
                                "Undefined"
                            });
                        }
                        left.checked_div(value).ok_or("Error")?
                    }
                    _ => value,
                });
                pending = None;
            }
        }
    }
    add(sum, term_sign, term.unwrap_or(Decimal::ZERO))
}

fn add(sum: Decimal, sign: Op, term: Decimal) -> Result<Decimal, &'static str> {
    match sign {
        Op::Subtract => sum.checked_sub(term),
        _ => sum.checked_add(term),
    }
    .ok_or("Error")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ids: &[&str]) -> Calculator {
        let mut calc = Calculator::new();
        for id in ids {
            calc.press(Key::from_id(id).unwrap());
        }
        calc
    }

    #[test]
    fn clear_key_says_c_while_typing() {
        let mut calc = run(&["Decimal", "Five"]);
        assert!(calc.clears_entry());
        calc.press(Key::Clear);
        assert!(!calc.clears_entry());
        assert_eq!(calc.display(), "0");
    }

    #[test]
    fn results_round_to_eleven_places() {
        assert_eq!(
            format_result(Decimal::from(2) / Decimal::from(3)),
            "0.66666666667"
        );
        assert_eq!(format_result(Decimal::from(1_002)), "1,002");
        assert_eq!(format_result(Decimal::from(-3)), "-3");
    }
}
