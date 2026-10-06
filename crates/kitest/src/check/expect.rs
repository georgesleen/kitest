//! Parsing a probe's `Expect` field.
//!
//! The field is a single call in the subset of the Python API a probe can
//! state, such as `oscillates(near=10.4e6, within=percent(5))`. It is parsed,
//! never evaluated.

use crate::Tolerance;

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
const CHECKS: &str = "dc(near=<volts>, within=<tolerance>) or \
                      oscillates(near=<hertz>, within=<tolerance>)";

/// The tolerances a check accepts, for error messages.
const TOLERANCES: &str = "percent(<p>) or abs(<value>)";

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
    Number(f64),
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
                self.number().map(Value::Number)
            }
            _ => Err(self.unexpected("a number or a call")),
        }
    }

    fn number(&mut self) -> Result<f64, ExpectError> {
        let start = self.at;
        while let Some(c) = self.peek() {
            let exponent_sign = matches!(c, '-' | '+')
                && self.text[start..self.at].ends_with(['e', 'E']);
            let leading_sign = matches!(c, '-' | '+') && self.at == start;
            if c.is_ascii_digit()
                || matches!(c, '.' | 'e' | 'E' | '_')
                || exponent_sign
                || leading_sign
            {
                self.at += c.len_utf8();
            } else {
                break;
            }
        }
        let token = &self.text[start..self.at];
        token
            .replace('_', "")
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                ExpectError::new(format!(
                    "{token:?} is not a number, in {}",
                    self.text
                ))
            })
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

fn expectation(call: &Call<'_>) -> Result<Expectation, ExpectError> {
    let build = match call.name {
        "dc" => |near, within| Expectation::Dc { near, within },
        "oscillates" => |near, within| Expectation::Oscillates { near, within },
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
            "near" => near = Some(number(name, keyword, value)?),
            "within" => within = Some(tolerance(name, value)?),
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
        ExpectError::new(format!(
            "{name} needs within=, such as within=percent(5)"
        ))
    })?;
    Ok(build(near, within))
}

fn number(
    name: &str,
    keyword: &str,
    value: &Value<'_>,
) -> Result<f64, ExpectError> {
    match value {
        Value::Number(number) => Ok(*number),
        Value::Call(call) => Err(ExpectError::new(format!(
            "{name}'s {keyword}= is a number, not {}(...)",
            call.name
        ))),
    }
}

fn tolerance(name: &str, value: &Value<'_>) -> Result<Tolerance, ExpectError> {
    let Value::Call(call) = value else {
        return Err(ExpectError::new(format!(
            "{name}'s within= is {TOLERANCES}, not a bare number"
        )));
    };
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
        ([Value::Number(amount)], true) if *amount > 0.0 => Ok(make(*amount)),
        ([Value::Number(_)], true) => Err(ExpectError::new(format!(
            "{}(...) must be positive",
            call.name
        ))),
        _ => Err(ExpectError::new(format!(
            "{} takes one number, such as {}(5)",
            call.name, call.name
        ))),
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
    fn unknown_check_lists_the_ones_that_exist() {
        let text = message("osc(near=1, within=percent(1))");
        assert!(text.contains("\"osc\""), "{text}");
        assert!(text.contains("oscillates(near=<hertz>"), "{text}");
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
        assert!(message("dc(near=1, within=5)").contains("not a bare number"));
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
                .contains("\"1e\" is not a number")
        );
        assert!(message("dc(near=1, within=abs(1)").contains("found the end"));
    }
}
