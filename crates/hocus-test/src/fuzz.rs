//! A small deterministic fuzzer: random edits of good inputs, the same on
//! every run, so that it runs on every CI build (#134).

/// xorshift: the same inputs on every run.
#[derive(Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn draw(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.draw() % n.max(1) as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "[",
    "]",
    "[[",
    "]]",
    "=",
    "\"",
    "'",
    "\n",
    "#",
    ",",
    "{",
    "}",
    ".",
    "-",
    "+",
    "0",
    "1e999",
    "-1",
    "4294967296",
    "true",
    "nan",
    "inf",
    "é",
    "\u{0}",
    "\u{FEFF}",
    "🐼",
    "\\u0000",
    "\\",
    "layouts",
    "outputs",
    "shortcuts",
    "Super+",
    "none",
    "spawn ",
    "screenshot ",
    "desktop 0",
    // Calendars: iCalendar and WebDAV.
    ";",
    ":",
    "\r\n ",
    "BEGIN:VEVENT\n",
    "END:VEVENT\n",
    "RRULE:FREQ=DAILY;INTERVAL=",
    ";BYDAY=-128MO",
    ";BYMONTHDAY=-31",
    ";COUNT=",
    ";UNTIL=99991231",
    "DTSTART;TZID=Nowhere/Never:",
    "VALUE=DATE",
    "DURATION:P99999D",
    "<",
    ">",
    "&amp;",
    "&#1114112;",
    "<![CDATA[",
    "]]>",
];

/// `seed` with a few random edits.
pub fn mangle(seed: &str, rng: &mut Rng) -> String {
    let mut chars: Vec<char> = seed.chars().collect();
    for _ in 0..=rng.below(8) {
        let at = rng.below(chars.len() + 1);
        match rng.below(5) {
            0 if !chars.is_empty() => {
                let end = (at + rng.below(16)).min(chars.len());
                chars.drain(at.min(end)..end);
            }
            1 => {
                let piece = PIECES[rng.below(PIECES.len())];
                chars.splice(at..at, piece.chars());
            }
            2 if !chars.is_empty() => {
                let i = at.min(chars.len() - 1);
                chars[i] = char::from_u32(rng.below(0x3000) as u32).unwrap_or('?');
            }
            3 if !chars.is_empty() => {
                let from = rng.below(chars.len());
                let to = (from + rng.below(40)).min(chars.len());
                let copy: Vec<char> = chars[from..to].to_vec();
                chars.splice(at..at, copy);
            }
            _ => chars.truncate(at),
        }
    }
    chars.into_iter().collect()
}
