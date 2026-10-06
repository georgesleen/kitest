//! Parsing a probe's `Expect` field.
//!
//! The field is a single call in the subset of the Python API a probe can
//! state, such as `oscillates(near=10.4MHz, within=5%)`. Values read like
//! KiCad part values, with SI multipliers and optional units. It is parsed,
//! never evaluated.

use crate::Tolerance;
use crate::kicad::{Unit, parse_value};

/// What a probe claims about its net.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Expectation {
    /// The operating-point voltage is within `within` of `near` volts.
    Dc { near: f64, within: Tolerance },
    /// The net oscillates at a frequency within `within` of `near` hertz.
    Oscillates { near: f64, within: Tolerance },
}

/// Why an `Expect` field could not be read.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct ExpectError {
    message: String,
}

impl ExpectError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// The checks a probe can state, with their keywords, for error messages.
const CHECKS: &str = "dc(near=3.3V, within=5%) or \
                      oscillates(near=10.4MHz, within=2%)";

/// The tolerances a check accepts, for error messages.
const TOLERANCES: &str = "a percentage such as 5%, an amount such as 100mV, \
                          percent(5), or abs(100mV)";

/// Parse `text`, which is not empty.
pub fn parse(text: &str) -> Result<Expectation, ExpectError> {
    let mut parser = Parser { text, at: 0 };
    let call = parser.call()?;
    parser.skip_space();
    if parser.at < text.len() {
        return Err(parser.unexpected("nothing after the closing \")\""));
    }
    expectation(&call)
}

/// A parsed call: a name, then positional and keyword arguments.
struct Call<'a> {
    name: &'a str,
    positional: Vec<Value<'a>>,
    keywords: Vec<(&'a str, Value<'a>)>,
}

enum Value<'a> {
    /// A number as written, with any SI multiplier, unit, or `%`.
    Quantity(&'a str),
    Call(Call<'a>),
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn call(&mut self) -> Result<Call<'a>, ExpectError> {
        let name = self.name()?;
        self.expect('(')?;
        let mut call = Call {
            name,
            positional: Vec::new(),
            keywords: Vec::new(),
        };
        loop {
            self.skip_space();
            if self.eat(')') {
                return Ok(call);
            }
            let start = self.at;
            let value = match self.name_then_equals() {
                Some(keyword) => {
                    if call.keywords.iter().any(|(seen, _)| *seen == keyword) {
                        return Err(ExpectError::new(format!(
                            "{keyword}= is given twice in {}",
                            self.text
                        )));
                    }
                    let value = self.value()?;
                    call.keywords.push((keyword, value));
                    None
                }
                None => Some(self.value()?),
            };
            if let Some(value) = value {
                if !call.keywords.is_empty() {
                    self.at = start;
                    return Err(self.unexpected("a keyword argument"));
                }
                call.positional.push(value);
            }
            self.skip_space();
            if !self.eat(',') {
                self.expect(')')?;
                return Ok(call);
            }
        }
    }

    fn value(&mut self) -> Result<Value<'a>, ExpectError> {
        self.skip_space();
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => Ok(Value::Call(self.call()?)),
            Some(c) if c.is_ascii_digit() || matches!(c, '-' | '+' | '.') => {
                Ok(Value::Quantity(self.quantity()))
            }
            _ => Err(self.unexpected("a number or a call")),
        }
    }

    /// A number with whatever multiplier, unit, or `%` follows it, which may
    /// be separated by one space as in `10.1 MHz`.
    fn quantity(&mut self) -> &'a str {
        let start = self.at;
        self.take_while(is_quantity_char);
        let joined = self.at;
        self.skip_space();
        let suffix = self.at;
        self.take_while(is_quantity_char);
        let follows = self.peek();
        let is_unit = self.at > suffix
            && self.text[suffix..self.at]
                .starts_with(|c: char| !c.is_ascii_digit())
            && !matches!(follows, Some('=' | '('));
        if !is_unit {
            self.at = joined;
        }
        &self.text[start..self.at]
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) {
        while let Some(c) = self.peek().filter(|c| keep(*c)) {
            self.at += c.len_utf8();
        }
    }

    /// A name followed by `=`, consumed; otherwise nothing is consumed.
    fn name_then_equals(&mut self) -> Option<&'a str> {
        let start = self.at;
        if let Ok(name) = self.name() {
            self.skip_space();
            if self.eat('=') {
                return Some(name);
            }
        }
        self.at = start;
        None
    }

    fn name(&mut self) -> Result<&'a str, ExpectError> {
        self.skip_space();
        let start = self.at;
        while let Some(c) = self.peek() {
            let first = self.at == start;
            if c.is_ascii_alphabetic()
                || c == '_'
                || (!first && c.is_ascii_digit())
            {
                self.at += 1;
            } else {
                break;
            }
        }
        if self.at == start {
            return Err(self.unexpected("a name"));
        }
        Ok(&self.text[start..self.at])
    }

    fn expect(&mut self, wanted: char) -> Result<(), ExpectError> {
        self.skip_space();
        if self.eat(wanted) {
            Ok(())
        } else {
            Err(self.unexpected(&format!("{wanted:?}")))
        }
    }

    fn eat(&mut self, wanted: char) -> bool {
        if self.peek() == Some(wanted) {
            self.at += wanted.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.at..].chars().next()
    }

    fn skip_space(&mut self) {
        while let Some(c) = self.peek().filter(|c| c.is_whitespace()) {
            self.at += c.len_utf8();
        }
    }

    fn unexpected(&self, wanted: &str) -> ExpectError {
        let found = match self.peek() {
            Some(c) => format!("{c:?}"),
            None => "the end".to_owned(),
        };
        ExpectError::new(format!(
            "expected {wanted} at column {}, found {found}, in {}",
            self.text[..self.at].chars().count() + 1,
            self.text
        ))
    }
}

/// Characters a quantity such as `-2.5e-3`, `10.1MHz` or `5%` is made of.
fn is_quantity_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, ',' | '(' | ')' | '=')
}

fn expectation(call: &Call<'_>) -> Result<Expectation, ExpectError> {
    let (unit, build): (Unit, fn(f64, Tolerance) -> Expectation) = match call
        .name
    {
        "dc" => (Unit::Volt, |near, within| Expectation::Dc { near, within }),
        "oscillates" => (Unit::Hertz, |near, within| Expectation::Oscillates {
            near,
            within,
        }),
        other => {
            return Err(ExpectError::new(format!(
                "unknown check {other:?}; a probe can state {CHECKS}"
            )));
        }
    };
    let name = call.name;
    if !call.positional.is_empty() {
        return Err(ExpectError::new(format!(
            "{name} takes keyword arguments only, such as {name}(near=..., within=...)"
        )));
    }
    let mut near = None;
    let mut within = None;
    for (keyword, value) in &call.keywords {
        match *keyword {
            "near" => near = Some(number(name, keyword, value, unit)?),
            "within" => within = Some(tolerance(name, value, unit)?),
            other => {
                return Err(ExpectError::new(format!(
                    "{name} takes near= and within=, not {other}="
                )));
            }
        }
    }
    let near = near.ok_or_else(|| {
        ExpectError::new(format!("{name} needs near=, the expected value"))
    })?;
    let within = within.ok_or_else(|| {
        ExpectError::new(format!("{name} needs within=, such as within=5%"))
    })?;
    Ok(build(near, within))
}

fn number(
    name: &str,
    keyword: &str,
    value: &Value<'_>,
    unit: Unit,
) -> Result<f64, ExpectError> {
    match value {
        Value::Quantity(text) => amount(text, unit),
        Value::Call(call) => Err(ExpectError::new(format!(
            "{name}'s {keyword}= is a {}, not {}(...)",
            unit.describe(),
            call.name
        ))),
    }
}

/// `text` as a value in `unit`, signed, with any SI multiplier.
fn amount(text: &str, unit: Unit) -> Result<f64, ExpectError> {
    let (sign, magnitude) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    parse_value(&magnitude.replace('_', ""), unit)
        .filter(|value| value.is_finite())
        .map(|value| sign * value)
        .ok_or_else(|| {
            ExpectError::new(format!("{text:?} is not a {}", unit.describe()))
        })
}

fn tolerance(
    name: &str,
    value: &Value<'_>,
    unit: Unit,
) -> Result<Tolerance, ExpectError> {
    let tolerance = match value {
        Value::Quantity(text) => match text.strip_suffix('%') {
            Some(percent) => Tolerance::percent(amount(percent.trim(), unit)?),
            None if text.replace('_', "").parse::<f64>().is_ok() => {
                return Err(ExpectError::new(format!(
                    "{name}'s within={text} could be a percentage or an amount; \
                     write {text}% for a percentage, or give a {}",
                    unit.describe()
                )));
            }
            None => Tolerance::abs(amount(text, unit)?),
        },
        Value::Call(call) => {
            let make = match call.name {
                "percent" => Tolerance::percent,
                "abs" => Tolerance::abs,
                other => {
                    return Err(ExpectError::new(format!(
                        "unknown tolerance {other:?}; within= is {TOLERANCES}"
                    )));
                }
            };
            match (call.positional.as_slice(), call.keywords.is_empty()) {
                ([Value::Quantity(text)], true) => make(amount(text, unit)?),
                _ => {
                    return Err(ExpectError::new(format!(
                        "{} takes one number, such as {}(5)",
                        call.name, call.name
                    )));
                }
            }
        }
    };
    let size = match tolerance {
        Tolerance::Abs(size) | Tolerance::Percent(size) => size,
    };
    if size > 0.0 {
        Ok(tolerance)
    } else {
        Err(ExpectError::new(format!(
            "{name}'s within= must be positive"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> String {
        parse(text).expect_err("rejected").to_string()
    }

    #[test]
    fn reads_both_checks_with_either_tolerance() {
        assert_eq!(
            parse("oscillates(near=10.4e6, within=percent(5))"),
            Ok(Expectation::Oscillates {
                near: 10.4e6,
                within: Tolerance::Percent(5.0)
            })
        );
        assert_eq!(
            parse(" dc( within = abs(0.1) , near=-2.5 , ) "),
            Ok(Expectation::Dc {
                near: -2.5,
                within: Tolerance::Abs(0.1)
            })
        );
    }

    #[test]
    fn reads_values_the_way_kicad_writes_them() {
        assert_eq!(
            parse("oscillates(near=10.4MHz, within=2%)"),
            Ok(Expectation::Oscillates {
                near: 10.4e6,
                within: Tolerance::Percent(2.0)
            })
        );
        assert_eq!(
            parse("oscillates(near=32.768 kHz, within=50Hz)"),
            Ok(Expectation::Oscillates {
                near: 32.768e3,
                within: Tolerance::Abs(50.0)
            })
        );
        assert_eq!(
            parse("dc(near=3V3, within=100mV)"),
            Ok(Expectation::Dc {
                near: 3.3,
                within: Tolerance::Abs(0.1)
            })
        );
        assert!(message("dc(near=10MHz, within=1%)").contains("not a voltage"));
    }

    #[test]
    fn unknown_check_lists_the_ones_that_exist() {
        let text = message("osc(near=1, within=percent(1))");
        assert!(text.contains("\"osc\""), "{text}");
        assert!(text.contains("oscillates(near=10.4MHz"), "{text}");
    }

    #[test]
    fn missing_or_unknown_keywords_are_named() {
        assert!(message("dc(near=1)").contains("needs within="));
        assert!(message("dc(within=abs(1))").contains("needs near="));
        assert!(
            message("dc(near=1, within=abs(1), freq=2)").contains("not freq=")
        );
        assert!(message("dc(near=1, near=2, within=abs(1))").contains("twice"));
        assert!(
            message("dc(1, within=abs(1))").contains("keyword arguments only")
        );
    }

    #[test]
    fn tolerance_must_be_a_positive_percent_or_abs() {
        assert!(
            message("dc(near=1, within=5)")
                .contains("write 5% for a percentage, or give a voltage")
        );
        assert!(
            message("dc(near=1, within=ppm(5))").contains("unknown tolerance")
        );
        assert!(message("dc(near=1, within=percent(0))").contains("positive"));
        assert!(
            message("dc(near=1, within=percent(1, 2))").contains("one number")
        );
    }

    #[test]
    fn syntax_errors_say_where() {
        assert!(message("dc(near=1 within=abs(1))").contains("column 11"));
        assert!(
            message("dc(near=1, within=abs(1)) extra")
                .contains("nothing after")
        );
        assert!(
            message("dc(near=1e, within=abs(1))")
                .contains("\"1e\" is not a voltage")
        );
        assert!(message("dc(near=1, within=abs(1)").contains("found the end"));
    }
}
