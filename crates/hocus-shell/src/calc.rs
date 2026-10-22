//! The calculator provider: arithmetic typed in the launcher, `2*21`.
//! Numbers take a point or a comma; `^` is a power, `%` a remainder.

/// The value of `input` when it is arithmetic with at least one operator,
/// formatted to read: `42`, `0.333333333`.
pub fn evaluate(input: &str) -> Option<String> {
    let text: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if text.is_empty() || !text.chars().any(|c| "+-*/^%×÷".contains(c)) {
        return None;
    }
    let mut parser = Parser {
        chars: text.chars().collect(),
        at: 0,
    };
    let value = parser.sum()?;
    if parser.at != parser.chars.len() || !value.is_finite() {
        return None;
    }
    Some(format(value))
}

fn format(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let s = format!("{value:.9}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            if self.eat('+') {
                value += self.product()?;
            } else if self.eat('-') {
                value -= self.product()?;
            } else {
                return Some(value);
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut value = self.power()?;
        loop {
            if self.eat('*') || self.eat('×') {
                value *= self.power()?;
            } else if self.eat('/') || self.eat('÷') {
                let d = self.power()?;
                if d == 0.0 {
                    return None;
                }
                value /= d;
            } else if self.eat('%') {
                let d = self.power()?;
                if d == 0.0 {
                    return None;
                }
                value %= d;
            } else {
                return Some(value);
            }
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat('^') {
            // Right to left: 2^3^2 is 2^9.
            return Some(base.powf(self.power()?));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.atom()
    }

    fn atom(&mut self) -> Option<f64> {
        if self.eat('(') {
            let value = self.sum()?;
            return self.eat(')').then_some(value);
        }
        let start = self.at;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' || c == ',' {
                self.at += 1;
            } else {
                break;
            }
        }
        let number: String = self.chars[start..self.at]
            .iter()
            .map(|&c| if c == ',' { '.' } else { c })
            .collect();
        number.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_reads_as_expected() {
        assert_eq!(evaluate("2*21").as_deref(), Some("42"));
        assert_eq!(evaluate("2 + 3 * 4").as_deref(), Some("14"));
        assert_eq!(evaluate("(2+3)*4").as_deref(), Some("20"));
        assert_eq!(evaluate("2^3^2").as_deref(), Some("512"));
        assert_eq!(evaluate("-3+1").as_deref(), Some("-2"));
        assert_eq!(evaluate("1/3").as_deref(), Some("0.333333333"));
        assert_eq!(evaluate("2,5*2").as_deref(), Some("5"));
    }

    #[test]
    fn what_is_not_arithmetic_is_left_alone() {
        assert_eq!(evaluate("firefox"), None);
        assert_eq!(evaluate("42"), None);
        assert_eq!(evaluate("1/0"), None);
        assert_eq!(evaluate("2*"), None);
        assert_eq!(evaluate("(2+3"), None);
    }
}
