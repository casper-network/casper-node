//! TOML-inspired command-line argument language.
//!
//! Supports strings, booleans, integers and arrays (lists).
//!
//! * Booleans are expressed as `true` or `false`.
//! * Any integer must fit into `i64`, otherwise will be parsed as strings.
//! * Strings can be quoted using double quotes. A backslash `\\` can be used to escape quotes
//!   inside.
//! * Unquoted strings are terminated on whitespace.
//! * Arrays are written using brackets and commas: `[1, 2, 3]`.
//!
//! ## Examples
//!
//! * `[127.0.0.1, 1.2.3.4, 6.7.8.9]` list of three strings
//! * `"hello world"` string `hello world`
//! * `["no\"de\"-1", node-2]` list of two strings (`no"de"-1` and `node-2`).

use std::{iter::Peekable, str::FromStr};

use thiserror::Error;
use toml::Value;

/// A Token to be parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Token {
    String(String),
    I64(i64),
    Boolean(bool),
    Comma,
    OpenBracket,
    CloseBracket,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub(crate) enum Error {
    #[error("unterminated string in input")]
    UnterminatedString,
    #[error("unexpected token {0:?}")]
    UnexpectedToken(Token),
    #[error("unexpected end of input")]
    UnexpectedEndOfInput,
    #[error("trailing input {0:?}...")]
    TrailingInput(Token),
}

impl Token {
    /// Constructs a token from a string.
    #[cfg(test)]
    fn string(value: &str) -> Token {
        Token::String(value.to_string())
    }
}

/// Tokenizes a stream of characters.
fn tokenize(input: &str) -> Result<Vec<Token>, Error> {
    let mut chars = input.chars();
    let mut tokens = Vec::new();

    let mut buffer = String::new();

    loop {
        let ch = chars.next();

        // Check if we need to complete a token.
        if !buffer.is_empty() {
            match ch {
                Some(' ') | Some('"') | Some('[') | Some(']') | Some(',') | None => {
                    // Try to parse as number or bool first.
                    if let Ok(value) = i64::from_str(&buffer) {
                        tokens.push(Token::I64(value));
                    } else if let Ok(value) = bool::from_str(&buffer) {
                        tokens.push(Token::Boolean(value));
                    } else {
                        tokens.push(Token::String(buffer.clone()))
                    }

                    buffer.clear();
                }
                _ => {
                    // Handled in second match below.
                }
            }
        }

        match ch {
            None => {
                // On EOF, we break.
                break;
            }
            Some(' ') => {
                // Ignore whitespace.
            }
            Some('"') => {
                // Quoted string.
                let mut escaped = false;
                let mut string = String::new();
                loop {
                    let c = chars.next();
                    match c {
                        Some(character) if escaped => {
                            string.push(character);
                            escaped = false;
                        }
                        Some('\\') => {
                            escaped = true;
                        }
                        Some('"') => {
                            break;
                        }
                        Some(character) => string.push(character),
                        None => {
                            return Err(Error::UnterminatedString);
                        }
                    }
                }
                tokens.push(Token::String(string));
            }
            Some('[') => tokens.push(Token::OpenBracket),
            Some(']') => tokens.push(Token::CloseBracket),
            Some(',') => tokens.push(Token::Comma),
            Some(character) => buffer.push(character),
        }
    }

    Ok(tokens)
}

/// Parse a stream of tokens of arglang.
fn parse_stream<I>(tokens: &mut Peekable<I>) -> Result<Value, Error>
where
    I: Iterator<Item = Token>,
{
    loop {
        match tokens.next() {
            Some(Token::String(value)) => return Ok(Value::String(value)),
            Some(Token::I64(value)) => return Ok(Value::Integer(value)),
            Some(Token::Boolean(value)) => return Ok(Value::Boolean(value)),
            Some(Token::OpenBracket) => {
                // Special case for empty list.
                if tokens.peek() == Some(&Token::CloseBracket) {
                    tokens.next();
                    return Ok(Value::Array(Vec::new()));
                }

                let mut items = Vec::new();
                loop {
                    items.push(parse_stream(tokens)?);

                    match tokens.next() {
                        Some(Token::CloseBracket) => {
                            return Ok(Value::Array(items));
                        }
                        Some(Token::Comma) => {
                            // Continue parsing next time.
                        }
                        Some(t) => {
                            return Err(Error::UnexpectedToken(t));
                        }
                        None => {
                            return Err(Error::UnexpectedEndOfInput);
                        }
                    }
                }
            }
            Some(t @ Token::CloseBracket) | Some(t @ Token::Comma) => {
                return Err(Error::UnexpectedToken(t));
            }
            None => {
                return Err(Error::UnexpectedEndOfInput);
            }
        }
    }
}

/// Parse string using arglang.
pub(crate) fn parse(input: &str) -> Result<Value, Error> {
    let mut tokens = tokenize(input)?.into_iter().peekable();
    let value = parse_stream(&mut tokens)?;

    // Check if there is trailing input.
    if let Some(trailing) = tokens.next() {
        return Err(Error::TrailingInput(trailing));
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use toml::Value;

    use super::{parse, tokenize, Error, Token};

    #[test]
    fn tokenize_single() {
        assert_eq!(tokenize("asdf").unwrap(), vec![Token::string("asdf")]);
        assert_eq!(tokenize("  ").unwrap(), vec![]);
        assert_eq!(tokenize("-123").unwrap(), vec![Token::I64(-123)]);
        assert_eq!(tokenize("123").unwrap(), vec![Token::I64(123)]);
        assert_eq!(tokenize("true").unwrap(), vec![Token::Boolean(true)]);
        assert_eq!(tokenize("false").unwrap(), vec![Token::Boolean(false)]);
        assert_eq!(tokenize("[").unwrap(), vec![Token::OpenBracket]);
        assert_eq!(tokenize("]").unwrap(), vec![Token::CloseBracket]);
        assert_eq!(tokenize(",").unwrap(), vec![Token::Comma]);

        assert_eq!(tokenize(" asdf").unwrap(), vec![Token::string("asdf")]);
        assert_eq!(tokenize("  ").unwrap(), vec![]);
        assert_eq!(tokenize(" -123").unwrap(), vec![Token::I64(-123)]);
        assert_eq!(tokenize(" 123").unwrap(), vec![Token::I64(123)]);
        assert_eq!(tokenize(" true").unwrap(), vec![Token::Boolean(true)]);
        assert_eq!(tokenize(" false").unwrap(), vec![Token::Boolean(false)]);
        assert_eq!(tokenize(" [").unwrap(), vec![Token::OpenBracket]);
        assert_eq!(tokenize(" ]").unwrap(), vec![Token::CloseBracket]);
        assert_eq!(tokenize(" ,").unwrap(), vec![Token::Comma]);

        assert_eq!(tokenize(" asdf ").unwrap(), vec![Token::string("asdf")]);
        assert_eq!(tokenize("  ").unwrap(), vec![]);
        assert_eq!(tokenize(" -123 ").unwrap(), vec![Token::I64(-123)]);
        assert_eq!(tokenize(" 123 ").unwrap(), vec![Token::I64(123)]);
        assert_eq!(tokenize(" true ").unwrap(), vec![Token::Boolean(true)]);
        assert_eq!(tokenize(" false ").unwrap(), vec![Token::Boolean(false)]);
        assert_eq!(tokenize(" [ ").unwrap(), vec![Token::OpenBracket]);
        assert_eq!(tokenize(" ] ").unwrap(), vec![Token::CloseBracket]);
        assert_eq!(tokenize(" , ").unwrap(), vec![Token::Comma]);
    }

    #[test]
    fn tokenize_strings() {
        assert_eq!(
            tokenize(" a1 b2 c3 ").unwrap(),
            vec![
                Token::string("a1"),
                Token::string("b2"),
                Token::string("c3")
            ]
        );

        assert_eq!(
            tokenize("hello \"world\"!").unwrap(),
            vec![
                Token::string("hello"),
                Token::string("world"),
                Token::string("!")
            ]
        );

        assert_eq!(
            tokenize("\"inner\\\"quote\"").unwrap(),
            vec![Token::string("inner\"quote"),]
        );

        assert_eq!(tokenize("\"asdf"), Err(Error::UnterminatedString))
    }

    #[test]
    fn tokenize_list() {
        assert_eq!(
            tokenize("[a, 1, 2]").unwrap(),
            vec![
                Token::OpenBracket,
                Token::String("a".to_owned()),
                Token::Comma,
                Token::I64(1),
                Token::Comma,
                Token::I64(2),
                Token::CloseBracket
            ]
        );
    }

    #[test]
    fn parse_simple() {
        assert_eq!(
            parse("\"hello\"").unwrap(),
            Value::String("hello".to_owned())
        );
        assert_eq!(
            parse("\"127.0.0.1\"").unwrap(),
            Value::String("127.0.0.1".to_owned())
        );
        assert_eq!(
            parse("127.0.0.1").unwrap(),
            Value::String("127.0.0.1".to_owned())
        );

        assert_eq!(parse("true").unwrap(), Value::Boolean(true));
        assert_eq!(parse("false").unwrap(), Value::Boolean(false));

        assert_eq!(parse("123").unwrap(), Value::Integer(123));
        assert_eq!(parse("-123").unwrap(), Value::Integer(-123));

        assert_eq!(
            parse("123456789012345678901234567890").unwrap(),
            Value::String("123456789012345678901234567890".to_string())
        );
    }

    #[test]
    fn parse_arrays() {
        assert_eq!(parse(" [ ] ").unwrap(), Value::Array(Vec::new()));
        assert_eq!(parse("[]").unwrap(), Value::Array(Vec::new()));

        assert_eq!(
            parse("[a, 1, 2]").unwrap(),
            Value::Array(vec![
                Value::String("a".to_string()),
                Value::Integer(1),
                Value::Integer(2),
            ])
        );

        assert_eq!(
            parse("[a, [1, 2], 3]").unwrap(),
            Value::Array(vec![
                Value::String("a".to_string()),
                Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
                Value::Integer(3),
            ])
        );
    }

    #[test]
    fn doc_examples() {
        assert_eq!(
            parse("[127.0.0.1, 1.2.3.4, 6.7.8.9]").unwrap(),
            Value::Array(vec![
                Value::String("127.0.0.1".to_owned()),
                Value::String("1.2.3.4".to_owned()),
                Value::String("6.7.8.9".to_owned())
            ])
        );

        assert_eq!(
            parse("\"hello world\"").unwrap(),
            Value::String("hello world".to_owned())
        );

        assert_eq!(
            parse("[\"no\\\"de\\\"-1\", node-2]").unwrap(),
            Value::Array(vec![
                Value::String("no\"de\"-1".to_owned()),
                Value::String("node-2".to_owned()),
            ])
        );
    }
}
