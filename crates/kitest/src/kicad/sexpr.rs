//! A reader for the S-expressions KiCad writes its schematic files in.

/// One node of an S-expression.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Sexp {
    List(Vec<Sexp>),
    Atom(String),
    Text(String),
}

impl Sexp {
    /// The leading atom of a list, which names it in KiCad files.
    pub(crate) fn head(&self) -> Option<&str> {
        match self.items().first() {
            Some(Sexp::Atom(atom)) => Some(atom),
            _ => None,
        }
    }

    /// The child lists whose head is `name`.
    pub(crate) fn children<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.items()
            .iter()
            .filter(move |child| child.head() == Some(name))
    }

    /// The first child list whose head is `name`.
    pub(crate) fn child(&self, name: &str) -> Option<&Sexp> {
        self.items().iter().find(|child| child.head() == Some(name))
    }

    /// The item at `index` as text, whether quoted or bare.
    pub(crate) fn arg(&self, index: usize) -> Option<&str> {
        match self.items().get(index)? {
            Sexp::Atom(text) | Sexp::Text(text) => Some(text),
            Sexp::List(_) => None,
        }
    }

    fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List(items) => items,
            Sexp::Atom(_) | Sexp::Text(_) => &[],
        }
    }
}

/// Where and why an S-expression failed to parse.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message} at byte {offset}")]
pub struct SexpError {
    pub offset: usize,
    pub message: &'static str,
}

/// Parse a document holding exactly one S-expression.
pub(crate) fn parse(input: &str) -> Result<Sexp, SexpError> {
    let mut stack: Vec<Vec<Sexp>> = Vec::new();
    let mut done: Option<Sexp> = None;
    let mut chars = input.char_indices().peekable();

    while let Some((offset, c)) = chars.next() {
        let item = match c {
            c if c.is_whitespace() => continue,
            '(' => {
                stack.push(Vec::new());
                continue;
            }
            ')' => {
                let items = stack.pop().ok_or(SexpError {
                    offset,
                    message: "unmatched ')'",
                })?;
                Sexp::List(items)
            }
            '"' => Sexp::Text(read_text(&mut chars, offset)?),
            _ => {
                let mut atom = String::from(c);
                while let Some(&(_, next)) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '(' | ')' | '"') {
                        break;
                    }
                    atom.push(next);
                    chars.next();
                }
                Sexp::Atom(atom)
            }
        };

        match stack.last_mut() {
            Some(list) => list.push(item),
            None if done.is_none() => done = Some(item),
            None => {
                return Err(SexpError {
                    offset,
                    message: "more than one top-level expression",
                });
            }
        }
    }

    if !stack.is_empty() {
        return Err(SexpError {
            offset: input.len(),
            message: "unclosed '('",
        });
    }
    done.ok_or(SexpError {
        offset: 0,
        message: "empty document",
    })
}

/// Read a quoted string whose opening quote sat at `start`.
fn read_text(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Result<String, SexpError> {
    let mut text = String::new();
    while let Some((_, c)) = chars.next() {
        match c {
            '"' => return Ok(text),
            '\\' => match chars.next() {
                Some((_, 'n')) => text.push('\n'),
                Some((_, 't')) => text.push('\t'),
                Some((_, escaped)) => text.push(escaped),
                None => break,
            },
            _ => text.push(c),
        }
    }
    Err(SexpError {
        offset: start,
        message: "unterminated string",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(text: &str) -> Sexp {
        Sexp::Atom(text.to_owned())
    }

    fn text(text: &str) -> Sexp {
        Sexp::Text(text.to_owned())
    }

    #[test]
    fn reads_nested_lists_of_atoms_and_text() {
        let parsed = parse(r#"(symbol (lib_id "power:VCC") (at 1.5 -2 0))"#)
            .expect("parses");
        assert_eq!(
            parsed,
            Sexp::List(vec![
                atom("symbol"),
                Sexp::List(vec![atom("lib_id"), text("power:VCC")]),
                Sexp::List(vec![
                    atom("at"),
                    atom("1.5"),
                    atom("-2"),
                    atom("0")
                ]),
            ])
        );
    }

    #[test]
    fn unescapes_quotes_backslashes_and_newlines() {
        let parsed = parse(r#"("say \"hi\"\\\n")"#).expect("parses");
        assert_eq!(parsed.arg(0), Some("say \"hi\"\\\n"));
    }

    #[test]
    fn keeps_parentheses_inside_text() {
        let parsed = parse(r#"(net "Net-(Q1-B)")"#).expect("parses");
        assert_eq!(parsed.arg(1), Some("Net-(Q1-B)"));
    }

    #[test]
    fn finds_children_by_head() {
        let parsed = parse(
            r#"(symbol (property "Reference" "R1") (property "Value" "10k"))"#,
        )
        .expect("parses");
        let values: Vec<_> = parsed
            .children("property")
            .filter_map(|p| p.arg(2))
            .collect();
        assert_eq!(values, ["R1", "10k"]);
        assert!(parsed.child("lib_id").is_none());
    }

    #[test]
    fn rejects_unbalanced_parentheses() {
        assert_eq!(parse("(a (b)").unwrap_err().message, "unclosed '('");
        assert_eq!(parse("(a))").unwrap_err().message, "unmatched ')'");
    }

    #[test]
    fn rejects_an_unterminated_string() {
        let error = parse(r#"(a "open)"#).unwrap_err();
        assert_eq!(error.message, "unterminated string");
        assert_eq!(error.offset, 3);
    }

    #[test]
    fn rejects_empty_and_multiple_documents() {
        assert_eq!(parse("  ").unwrap_err().message, "empty document");
        assert_eq!(
            parse("(a) (b)").unwrap_err().message,
            "more than one top-level expression"
        );
    }
}
