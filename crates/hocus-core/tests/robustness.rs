//! What the configuration and session files may hold, mangled: reading
//! them gives a value or an error, never a panic (#134). A small
//! deterministic fuzzer, so that it runs on every CI build.

use hocus_core::{Config, session::Session, shortcuts::Action};

/// xorshift: the same inputs on every run.
struct Rng(u64);

impl Rng {
    fn draw(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
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
];

/// `seed` with a few random edits.
fn mangle(seed: &str, rng: &mut Rng) -> String {
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

const CONFIG: &str = r##"
[appearance]
theme = "dark"
accent = "blue"
text_size = 110
[windows.gaps]
inner = 8
[desktops]
per_output = 4
[keyboard]
layouts = ["us", "fr"]
[recording]
fps = 30
[[rules]]
app_id = "firefox"
opacity = 0.9
desktop = 2
[[outputs]]
name = "DP-1"
mode = "2560x1440@144"
scale = 1.5
position = [0, 0]
[[layouts]]
name = "Home"
outputs = ["eDP-1", "DP-1"]
positions = { DP-1 = [0, 0], eDP-1 = [320, 1440] }
[shortcuts]
"Super+Return" = "spawn foot"
"Alt+F4" = "none"
"Super+Shift+S" = "screenshot region 3"
"##;

const SESSION: &str = r##"
[[window]]
app_id = "foot"
title = "~"
command = ["foot"]
output = "DP-1"
desktop = 2
geometry = [10, 20, 600, 400]
state = "normal"
pinned = false
"##;

#[test]
fn mangled_configurations_read_or_fail_but_never_panic() {
    let mut rng = Rng(0x5eed_cafe);
    for _ in 0..20_000 {
        let text = mangle(CONFIG, &mut rng);
        if let Ok(config) = Config::parse(&text) {
            // What reads also goes all the way through.
            let _ = config.shortcuts();
            let _ = config.to_toml();
        }
    }
}

#[test]
fn mangled_sessions_and_actions_read_or_fail_but_never_panic() {
    let mut rng = Rng(0xbad_5e55);
    for _ in 0..20_000 {
        let text = mangle(SESSION, &mut rng);
        if let Ok(session) = Session::parse(&text) {
            let _ = session.commands();
        }
        let action = mangle("screenshot region 3", &mut rng);
        let _ = action.parse::<Action>();
        let combo = mangle("Super+Shift+S", &mut rng);
        let _ = combo.parse::<hocus_core::shortcuts::Combo>();
    }
}

#[test]
fn deep_nesting_is_refused_not_a_stack_overflow() {
    for depth in [100, 10_000] {
        let text = format!("a = {}{}", "[".repeat(depth), "]".repeat(depth));
        let _ = Config::parse(&text);
        let text = format!("{}x = 1", "[a.".repeat(depth / 10));
        let _ = Config::parse(&text);
    }
}
