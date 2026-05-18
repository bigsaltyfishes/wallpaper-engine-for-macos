//! Property visibility condition DSL used by Wallpaper Engine.
//!
//! Grammar:
//! ```text
//! condition := and_expr ( "||" and_expr )*
//! and_expr  := unary ( "&&" unary )*
//! unary     := "!"? atom
//! atom      := ident "." "value" ( op literal )?
//! op        := "==" | "!=" | ">=" | "<=" | ">" | "<"
//! literal   := Number | String | Bool
//! ```

use super::property::PropertyValue;
use crate::{BridgeError, BridgeErrorKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Or(Box<Condition>, Box<Condition>),
    And(Box<Condition>, Box<Condition>),
    Not(Box<Condition>),
    Truthy(String),
    Compare(String, CmpOp, Literal),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CmpOp {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
    Bool(bool),
}

impl Condition {
    /// # Errors
    ///
    /// Returns an error when the condition string does not match the supported
    /// DSL grammar.
    pub fn parse(input: &str) -> Result<Self, BridgeError> {
        let tokens = Token::tokenize(input)?;
        let mut parser = Parser { tokens, pos: 0 };
        let condition = parser.parse_or()?;

        if parser.pos != parser.tokens.len() {
            let pos = parser.tokens[parser.pos].pos;
            return Err(condition_error(pos, "trailing tokens"));
        }

        Ok(condition)
    }

    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn eval(&self, lookup: &impl Fn(&str) -> Option<PropertyValue>) -> bool {
        match self {
            Condition::Or(left, right) => left.eval(lookup) || right.eval(lookup),
            Condition::And(left, right) => left.eval(lookup) && right.eval(lookup),
            Condition::Not(inner) => !inner.eval(lookup),
            Condition::Truthy(id) => match lookup(id) {
                Some(PropertyValue::Bool(value)) => value,
                Some(PropertyValue::Number(value)) => value != 0.0,
                Some(PropertyValue::String(value)) => {
                    let value = value.trim();
                    !value.is_empty()
                        && !matches!(value.to_ascii_lowercase().as_str(), "0" | "false")
                }
                Some(PropertyValue::Null) => false,
                None | Some(PropertyValue::ColorRgb(..)) => true,
            },
            Condition::Compare(id, op, literal) => {
                let Some(value) = lookup(id) else {
                    return true;
                };

                match (value, literal) {
                    (PropertyValue::Number(left), Literal::Number(right)) => {
                        compare_number(left, *right, *op)
                    }
                    (PropertyValue::Bool(left), Literal::Number(right)) => {
                        let left = if left { 1.0 } else { 0.0 };
                        compare_number(left, *right, *op)
                    }
                    (PropertyValue::Bool(left), Literal::Bool(right)) => match op {
                        CmpOp::Eq => left == *right,
                        CmpOp::Ne => left != *right,
                        _ => {
                            let left = if left { 1.0 } else { 0.0 };
                            let right = if *right { 1.0 } else { 0.0 };
                            compare_number(left, right, *op)
                        }
                    },
                    (value, literal) => {
                        let value = match &value {
                            PropertyValue::Bool(value) => value.to_string(),
                            PropertyValue::Number(value) => {
                                if value.fract() == 0.0 {
                                    format!("{value:.0}")
                                } else {
                                    value.to_string()
                                }
                            }
                            PropertyValue::String(value) => value.clone(),
                            PropertyValue::ColorRgb(r, g, b) => format!("{r} {g} {b}"),
                            PropertyValue::Null => String::new(),
                        };
                        let literal = match literal {
                            Literal::Number(value) => {
                                if value.fract() == 0.0 {
                                    format!("{value:.0}")
                                } else {
                                    value.to_string()
                                }
                            }
                            Literal::String(value) => value.clone(),
                            Literal::Bool(value) => value.to_string(),
                        };

                        match op {
                            CmpOp::Eq => value == literal,
                            CmpOp::Ne => value != literal,
                            CmpOp::Ge => {
                                value.trim().parse::<f64>().unwrap_or(0.0)
                                    >= literal.trim().parse::<f64>().unwrap_or(0.0)
                            }
                            CmpOp::Le => {
                                value.trim().parse::<f64>().unwrap_or(0.0)
                                    <= literal.trim().parse::<f64>().unwrap_or(0.0)
                            }
                            CmpOp::Gt => {
                                value.trim().parse::<f64>().unwrap_or(0.0)
                                    > literal.trim().parse::<f64>().unwrap_or(0.0)
                            }
                            CmpOp::Lt => {
                                value.trim().parse::<f64>().unwrap_or(0.0)
                                    < literal.trim().parse::<f64>().unwrap_or(0.0)
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    kind: TokenKind,
    pos: usize,
}

impl Token {
    #[allow(clippy::single_call_fn, clippy::too_many_lines)] // Tokenization is a distinct grammar unit for this DSL parser.
    pub fn tokenize(input: &str) -> Result<Vec<Self>, BridgeError> {
        let bytes = input.as_bytes();
        let mut tokens = Vec::new();
        let mut pos = 0;

        while let Some(&byte) = bytes.get(pos) {
            let ch = byte as char;
            if ch.is_ascii_whitespace() {
                pos += 1;
                continue;
            }

            match ch {
                '.' => {
                    tokens.push(Token {
                        kind: TokenKind::Dot,
                        pos,
                    });
                    pos += 1;
                }
                '!' => {
                    if bytes.get(pos + 1) == Some(&b'=') {
                        tokens.push(Token {
                            kind: TokenKind::Ne,
                            pos,
                        });
                        pos += 2;
                    } else {
                        tokens.push(Token {
                            kind: TokenKind::Not,
                            pos,
                        });
                        pos += 1;
                    }
                }
                '=' => {
                    if bytes.get(pos + 1) == Some(&b'=') {
                        tokens.push(Token {
                            kind: TokenKind::Eq,
                            pos,
                        });
                        pos += 2;
                    } else {
                        return Err(condition_error(pos, "bare `=`"));
                    }
                }
                '>' => {
                    if bytes.get(pos + 1) == Some(&b'=') {
                        tokens.push(Token {
                            kind: TokenKind::Ge,
                            pos,
                        });
                        pos += 2;
                    } else {
                        tokens.push(Token {
                            kind: TokenKind::Gt,
                            pos,
                        });
                        pos += 1;
                    }
                }
                '<' => {
                    if bytes.get(pos + 1) == Some(&b'=') {
                        tokens.push(Token {
                            kind: TokenKind::Le,
                            pos,
                        });
                        pos += 2;
                    } else {
                        tokens.push(Token {
                            kind: TokenKind::Lt,
                            pos,
                        });
                        pos += 1;
                    }
                }
                '&' => {
                    if bytes.get(pos + 1) == Some(&b'&') {
                        tokens.push(Token {
                            kind: TokenKind::And,
                            pos,
                        });
                        pos += 2;
                    } else {
                        return Err(condition_error(pos, "bare `&`"));
                    }
                }
                '|' => {
                    if bytes.get(pos + 1) == Some(&b'|') {
                        tokens.push(Token {
                            kind: TokenKind::Or,
                            pos,
                        });
                        pos += 2;
                    } else {
                        return Err(condition_error(pos, "bare `|`"));
                    }
                }
                '"' => {
                    let start = pos + 1;
                    let mut end = start;
                    while end < bytes.len() && bytes[end] != b'"' {
                        end += 1;
                    }

                    if end >= bytes.len() {
                        return Err(condition_error(start, "unclosed string"));
                    }

                    tokens.push(Token {
                        kind: TokenKind::String(input[start..end].to_string()),
                        pos,
                    });
                    pos = end + 1;
                }
                '-' | '0'..='9' => {
                    let start = pos;
                    let (value, next_pos) = {
                        let mut number_pos = pos;

                        if bytes[number_pos] == b'-' {
                            number_pos += 1;
                        }

                        let digits_start = number_pos;
                        while matches!(bytes.get(number_pos), Some(b'0'..=b'9')) {
                            number_pos += 1;
                        }

                        let mut saw_fraction = false;
                        if bytes.get(number_pos) == Some(&b'.') {
                            saw_fraction = true;
                            number_pos += 1;
                            let fraction_start = number_pos;
                            while matches!(bytes.get(number_pos), Some(b'0'..=b'9')) {
                                number_pos += 1;
                            }

                            if number_pos == fraction_start {
                                return Err(condition_error(start, "bad number"));
                            }
                        }

                        if number_pos == digits_start {
                            return Err(condition_error(start, "bad number"));
                        }

                        if matches!(
                            bytes.get(number_pos),
                            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'.')
                        ) {
                            return Err(condition_error(start, "bad number"));
                        }

                        let text = &input[start..number_pos];
                        let value = text
                            .parse::<f64>()
                            .map_err(|_| condition_error(start, "bad number"))?;

                        if saw_fraction && text.ends_with('.') {
                            return Err(condition_error(start, "bad number"));
                        }

                        (value, number_pos)
                    };
                    pos = next_pos;
                    tokens.push(Token {
                        kind: TokenKind::Number(value),
                        pos: start,
                    });
                }
                'a'..='z' | 'A'..='Z' | '_' => {
                    let start = pos;
                    pos += 1;
                    while let Some(&next) = bytes.get(pos) {
                        let next = next as char;
                        if next.is_ascii_alphanumeric() || next == '_' {
                            pos += 1;
                        } else {
                            break;
                        }
                    }

                    tokens.push(Token {
                        kind: TokenKind::Ident(input[start..pos].to_string()),
                        pos: start,
                    });
                }
                _ => {
                    return Err(condition_error(pos, format!("unexpected `{ch}`")));
                }
            }
        }

        Ok(tokens)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(f64),
    String(String),
    Dot,
    Not,
    And,
    Or,
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

fn condition_error(pos: usize, reason: impl Into<String>) -> BridgeError {
    BridgeError::Error {
        kind: BridgeErrorKind::Project,
        message: format!("condition parse error at {pos}: {}", reason.into()),
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn parse_or(&mut self) -> Result<Condition, BridgeError> {
        let mut condition = self.parse_and()?;
        while self.matches(&TokenKind::Or) {
            let right = self.parse_and()?;
            condition = Condition::Or(Box::new(condition), Box::new(right));
        }
        Ok(condition)
    }

    fn parse_and(&mut self) -> Result<Condition, BridgeError> {
        let mut condition = self.parse_unary()?;
        while self.matches(&TokenKind::And) {
            let right = self.parse_unary()?;
            condition = Condition::And(Box::new(condition), Box::new(right));
        }
        Ok(condition)
    }

    fn parse_unary(&mut self) -> Result<Condition, BridgeError> {
        if self.matches(&TokenKind::Not) {
            let inner = self.parse_unary_atom()?;
            return Ok(Condition::Not(Box::new(inner)));
        }

        self.parse_unary_atom()
    }

    fn parse_unary_atom(&mut self) -> Result<Condition, BridgeError> {
        let ident = match self.bump() {
            Some(Token {
                kind: TokenKind::Ident(value),
                ..
            }) => value,
            Some(token) => {
                return Err(condition_error(token.pos, "expected identifier"));
            }
            None => return Err(condition_error(self.eof_pos(), "expected identifier")),
        };

        self.expect(&TokenKind::Dot, "`.`")?;
        self.expect_value_suffix()?;

        let Some(op) = self.peek_cmp() else {
            return Ok(Condition::Truthy(ident));
        };

        self.bump();
        let literal = self.parse_literal()?;
        Ok(Condition::Compare(ident, op, literal))
    }

    fn parse_literal(&mut self) -> Result<Literal, BridgeError> {
        match self.bump() {
            Some(Token {
                kind: TokenKind::Number(value),
                ..
            }) => Ok(Literal::Number(value)),
            Some(Token {
                kind: TokenKind::String(value),
                ..
            }) => Ok(Literal::String(value)),
            Some(Token {
                kind: TokenKind::Ident(value),
                ..
            }) if value == "true" => Ok(Literal::Bool(true)),
            Some(Token {
                kind: TokenKind::Ident(value),
                ..
            }) if value == "false" => Ok(Literal::Bool(false)),
            Some(token) => Err(condition_error(token.pos, "expected literal")),
            None => Err(condition_error(self.eof_pos(), "expected literal")),
        }
    }

    fn expect_value_suffix(&mut self) -> Result<(), BridgeError> {
        match self.bump() {
            Some(Token {
                kind: TokenKind::Ident(value),
                ..
            }) if value == "value" => Ok(()),
            Some(token) => Err(condition_error(token.pos, "expected `value`")),
            None => Err(condition_error(self.eof_pos(), "expected `value`")),
        }
    }

    fn matches(&mut self, kind: &TokenKind) -> bool {
        if self.peek_kind_is(kind) {
            self.pos += 1;
            return true;
        }

        false
    }

    fn expect(&mut self, kind: &TokenKind, expected: &str) -> Result<(), BridgeError> {
        if self.matches(kind) {
            return Ok(());
        }

        let pos = self.peek_pos().unwrap_or_else(|| self.eof_pos());
        Err(condition_error(pos, format!("expected {expected}")))
    }

    fn peek_cmp(&self) -> Option<CmpOp> {
        match self.tokens.get(self.pos).map(|token| &token.kind) {
            Some(TokenKind::Eq) => Some(CmpOp::Eq),
            Some(TokenKind::Ne) => Some(CmpOp::Ne),
            Some(TokenKind::Ge) => Some(CmpOp::Ge),
            Some(TokenKind::Le) => Some(CmpOp::Le),
            Some(TokenKind::Gt) => Some(CmpOp::Gt),
            Some(TokenKind::Lt) => Some(CmpOp::Lt),
            _ => None,
        }
    }

    fn peek_kind_is(&self, expected: &TokenKind) -> bool {
        matches!(
            (self.tokens.get(self.pos).map(|token| &token.kind), expected),
            (Some(TokenKind::Dot), TokenKind::Dot)
                | (Some(TokenKind::Not), TokenKind::Not)
                | (Some(TokenKind::And), TokenKind::And)
                | (Some(TokenKind::Or), TokenKind::Or)
                | (Some(TokenKind::Eq), TokenKind::Eq)
                | (Some(TokenKind::Ne), TokenKind::Ne)
                | (Some(TokenKind::Ge), TokenKind::Ge)
                | (Some(TokenKind::Le), TokenKind::Le)
                | (Some(TokenKind::Gt), TokenKind::Gt)
                | (Some(TokenKind::Lt), TokenKind::Lt)
        )
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn peek_pos(&self) -> Option<usize> {
        self.tokens.get(self.pos).map(|token| token.pos)
    }

    fn eof_pos(&self) -> usize {
        self.tokens.last().map_or(0, |token| token.pos + 1)
    }
}

fn compare_number(left: f64, right: f64, op: CmpOp) -> bool {
    match op {
        CmpOp::Eq => left
            .partial_cmp(&right)
            .is_some_and(std::cmp::Ordering::is_eq),
        CmpOp::Ne => !left
            .partial_cmp(&right)
            .is_some_and(std::cmp::Ordering::is_eq),
        CmpOp::Ge => left >= right,
        CmpOp::Le => left <= right,
        CmpOp::Gt => left > right,
        CmpOp::Lt => left < right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(
        items: &'a [(&'a str, PropertyValue)],
    ) -> impl Fn(&str) -> Option<PropertyValue> + 'a {
        move |key| {
            items
                .iter()
                .find(|(id, _)| *id == key)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn truthy_bool() {
        let condition = Condition::parse("foo.value").unwrap();
        assert!(condition.eval(&lookup(&[("foo", PropertyValue::Bool(true))])));
        assert!(!condition.eval(&lookup(&[("foo", PropertyValue::Bool(false))])));
    }

    #[test]
    fn not_inverts() {
        let condition = Condition::parse("!foo.value").unwrap();
        assert!(condition.eval(&lookup(&[("foo", PropertyValue::Bool(false))])));
    }

    #[test]
    fn numeric_eq() {
        let condition = Condition::parse("x.value==1").unwrap();
        assert!(condition.eval(&lookup(&[("x", PropertyValue::Number(1.0))])));
        assert!(!condition.eval(&lookup(&[("x", PropertyValue::Number(2.0))])));
    }

    #[test]
    fn numeric_ge_lt() {
        let ge = Condition::parse("x.value>=5").unwrap();
        let lt = Condition::parse("x.value<5").unwrap();
        assert!(ge.eval(&lookup(&[("x", PropertyValue::Number(5.0))])));
        assert!(!ge.eval(&lookup(&[("x", PropertyValue::Number(4.0))])));
        assert!(lt.eval(&lookup(&[("x", PropertyValue::Number(4.0))])));
        assert!(!lt.eval(&lookup(&[("x", PropertyValue::Number(5.0))])));
    }

    #[test]
    fn string_eq_permissive_coercion() {
        let condition = Condition::parse(r#"x.value=="1""#).unwrap();
        assert!(condition.eval(&lookup(&[("x", PropertyValue::Number(1.0))])));
        assert!(condition.eval(&lookup(&[("x", PropertyValue::String("1".into()))])));
    }

    #[test]
    fn missing_id_is_fail_open() {
        let truthy = Condition::parse("absent.value").unwrap();
        let compare = Condition::parse("absent.value==1").unwrap();
        assert!(truthy.eval(&lookup(&[])));
        assert!(compare.eval(&lookup(&[])));
    }

    #[test]
    fn string_truthy_matches_condition_semantics() {
        let condition = Condition::parse("x.value").unwrap();
        assert!(condition.eval(&lookup(&[("x", PropertyValue::String("yes".into()))])));
        assert!(!condition.eval(&lookup(&[("x", PropertyValue::String(String::new()))])));
        assert!(!condition.eval(&lookup(&[("x", PropertyValue::String("0".into()))])));
        assert!(!condition.eval(&lookup(&[("x", PropertyValue::String("false".into()))])));
    }

    #[test]
    fn keyword_named_property_ids_are_allowed() {
        let condition = Condition::parse("true.value&&value.value&&false.value==true").unwrap();
        assert!(condition.eval(&lookup(&[
            ("true", PropertyValue::Bool(true)),
            ("value", PropertyValue::Bool(true)),
            ("false", PropertyValue::Bool(true)),
        ])));
    }

    #[test]
    fn compound_and() {
        let condition = Condition::parse("a.value&&b.value&&c.value").unwrap();
        assert!(condition.eval(&lookup(&[
            ("a", PropertyValue::Bool(true)),
            ("b", PropertyValue::Bool(true)),
            ("c", PropertyValue::Bool(true)),
        ])));
        assert!(!condition.eval(&lookup(&[
            ("a", PropertyValue::Bool(true)),
            ("b", PropertyValue::Bool(false)),
            ("c", PropertyValue::Bool(true)),
        ])));
    }

    #[test]
    fn or_precedence_without_parentheses() {
        let condition = Condition::parse("a.value||b.value&&c.value").unwrap();
        assert!(condition.eval(&lookup(&[
            ("a", PropertyValue::Bool(true)),
            ("b", PropertyValue::Bool(false)),
            ("c", PropertyValue::Bool(false)),
        ])));
        assert!(!condition.eval(&lookup(&[
            ("a", PropertyValue::Bool(false)),
            ("b", PropertyValue::Bool(true)),
            ("c", PropertyValue::Bool(false)),
        ])));
    }

    #[test]
    fn malformed_fails() {
        assert!(Condition::parse("foo.value = 1").is_err());
        assert!(Condition::parse("foo.value & bar.value").is_err());
        assert!(Condition::parse(r#"foo.value=="unterminated"#).is_err());
        assert!(Condition::parse("foo.value @ 1").is_err());
        assert!(Condition::parse("foo.value==1.2.3").is_err());
    }
}
