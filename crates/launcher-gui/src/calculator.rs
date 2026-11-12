//! The calculator result: typing `12*(3+4)` into the search box shows
//! `= 84` above the applications, as Windows 11's Start search does.
//!
//! A small recursive-descent evaluator over `+ - * / ^`, the `×` and `÷`
//! signs, parentheses, unary minus, and decimals. It runs on every keystroke,
//! so it allocates one token list and nothing else, and it says nothing at
//! all unless the query is unmistakably arithmetic: a lone number, a word, or
//! a version string such as `0.2.9` is not a calculation.

/// One evaluated expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Calculation {
    /// The expression as it was typed, trimmed and without a leading `=`.
    pub expression: String,
    pub value: f64,
    /// The value as it is shown and copied: no exponent, at most ten
    /// decimals, no trailing zeros, no `-0`.
    pub display: String,
}

/// The calculation a query asks for, if it asks for one.
pub fn evaluate(query: &str) -> Option<Calculation> {
    let expression = query.trim();
    let expression = expression.strip_prefix('=').unwrap_or(expression).trim();
    if expression.is_empty() || expression.len() > 256 {
        return None;
    }
    let tokens = tokenize(expression)?;
    // Arithmetic needs an operator between two operands; `42` and `-3` are
    // numbers, not questions.
    let binary = tokens.iter().enumerate().any(|(at, token)| {
        matches!(token, Token::Operator(_))
            && at > 0
            && !matches!(tokens[at - 1], Token::Operator(_) | Token::Open)
    });
    if !binary {
        return None;
    }
    let mut parser = Parser { tokens, at: 0 };
    let value = parser.expression(0)?;
    if parser.at != parser.tokens.len() {
        return None;
    }
    let display = format_value(value)?;
    Some(Calculation {
        expression: expression.to_string(),
        value,
        display,
    })
}

/// How many significant digits a result keeps: what an `f64` holds exactly.
pub const SIGNIFICANT_DIGITS: i32 = 15;

/// A value as Windows Calculator shows it in Start: plain digits to fifteen
/// significant figures, trailing zeros dropped, no `-0`; scientific notation
/// past that range (`1.5e300`). A result that is not a finite number, such
/// as `10^400`, has no display and is not offered.
pub fn format_value(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some("0".to_string());
    }
    let magnitude = value.abs().log10().floor() as i32;
    if !(-6..SIGNIFICANT_DIGITS).contains(&magnitude) {
        // `{:e}` with fourteen decimals is fifteen significant digits.
        let text = format!("{:.*e}", (SIGNIFICANT_DIGITS - 1) as usize, value);
        let (mantissa, exponent) = text.split_once('e')?;
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return Some(format!("{mantissa}e{exponent}"));
    }
    // Only the digits a double holds are shown: fifteen in all, so a large
    // integer keeps no noise after its point and a small value keeps its
    // fraction.
    let decimals = (SIGNIFICANT_DIGITS - 1 - magnitude).max(0) as usize;
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    Some(if text == "-0" { "0".to_string() } else { text })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Number(f64),
    Operator(char),
    Open,
    Close,
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&character) = chars.peek() {
        match character {
            ' ' | '\t' => {
                chars.next();
            }
            '0'..='9' | '.' => {
                let mut number = String::new();
                let mut dots = 0;
                while let Some(&digit) = chars.peek() {
                    if digit.is_ascii_digit() {
                        number.push(digit);
                    } else if digit == '.' {
                        dots += 1;
                        number.push(digit);
                    } else {
                        break;
                    }
                    chars.next();
                }
                if dots > 1 || number == "." {
                    return None;
                }
                tokens.push(Token::Number(number.parse().ok()?));
            }
            '+' | '-' | '*' | '/' | '^' => {
                tokens.push(Token::Operator(character));
                chars.next();
            }
            '×' | 'x' | 'X' => {
                // `x` reads as times only between two operands: `2x3`.
                if character != '×'
                    && !matches!(tokens.last(), Some(Token::Number(_) | Token::Close))
                {
                    return None;
                }
                tokens.push(Token::Operator('*'));
                chars.next();
            }
            '÷' => {
                tokens.push(Token::Operator('/'));
                chars.next();
            }
            '(' => {
                tokens.push(Token::Open);
                chars.next();
            }
            ')' => {
                tokens.push(Token::Close);
                chars.next();
            }
            _ => return None,
        }
    }
    Some(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    /// Precedence climbing: `+ -` bind loosest, then `* /`, then `^`, which
    /// is right-associative as it is on paper.
    fn expression(&mut self, minimum: u8) -> Option<f64> {
        let mut left = self.unary()?;
        while let Some(Token::Operator(operator)) = self.peek() {
            let (precedence, right_associative) = match operator {
                '+' | '-' => (1, false),
                '*' | '/' => (2, false),
                '^' => (3, true),
                _ => return None,
            };
            if precedence < minimum {
                break;
            }
            self.at += 1;
            let next = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            let right = self.expression(next)?;
            left = match operator {
                '+' => left + right,
                '-' => left - right,
                '*' => left * right,
                '/' if right == 0.0 => return None,
                '/' => left / right,
                '^' => left.powf(right),
                _ => return None,
            };
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            Token::Operator('-') => {
                self.at += 1;
                Some(-self.unary()?)
            }
            Token::Operator('+') => {
                self.at += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Option<f64> {
        match self.peek()? {
            Token::Number(value) => {
                self.at += 1;
                Some(value)
            }
            Token::Open => {
                self.at += 1;
                let value = self.expression(0)?;
                if self.peek()? != Token::Close {
                    return None;
                }
                self.at += 1;
                Some(value)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(query: &str) -> Option<String> {
        evaluate(query).map(|calculation| calculation.display)
    }

    #[test]
    fn arithmetic_is_answered_with_the_usual_precedence() {
        assert_eq!(shown("12*(3+4)").as_deref(), Some("84"));
        assert_eq!(shown("2 + 3 * 4").as_deref(), Some("14"));
        assert_eq!(shown("(2 + 3) * 4").as_deref(), Some("20"));
        assert_eq!(shown("10 / 4").as_deref(), Some("2.5"));
        assert_eq!(shown("2^10").as_deref(), Some("1024"));
        assert_eq!(
            shown("2^3^2").as_deref(),
            Some("512"),
            "power is right-associative"
        );
        assert_eq!(shown("-3 + 5").as_deref(), Some("2"));
        assert_eq!(shown("4 - -2").as_deref(), Some("6"));
        assert_eq!(shown("=1+1").as_deref(), Some("2"));
        assert_eq!(shown("6 × 7").as_deref(), Some("42"));
        assert_eq!(shown("2x3").as_deref(), Some("6"));
        assert_eq!(shown("9 ÷ 3").as_deref(), Some("3"));
        assert_eq!(shown(".5 + .25").as_deref(), Some("0.75"));
    }

    #[test]
    fn floating_point_noise_is_rounded_away() {
        assert_eq!(shown("0.1 + 0.2").as_deref(), Some("0.3"));
        assert_eq!(shown("1 / 3").as_deref(), Some("0.333333333333333"));
        assert_eq!(shown("-0.0001 * 0").as_deref(), Some("0"));
    }

    #[test]
    fn anything_that_is_not_arithmetic_says_nothing() {
        for query in [
            "",
            "42",
            "-3",
            "cal",
            "calculator",
            "0.2.9",
            "x2",
            "2+",
            "(1+2",
            "1+2)",
            "1 +* 2",
            "1/0",
            "5 / (2-2)",
            "a+b",
            "gimp 2",
            "x",
            "3x",
        ] {
            assert_eq!(evaluate(query), None, "{query:?}");
        }
    }

    #[test]
    fn a_result_keeps_what_was_typed() {
        let calculation = evaluate("  = 2*21 ").expect("arithmetic");
        assert_eq!(calculation.expression, "2*21");
        assert_eq!(calculation.value, 42.0);
    }

    #[test]
    fn large_and_small_values_stay_in_plain_digits() {
        assert_eq!(format_value(1e14).as_deref(), Some("100000000000000"));
        assert_eq!(format_value(-2.5).as_deref(), Some("-2.5"));
        assert_eq!(format_value(-0.0).as_deref(), Some("0"));
    }

    #[test]
    fn a_large_product_shows_no_digits_the_double_does_not_hold() {
        // 12345678 × 1234567 = 15241566651426 exactly; rounding through
        // `value * 1e10` used to print 15241566651426.001953125.
        assert_eq!(shown("12345678*1234567").as_deref(), Some("15241566651426"));
        assert_eq!(shown("2^60").as_deref(), Some("1.15292150460685e18"));
        assert_eq!(
            shown("10^300").as_deref(),
            Some("1e300"),
            "used to read inf"
        );
        assert_eq!(shown("1/3*1000000").as_deref(), Some("333333.333333333"));
        assert_eq!(shown("1/1000000000").as_deref(), Some("1e-9"));
    }

    #[test]
    fn a_result_beyond_a_double_is_not_offered() {
        assert_eq!(evaluate("10^400"), None);
        assert_eq!(evaluate("-(10^400)"), None);
        assert_eq!(format_value(f64::INFINITY), None);
        assert_eq!(format_value(f64::NAN), None);
    }
}
