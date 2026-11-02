//! What the launcher's field understands besides applications (5.2): a
//! calculation, a conversion, an address; and the web search offered for
//! anything typed.
//!
//! - **A calculation**: `+ − × ÷ ^ %`, parentheses and square roots
//!   (`sqrt`, `√`); a comma or a point as the decimal mark; `x`, `×` or `*`
//!   multiply, `÷` or `/` divide. It counts only with an operator (a bare
//!   number is not one), unless it starts with `=`.
//! - **A conversion**: `<number> <unit> en|to|vers|-> <unit>`, the units'
//!   French and English names and symbols, accents and case ignored:
//!   lengths, masses, times, volumes, temperatures, data (decimal and
//!   binary), and currencies at the European Central Bank's rates
//!   ([`super::rates`]).
//! - **An address**: `scheme://…`, `localhost[:port][/…]`, or
//!   `name.tld[:port][/…]`, without spaces.
//!
//! The 5.2 mockup's logic is the reference for what counts; this is our
//! own reading of it.

use super::{Words, rates::Rates, search::fold};

/// How numbers are written: French (`1 234,5`) or English (`1,234.5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numbers {
    fr: bool,
}

impl Numbers {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self {
            fr: lang == crate::clock::time::Lang::French,
        }
    }

    /// `value` with up to six decimals and the thousands separated, from
    /// its twelve first significant digits (what a float holds of a
    /// decimal calculation: `0.1 + 0.2` is `0.3`). `None` when it is not a
    /// number.
    pub fn format(self, value: f64) -> Option<String> {
        self.write(value, 6, false)
    }

    /// A conversion's `value`, as it reads well: four significant digits
    /// (none to three decimals, their trailing zeros dropped); a currency's
    /// always with two decimals.
    pub fn format_converted(self, value: f64, kind: Kind) -> Option<String> {
        if kind == Kind::Currency {
            return self.write(value, 2, true);
        }
        let whole = value.abs().trunc();
        let digits = if whole < 1.0 {
            1
        } else {
            whole.log10().floor() as i64 + 1
        };
        self.write(value, (4 - digits).clamp(0, 3) as usize, false)
    }

    /// `value` with `places` decimals, their trailing zeros dropped unless
    /// `kept`, from its twelve first significant digits.
    fn write(self, value: f64, places: usize, kept: bool) -> Option<String> {
        if !value.is_finite() {
            return None;
        }
        let value: f64 = format!("{value:.11e}").parse().ok()?;
        let digits = format!("{:.*}", places, value.abs());
        let (whole, decimals) = digits.split_once('.').unwrap_or((&digits, ""));
        let decimals = if kept {
            decimals
        } else {
            decimals.trim_end_matches('0')
        };
        let (group, mark) = if self.fr {
            ('\u{202f}', ',')
        } else {
            (',', '.')
        };
        let mut out = String::new();
        let zero = whole == "0" && decimals.chars().all(|c| c == '0');
        if value < 0.0 && !zero {
            out.push('-');
        }
        for (i, digit) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                out.push(group);
            }
            out.push(digit);
        }
        if !decimals.is_empty() {
            out.push(mark);
            out.push_str(decimals);
        }
        Some(out)
    }
}

/// An answer as its card shows it: the result, the line under it, and
/// what `Enter` copies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub main: String,
    pub sub: String,
    pub copy: String,
}

/// The answer to `text`, if it is a calculation or a conversion.
pub fn answer(text: &str, rates: Option<&Rates>, numbers: Numbers, words: Words) -> Option<Answer> {
    if let Some(value) = calc(text) {
        let shown = numbers.format(value)?;
        return Some(Answer {
            main: shown.clone(),
            sub: format!("{} =", pretty(text)),
            copy: shown,
        });
    }
    let conversion = convert(text, rates)?;
    let value = numbers.format_converted(conversion.value, conversion.to.kind)?;
    let from = numbers.format(conversion.from)?;
    let to = show(&conversion.to.key, words);
    let kind = match (&conversion.to.kind, rates) {
        (Kind::Currency, Some(rates)) => words.rates_of(&rates.date),
        (kind, _) => kind.name(words).to_owned(),
    };
    Some(Answer {
        main: format!("{value} {to}"),
        sub: format!(
            "{from} {} = {value} {to} · {kind}",
            show(&conversion.unit.key, words)
        ),
        copy: value,
    })
}

// --- Calculations ------------------------------------------------------------------

/// What `text` computes to, if it is a calculation: with an operator, or
/// after `=`; finite.
pub fn calc(text: &str) -> Option<f64> {
    let text = text.trim();
    let (forced, text) = match text.strip_prefix('=') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let lowered = text.to_lowercase().replace("sqrt", "√");
    let mut chars = Vec::new();
    for c in lowered.chars() {
        let c = match c {
            c if c.is_whitespace() => continue,
            ',' => '.',
            'x' | '×' | '*' | '·' => '*',
            '÷' | '/' => '/',
            '−' | '-' => '-',
            '0'..='9' | '.' | '+' | '^' | '%' | '(' | ')' | '√' => c,
            _ => return None,
        };
        chars.push(c);
    }
    if !chars.iter().any(char::is_ascii_digit) {
        return None;
    }
    let operated = chars
        .iter()
        .skip(usize::from(chars.first() == Some(&'-')))
        .any(|c| "+-*/^%(√".contains(*c));
    if !forced && !operated {
        return None;
    }
    let mut parser = Parser { chars, at: 0 };
    let value = parser.expr()?;
    (parser.at == parser.chars.len() && value.is_finite()).then_some(value)
}

/// A calculation's characters, read from `at`: the usual precedence, `^`
/// to the right, `%` a hundredth of what it follows.
struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let there = self.peek() == Some(c);
        self.at += usize::from(there);
        there
    }

    fn expr(&mut self) -> Option<f64> {
        let mut value = self.term()?;
        loop {
            if self.eat('+') {
                value += self.term()?;
            } else if self.eat('-') {
                value -= self.term()?;
            } else {
                return Some(value);
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut value = self.power()?;
        loop {
            if self.eat('*') {
                value *= self.power()?;
            } else if self.eat('/') {
                value /= self.power()?;
            } else {
                return Some(value);
            }
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat('^') {
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
        let mut value = self.primary()?;
        while self.eat('%') {
            value /= 100.0;
        }
        Some(value)
    }

    fn primary(&mut self) -> Option<f64> {
        if self.eat('√') {
            return Some(self.primary()?.sqrt());
        }
        if self.eat('(') {
            let value = self.expr()?;
            return self.eat(')').then_some(value);
        }
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        if self.eat('.') {
            let decimals = self.at;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.at += 1;
            }
            if self.at == decimals {
                return None;
            }
        }
        if self.at == start {
            return None;
        }
        self.chars[start..self.at]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }
}

/// A calculation as its card shows it under the result: its operators
/// spaced, `×` and `÷` for `*` and `/`.
pub fn pretty(text: &str) -> String {
    let text = text.trim();
    let text = text.strip_prefix('=').unwrap_or(text);
    let mut out = String::new();
    // Whether what came last takes a sign after it as its own (`2 × -3`).
    let mut after_operator = true;
    for c in text.chars() {
        let shown = match c {
            '*' | 'x' | 'X' | '×' | '·' => Some('×'),
            '/' | '÷' => Some('÷'),
            '+' | '-' | '−' | '^' => Some(c),
            _ => None,
        };
        match shown {
            Some(sign) if after_operator && matches!(sign, '+' | '-' | '−') => out.push(sign),
            Some(op) => {
                out.push(' ');
                out.push(op);
                out.push(' ');
                after_operator = true;
            }
            None if c.is_whitespace() => out.push(' '),
            None => {
                out.push(c);
                after_operator = c == '(';
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// --- Conversions -------------------------------------------------------------------

/// What a unit measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Length,
    Mass,
    Time,
    Volume,
    Temperature,
    Data,
    Currency,
}

impl Kind {
    fn name(self, words: Words) -> &'static str {
        match self {
            Self::Length => words.pick("Longueur", "Length"),
            Self::Mass => words.pick("Masse", "Mass"),
            Self::Time => words.pick("Durée", "Time"),
            Self::Volume => words.pick("Volume", "Volume"),
            Self::Temperature => words.pick("Température", "Temperature"),
            Self::Data => words.pick("Données", "Data"),
            Self::Currency => words.pick("Monnaie", "Currency"),
        }
    }
}

/// The units, by their key: what they measure, and how many of the
/// kind's base (metre, kilogram, second, litre, byte) each is. The
/// temperatures go through kelvins instead, the currencies through euros.
const UNITS: &[(&str, Kind, f64)] = &[
    ("m", Kind::Length, 1.0),
    ("km", Kind::Length, 1000.0),
    ("cm", Kind::Length, 0.01),
    ("mm", Kind::Length, 0.001),
    ("mi", Kind::Length, 1609.344),
    ("yd", Kind::Length, 0.9144),
    ("ft", Kind::Length, 0.3048),
    ("in", Kind::Length, 0.0254),
    ("kg", Kind::Mass, 1.0),
    ("g", Kind::Mass, 0.001),
    ("mg", Kind::Mass, 0.000_001),
    ("t", Kind::Mass, 1000.0),
    ("lb", Kind::Mass, 0.453_592_37),
    ("oz", Kind::Mass, 0.028_349_523_125),
    ("ms", Kind::Time, 0.001),
    ("s", Kind::Time, 1.0),
    ("min", Kind::Time, 60.0),
    ("h", Kind::Time, 3600.0),
    ("j", Kind::Time, 86_400.0),
    ("sem", Kind::Time, 604_800.0),
    ("l", Kind::Volume, 1.0),
    ("dl", Kind::Volume, 0.1),
    ("cl", Kind::Volume, 0.01),
    ("ml", Kind::Volume, 0.001),
    ("gal", Kind::Volume, 3.785_411_784),
    ("c", Kind::Temperature, 0.0),
    ("f", Kind::Temperature, 0.0),
    ("k", Kind::Temperature, 0.0),
    ("o", Kind::Data, 1.0),
    ("ko", Kind::Data, 1e3),
    ("mo", Kind::Data, 1e6),
    ("go", Kind::Data, 1e9),
    ("to", Kind::Data, 1e12),
    ("kio", Kind::Data, 1024.0),
    ("mio", Kind::Data, 1_048_576.0),
    ("gio", Kind::Data, 1_073_741_824.0),
    ("tio", Kind::Data, 1_099_511_627_776.0),
];

/// Other names of the units (folded: lower case, without accents), French
/// and English, and the currencies' symbols and names.
const ALIASES: &[(&str, &str)] = &[
    ("metre", "m"),
    ("metres", "m"),
    ("meter", "m"),
    ("meters", "m"),
    ("kilometre", "km"),
    ("kilometres", "km"),
    ("kilometer", "km"),
    ("kilometers", "km"),
    ("centimetre", "cm"),
    ("centimetres", "cm"),
    ("centimeter", "cm"),
    ("centimeters", "cm"),
    ("millimetre", "mm"),
    ("millimetres", "mm"),
    ("millimeter", "mm"),
    ("millimeters", "mm"),
    ("mile", "mi"),
    ("miles", "mi"),
    ("yard", "yd"),
    ("yards", "yd"),
    ("pied", "ft"),
    ("pieds", "ft"),
    ("foot", "ft"),
    ("feet", "ft"),
    ("pouce", "in"),
    ("pouces", "in"),
    ("inch", "in"),
    ("inches", "in"),
    ("kilo", "kg"),
    ("kilos", "kg"),
    ("kilogramme", "kg"),
    ("kilogrammes", "kg"),
    ("kilogram", "kg"),
    ("kilograms", "kg"),
    ("gramme", "g"),
    ("grammes", "g"),
    ("gram", "g"),
    ("grams", "g"),
    ("milligramme", "mg"),
    ("milligrammes", "mg"),
    ("milligram", "mg"),
    ("milligrams", "mg"),
    ("tonne", "t"),
    ("tonnes", "t"),
    ("livre", "lb"),
    ("livres", "lb"),
    ("pound", "lb"),
    ("pounds", "lb"),
    ("lbs", "lb"),
    ("once", "oz"),
    ("onces", "oz"),
    ("ounce", "oz"),
    ("ounces", "oz"),
    ("milliseconde", "ms"),
    ("millisecondes", "ms"),
    ("millisecond", "ms"),
    ("milliseconds", "ms"),
    ("sec", "s"),
    ("seconde", "s"),
    ("secondes", "s"),
    ("second", "s"),
    ("seconds", "s"),
    ("mn", "min"),
    ("minute", "min"),
    ("minutes", "min"),
    ("heure", "h"),
    ("heures", "h"),
    ("hour", "h"),
    ("hours", "h"),
    ("hr", "h"),
    ("jour", "j"),
    ("jours", "j"),
    ("day", "j"),
    ("days", "j"),
    ("d", "j"),
    ("semaine", "sem"),
    ("semaines", "sem"),
    ("week", "sem"),
    ("weeks", "sem"),
    ("wk", "sem"),
    ("litre", "l"),
    ("litres", "l"),
    ("liter", "l"),
    ("liters", "l"),
    ("decilitre", "dl"),
    ("decilitres", "dl"),
    ("deciliter", "dl"),
    ("deciliters", "dl"),
    ("centilitre", "cl"),
    ("centilitres", "cl"),
    ("centiliter", "cl"),
    ("centiliters", "cl"),
    ("millilitre", "ml"),
    ("millilitres", "ml"),
    ("milliliter", "ml"),
    ("milliliters", "ml"),
    ("gallon", "gal"),
    ("gallons", "gal"),
    ("°c", "c"),
    ("celsius", "c"),
    ("°f", "f"),
    ("fahrenheit", "f"),
    ("kelvin", "k"),
    ("kelvins", "k"),
    ("b", "o"),
    ("octet", "o"),
    ("octets", "o"),
    ("byte", "o"),
    ("bytes", "o"),
    ("kb", "ko"),
    ("mb", "mo"),
    ("gb", "go"),
    ("tb", "to"),
    ("kib", "kio"),
    ("mib", "mio"),
    ("gib", "gio"),
    ("tib", "tio"),
    ("€", "eur"),
    ("euro", "eur"),
    ("euros", "eur"),
    ("$", "usd"),
    ("dollar", "usd"),
    ("dollars", "usd"),
    ("£", "gbp"),
    ("¥", "jpy"),
    ("yen", "jpy"),
    ("yens", "jpy"),
];

/// A unit typed: its key, and what it measures.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub key: String,
    pub kind: Kind,
}

/// The unit `word` names, a currency only when its rate is known.
fn unit(word: &str, rates: Option<&Rates>) -> Option<Unit> {
    let folded = fold(word);
    let key = ALIASES
        .iter()
        .find(|(alias, _)| *alias == folded)
        .map_or(folded.as_str(), |(_, key)| *key);
    if let Some((key, kind, _)) = UNITS.iter().find(|(k, ..)| *k == key) {
        return Some(Unit {
            key: (*key).to_owned(),
            kind: *kind,
        });
    }
    rates?.per_eur.get(key).map(|_| Unit {
        key: key.to_owned(),
        kind: Kind::Currency,
    })
}

/// `value` of `unit` in its kind's base: metres, kilograms, seconds,
/// litres, bytes, kelvins or euros.
fn to_base(unit: &Unit, value: f64, rates: Option<&Rates>) -> Option<f64> {
    match (unit.kind, unit.key.as_str()) {
        (Kind::Temperature, "c") => Some(value + 273.15),
        (Kind::Temperature, "f") => Some((value - 32.0) * 5.0 / 9.0 + 273.15),
        (Kind::Temperature, _) => Some(value),
        (Kind::Currency, key) => Some(value / rates?.per_eur.get(key)?),
        (_, key) => UNITS
            .iter()
            .find(|(k, ..)| *k == key)
            .map(|(_, _, factor)| value * factor),
    }
}

fn from_base(unit: &Unit, value: f64, rates: Option<&Rates>) -> Option<f64> {
    match (unit.kind, unit.key.as_str()) {
        (Kind::Temperature, "c") => Some(value - 273.15),
        (Kind::Temperature, "f") => Some((value - 273.15) * 9.0 / 5.0 + 32.0),
        (Kind::Temperature, _) => Some(value),
        (Kind::Currency, key) => Some(value * rates?.per_eur.get(key)?),
        (_, key) => UNITS
            .iter()
            .find(|(k, ..)| *k == key)
            .map(|(_, _, factor)| value / factor),
    }
}

/// A unit's key as the card writes it.
fn show(key: &str, words: Words) -> String {
    let shown = match key {
        "c" => "°C",
        "f" => "°F",
        "k" => "K",
        "eur" => "€",
        "usd" => "$",
        "gbp" => "£",
        "jpy" => "¥",
        "j" => words.pick("j", "d"),
        "sem" => words.pick("sem", "wk"),
        "o" => words.pick("o", "B"),
        "ko" => words.pick("Ko", "kB"),
        "mo" => words.pick("Mo", "MB"),
        "go" => words.pick("Go", "GB"),
        "to" => words.pick("To", "TB"),
        "kio" => words.pick("Kio", "KiB"),
        "mio" => words.pick("Mio", "MiB"),
        "gio" => words.pick("Gio", "GiB"),
        "tio" => words.pick("Tio", "TiB"),
        key if UNITS.iter().any(|(k, ..)| *k == key) => key,
        // Another currency: its code.
        key => return key.to_uppercase(),
    };
    shown.to_owned()
}

/// A conversion understood: `from` of `unit` is `value` of `to`.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversion {
    pub from: f64,
    pub unit: Unit,
    pub value: f64,
    pub to: Unit,
}

/// The words between a quantity and the unit it is wanted in.
const INTO: [&str; 4] = ["en", "to", "vers", "->"];

/// `text` as a conversion, `<number> <unit> en|to|vers|-> <unit>`, if it
/// is one of two different units of the same kind.
pub fn convert(text: &str, rates: Option<&Rates>) -> Option<Conversion> {
    let spaced = text.replace("->", " -> ");
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let [left @ .., into, target] = words.as_slice() else {
        return None;
    };
    if left.is_empty() || !INTO.contains(&fold(into).as_str()) {
        return None;
    }
    let left = left.join(" ");
    // The unit: what ends the quantity after its digits.
    let unit_at = left
        .char_indices()
        .rev()
        .take_while(|(_, c)| !(c.is_ascii_digit() || c.is_whitespace() || matches!(c, '.' | ',')))
        .last()
        .map(|(at, _)| at)?;
    let (number, from_word) = left.split_at(unit_at);
    let from = number_of(number)?;
    let unit = unit(from_word, rates)?;
    let to = self::unit(target, rates)?;
    if unit.kind != to.kind || unit.key == to.key {
        return None;
    }
    let value = from_base(&to, to_base(&unit, from, rates)?, rates)?;
    value.is_finite().then_some(Conversion {
        from,
        unit,
        value,
        to,
    })
}

/// A quantity as typed: digits, spaces between thousands, a comma or a
/// point before the decimals, a sign.
fn number_of(text: &str) -> Option<f64> {
    let digits: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    let unsigned = digits.strip_prefix('-').unwrap_or(&digits);
    let fine = unsigned.chars().any(|c| c.is_ascii_digit())
        && unsigned.chars().all(|c| c.is_ascii_digit() || c == '.')
        && unsigned.matches('.').count() <= 1
        && !unsigned.ends_with('.');
    fine.then(|| digits.parse().ok())?
}

// --- Addresses and the web ---------------------------------------------------------

/// The address `text` is, made whole: `scheme://…` as typed, `localhost`
/// over `http`, a domain over `https`; `None` when it is none.
pub fn address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some((scheme, rest)) = text.split_once("://") {
        let mut letters = scheme.chars();
        let fine = letters.next().is_some_and(|c| c.is_ascii_alphabetic())
            && letters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
        return (fine && !rest.is_empty()).then(|| text.to_owned());
    }
    let host_end = text.find('/').unwrap_or(text.len());
    let (host, port) = match text[..host_end].split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (&text[..host_end], None),
    };
    if port.is_some_and(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Some(format!("http://{text}"));
    }
    let labels: Vec<&str> = host.split('.').collect();
    let fine = labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && labels
            .last()
            .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
    fine.then(|| format!("https://{text}"))
}

/// The web search for `query`: DuckDuckGo's (the setting comes with the
/// settings).
pub fn web(query: &str) -> String {
    let mut url = String::from("https://duckduckgo.com/?q=");
    for byte in query.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                url.push(char::from(byte));
            }
            b' ' => url.push('+'),
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const FR: Numbers = Numbers { fr: true };
    const EN: Numbers = Numbers { fr: false };

    #[test]
    fn numbers_are_written_for_the_locale() {
        assert_eq!(FR.format(6.213_711_922_37).as_deref(), Some("6,213712"));
        assert_eq!(EN.format(6.213_711_922_37).as_deref(), Some("6.213712"));
        assert_eq!(
            FR.format(1_234_567.5).as_deref(),
            Some("1\u{202f}234\u{202f}567,5")
        );
        assert_eq!(EN.format(1_234_567.5).as_deref(), Some("1,234,567.5"));
        assert_eq!(EN.format(0.1 + 0.2).as_deref(), Some("0.3"));
        assert_eq!(EN.format(-84.0).as_deref(), Some("-84"));
        assert_eq!(EN.format(-0.000_000_1).as_deref(), Some("0"));
        assert_eq!(EN.format(f64::INFINITY), None);
    }

    #[test]
    fn calculations_need_an_operator_unless_they_start_with_equals() {
        assert_eq!(calc("12 × (3 + 4)"), Some(84.0));
        assert_eq!(calc("2x3"), Some(6.0));
        assert_eq!(calc("2 * 3 ÷ 4"), Some(1.5));
        assert_eq!(calc("10 / 4"), Some(2.5));
        assert_eq!(calc("1,5 + 1.5"), Some(3.0));
        assert_eq!(calc("2^3^2"), Some(512.0));
        assert_eq!(calc("200 × 15%"), Some(30.0));
        assert_eq!(calc("sqrt(16) + √9"), Some(7.0));
        assert_eq!(calc("−3 − 4"), Some(-7.0));
        assert_eq!(calc("-(2+3)*2"), Some(-10.0));
        // A bare number is not one, unless forced.
        assert_eq!(calc("42"), None);
        assert_eq!(calc("-42"), None);
        assert_eq!(calc("3,5"), None);
        assert_eq!(calc("=42"), Some(42.0));
        // Not calculations.
        assert_eq!(calc("1 / 0"), None);
        assert_eq!(calc("2 +"), None);
        assert_eq!(calc("(2 + 3"), None);
        assert_eq!(calc("firefox"), None);
        assert_eq!(calc("10 km en mi"), None);
        assert_eq!(calc("+"), None);
        assert_eq!(pretty("12*(3+4)"), "12 × (3 + 4)");
        assert_eq!(pretty("= 2x-3 / 4"), "2 × -3 ÷ 4");
    }

    fn rates() -> Rates {
        Rates {
            date: "2026-10-30".to_owned(),
            per_eur: HashMap::from([
                ("eur".to_owned(), 1.0),
                ("usd".to_owned(), 1.08),
                ("chf".to_owned(), 0.94),
            ]),
        }
    }

    fn close(conversion: Option<Conversion>, want: f64) {
        let value = conversion.expect("a conversion").value;
        assert!(
            (value - want).abs() < 1e-9 * want.abs().max(1.0),
            "{value} {want}"
        );
    }

    #[test]
    fn units_convert_within_their_kind() {
        let rates = rates();
        let r = Some(&rates);
        close(convert("10 km en miles", r), 6.213_711_922_373_34);
        close(convert("10km to mi", r), 6.213_711_922_373_34);
        close(convert("1,5 go en mo", r), 1500.0);
        close(convert("1 Gio vers Mo", r), 1_073.741_824);
        close(convert("1 To to GB", r), 1000.0);
        close(convert("22 °C en F", r), 71.6);
        close(convert("-40 fahrenheit -> celsius", r), -40.0);
        close(convert("0 k en °c", r), -273.15);
        close(convert("2 heures en minutes", r), 120.0);
        close(convert("1 semaine en jours", r), 7.0);
        close(convert("1 000 g en kg", r), 1.0);
        close(convert("3 Pieds en mètres", r), 0.9144);
        close(convert("1 gallon en litres", r), 3.785_411_784);
        // Currencies at the rates, through the euro.
        close(convert("108 usd en eur", r), 100.0);
        close(convert("108 $ en CHF", r), 94.0);
        // Not conversions: another kind, the same unit, an unknown unit or
        // currency, a currency without rates.
        assert_eq!(convert("10 km en kg", r), None);
        assert_eq!(convert("10 km en km", r), None);
        assert_eq!(convert("10 parsecs en km", r), None);
        assert_eq!(convert("10 gbp en eur", r), None);
        assert_eq!(convert("10 usd en eur", None), None);
        assert_eq!(convert("km en mi", r), None);
        assert_eq!(convert("10 km mi", r), None);
    }

    #[test]
    fn a_conversion_answers_with_its_kind_and_the_rates_date() {
        let rates = rates();
        let fr = Words { fr: true };
        let answer = answer("10 km en miles", Some(&rates), FR, fr).expect("an answer");
        assert_eq!(answer.main, "6,214 mi");
        assert_eq!(answer.sub, "10 km = 6,214 mi · Longueur");
        assert_eq!(answer.copy, "6,214");
        let main = |text: &str| super::answer(text, Some(&rates), FR, fr).map(|a| a.main);
        assert_eq!(main("22 °C en F").as_deref(), Some("71,6 °F"));
        assert_eq!(main("1,5 go en mo").as_deref(), Some("1\u{202f}500 Mo"));
        assert_eq!(main("1 o en To").as_deref(), Some("0 To"));
        let answer = answer_en("150 usd to eur", &rates);
        assert_eq!(answer.main, "138.89 €");
        assert_eq!(answer.copy, "138.89");
        assert_eq!(
            answer.sub,
            "150 $ = 138.89 € · Currency, rates of 2026-10-30"
        );
        assert_eq!(answer_en("108 usd to eur", &rates).main, "100.00 €");
        let calc = super::answer("12 × (3 + 4)", None, FR, fr).expect("an answer");
        assert_eq!(
            (calc.main.as_str(), calc.sub.as_str()),
            ("84", "12 × (3 + 4) =")
        );
    }

    fn answer_en(text: &str, rates: &Rates) -> Answer {
        answer(text, Some(rates), EN, Words { fr: false }).expect("an answer")
    }

    #[test]
    fn addresses_are_made_whole() {
        assert_eq!(address("github.com").as_deref(), Some("https://github.com"));
        assert_eq!(
            address("docs.rs/iced/latest").as_deref(),
            Some("https://docs.rs/iced/latest")
        );
        assert_eq!(
            address("localhost:3000/api").as_deref(),
            Some("http://localhost:3000/api")
        );
        assert_eq!(
            address("http://example.org").as_deref(),
            Some("http://example.org")
        );
        assert_eq!(
            address("example.org:8080").as_deref(),
            Some("https://example.org:8080")
        );
        assert_eq!(address("3.14"), None);
        assert_eq!(address("firefox"), None);
        assert_eq!(address("recette crêpes.fr"), None);
        assert_eq!(address("localhost:"), None);
        assert_eq!(address("://x"), None);
        assert_eq!(
            web("recette crêpes & co"),
            "https://duckduckgo.com/?q=recette+cr%C3%AApes+%26+co"
        );
    }
}
