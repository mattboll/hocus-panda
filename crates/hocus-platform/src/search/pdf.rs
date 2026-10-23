//! The text layer of a PDF, for search: no layout, no pictures, only the
//! words in reading order as the pages draw them. Objects are found by
//! scanning the file (a broken cross-reference table does not matter),
//! object streams are opened, content streams inflated, and strings
//! decoded through each font's ToUnicode map, or its simple encoding.
//! Encrypted files and filters other than Flate give no text.

use std::{collections::HashMap, io::Read};

/// What an object stream or a page may inflate to, at most.
const MAX_STREAM: u64 = 32 << 20;
/// How deep pages, forms and arrays nest before the rest is ignored.
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
enum Obj {
    Null,
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    Array(Vec<Obj>),
    Dict(Dict),
    Ref(u32),
    /// A keyword: an operator in a content stream.
    Op(Vec<u8>),
}

type Dict = Vec<(Vec<u8>, Obj)>;

fn get<'a>(dict: &'a Dict, key: &[u8]) -> Option<&'a Obj> {
    dict.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\n' | b'\r' | b'\t' | b'\x0c' | b'\0')
}

fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Reads objects from bytes.
struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Moves on `n` bytes, never past the end.
    fn skip(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.data.len());
    }

    fn skip_space(&mut self) {
        while let Some(b) = self.peek() {
            if is_space(b) {
                self.skip(1);
            } else if b == b'%' {
                while let Some(b) = self.peek() {
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                    self.skip(1);
                }
            } else {
                break;
            }
        }
    }

    fn word(&mut self) -> &'a [u8] {
        self.pos = self.pos.min(self.data.len());
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_space(b) || is_delimiter(b) {
                break;
            }
            self.skip(1);
        }
        &self.data[start..self.pos]
    }

    fn literal(&mut self) -> Vec<u8> {
        // After the opening parenthesis.
        let mut out = Vec::new();
        let mut depth = 1;
        while let Some(b) = self.peek() {
            self.skip(1);
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(b);
                }
                b'\\' => {
                    let Some(e) = self.peek() else { break };
                    self.skip(1);
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.skip(1);
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut v = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + u32::from(d - b'0');
                                        self.skip(1);
                                    }
                                    _ => break,
                                }
                            }
                            out.push((v & 0xFF) as u8);
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(b),
            }
        }
        out
    }

    fn hex(&mut self) -> Vec<u8> {
        // After the opening angle bracket.
        let mut digits = Vec::new();
        while let Some(b) = self.peek() {
            self.skip(1);
            if b == b'>' {
                break;
            }
            if let Some(d) = (b as char).to_digit(16) {
                digits.push(d as u8);
            }
        }
        if digits.len() % 2 == 1 {
            digits.push(0);
        }
        digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect()
    }

    fn name(&mut self) -> Vec<u8> {
        // After the slash.
        let raw = self.word();
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'#'
                && let Some(v) = raw
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
            } else {
                out.push(raw[i]);
                i += 1;
            }
        }
        out
    }

    /// The next object; `None` at the end, or on `>>` and `]`.
    fn object(&mut self, depth: usize) -> Option<Obj> {
        self.skip_space();
        let b = self.peek()?;
        if depth > MAX_DEPTH {
            // Nested too deep: a byte at a time, without going deeper.
            self.skip(1);
            return Some(Obj::Null);
        }
        match b {
            b'/' => {
                self.skip(1);
                Some(Obj::Name(self.name()))
            }
            b'(' => {
                self.skip(1);
                Some(Obj::Str(self.literal()))
            }
            b'<' if self.data.get(self.pos + 1) == Some(&b'<') => {
                self.skip(2);
                let mut dict = Vec::new();
                loop {
                    self.skip_space();
                    match self.peek() {
                        None => break,
                        Some(b'>') => {
                            self.skip(2);
                            break;
                        }
                        Some(b'/') => {
                            self.skip(1);
                            let key = self.name();
                            let value = if depth < MAX_DEPTH {
                                self.object(depth + 1).unwrap_or(Obj::Null)
                            } else {
                                Obj::Null
                            };
                            dict.push((key, value));
                        }
                        Some(_) => {
                            // Not a key: skipped.
                            if self.object(depth + 1).is_none() {
                                self.skip(1);
                            }
                        }
                    }
                }
                Some(Obj::Dict(dict))
            }
            b'<' => {
                self.skip(1);
                Some(Obj::Str(self.hex()))
            }
            b'>' => {
                self.skip(2);
                None
            }
            b'[' => {
                self.skip(1);
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    match self.peek() {
                        None => break,
                        Some(b']') => {
                            self.skip(1);
                            break;
                        }
                        Some(_) => match self.object(depth + 1) {
                            Some(o) if depth < MAX_DEPTH => items.push(o),
                            Some(_) => {}
                            None => self.skip(1),
                        },
                    }
                }
                Some(Obj::Array(items))
            }
            b']' | b')' | b'{' | b'}' => {
                self.skip(1);
                None
            }
            _ => {
                let word = self.word();
                if word.is_empty() {
                    self.skip(1);
                    return Some(Obj::Null);
                }
                Some(match word {
                    b"true" => Obj::Bool(true),
                    b"false" => Obj::Bool(false),
                    b"null" => Obj::Null,
                    w if w[0].is_ascii_digit() || matches!(w[0], b'+' | b'-' | b'.') => {
                        let n = std::str::from_utf8(w)
                            .ok()
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        // `12 0 R`: a reference.
                        let save = self.pos;
                        if w.iter().all(u8::is_ascii_digit) {
                            self.skip_space();
                            let generation = self.word();
                            if !generation.is_empty() && generation.iter().all(u8::is_ascii_digit) {
                                self.skip_space();
                                if self.word() == b"R" {
                                    return Some(Obj::Ref(n as u32));
                                }
                            }
                            self.pos = save;
                        }
                        Obj::Num(n)
                    }
                    w => Obj::Op(w.to_vec()),
                })
            }
        }
    }
}

/// An object of the file, and its stream's raw bytes when it has one.
#[derive(Debug, Clone)]
struct Entry {
    obj: Obj,
    stream: Option<Vec<u8>>,
}

struct Document<'a> {
    data: &'a [u8],
    /// Where each object's body starts, the last definition winning.
    offsets: HashMap<u32, usize>,
    /// Objects found in object streams.
    packed: HashMap<u32, Obj>,
    cache: HashMap<u32, Option<Entry>>,
}

/// Where `N G obj` keywords are, by object number.
fn scan(data: &[u8]) -> HashMap<u32, usize> {
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if &data[i..i + 3] == b"obj"
            && data
                .get(i + 3)
                .is_none_or(|b| is_space(*b) || is_delimiter(*b))
            && i > 0
            && is_space(data[i - 1])
        {
            // Back over `N G `.
            let mut j = i - 1;
            while j > 0 && is_space(data[j]) {
                j -= 1;
            }
            let gen_end = j + 1;
            while j > 0 && data[j].is_ascii_digit() {
                j -= 1;
            }
            let generation = &data[j + 1..gen_end];
            let mut k = j;
            while k > 0 && is_space(data[k]) {
                k -= 1;
            }
            let num_end = k + 1;
            while k > 0 && data[k].is_ascii_digit() {
                k -= 1;
            }
            let start = if data[k].is_ascii_digit() { k } else { k + 1 };
            let number = &data[start..num_end];
            if !generation.is_empty()
                && !number.is_empty()
                && number.iter().all(u8::is_ascii_digit)
                && let Some(n) = std::str::from_utf8(number)
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
            {
                out.insert(n, i + 3);
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

/// Inflates what a stream holds, by its filters; `None` for a filter not
/// read here.
fn decode(dict: &Dict, raw: &[u8]) -> Option<Vec<u8>> {
    let filters: Vec<&[u8]> = match get(dict, b"Filter") {
        None => Vec::new(),
        Some(Obj::Name(n)) => vec![n.as_slice()],
        Some(Obj::Array(a)) => a
            .iter()
            .filter_map(|f| match f {
                Obj::Name(n) => Some(n.as_slice()),
                _ => None,
            })
            .collect(),
        Some(_) => return None,
    };
    let mut data = raw.to_vec();
    for filter in filters {
        data = match filter {
            b"FlateDecode" | b"Fl" => inflate(&data)?,
            _ => return None,
        };
    }
    Some(data)
}

/// Zlib data inflated, as much as reads: a broken end keeps the start.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut reader = flate2::read::ZlibDecoder::new(data).take(MAX_STREAM);
    let mut buf = [0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(_) if !out.is_empty() => break,
            Err(_) => return None,
        }
    }
    Some(out)
}

impl<'a> Document<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut doc = Self {
            data,
            offsets: scan(data),
            packed: HashMap::new(),
            cache: HashMap::new(),
        };
        doc.unpack();
        doc
    }

    /// Opens the object streams.
    fn unpack(&mut self) {
        let numbers: Vec<u32> = self.offsets.keys().copied().collect();
        for n in numbers {
            let Some(Entry {
                obj: Obj::Dict(dict),
                stream: Some(raw),
            }) = self.entry(n)
            else {
                continue;
            };
            if get(&dict, b"Type") != Some(&Obj::Name(b"ObjStm".to_vec())) {
                continue;
            }
            let Some(body) = decode(&dict, &raw) else {
                continue;
            };
            let count = self.number(get(&dict, b"N")).unwrap_or(0.0) as usize;
            let first = self.number(get(&dict, b"First")).unwrap_or(0.0) as usize;
            let mut lexer = Lexer::new(&body, 0);
            let mut pairs = Vec::new();
            for _ in 0..count.min(100_000) {
                match (lexer.object(0), lexer.object(0)) {
                    (Some(Obj::Num(n)), Some(Obj::Num(at))) => pairs.push((n as u32, at as usize)),
                    _ => break,
                }
            }
            for (number, at) in pairs {
                if self.offsets.contains_key(&number) && !self.packed.contains_key(&number) {
                    // Defined in the file itself too: that one is kept.
                    continue;
                }
                let mut lexer = Lexer::new(&body, first + at);
                if let Some(obj) = lexer.object(0) {
                    self.packed.insert(number, obj);
                }
            }
        }
    }

    fn number(&mut self, obj: Option<&Obj>) -> Option<f64> {
        match obj? {
            Obj::Num(n) => Some(*n),
            Obj::Ref(r) => match self.entry(*r)?.obj {
                Obj::Num(n) => Some(n),
                _ => None,
            },
            _ => None,
        }
    }

    fn entry(&mut self, number: u32) -> Option<Entry> {
        if let Some(cached) = self.cache.get(&number) {
            return cached.clone();
        }
        let entry = self.read_entry(number);
        self.cache.insert(number, entry.clone());
        entry
    }

    fn read_entry(&mut self, number: u32) -> Option<Entry> {
        if let Some(&at) = self.offsets.get(&number) {
            let mut lexer = Lexer::new(self.data, at);
            let obj = lexer.object(0)?;
            lexer.skip_space();
            let rest = self.data.get(lexer.pos..).unwrap_or_default();
            let stream = if rest.starts_with(b"stream") {
                let mut start = lexer.pos + 6;
                if self.data.get(start) == Some(&b'\r') {
                    start += 1;
                }
                if self.data.get(start) == Some(&b'\n') {
                    start += 1;
                }
                let length = match &obj {
                    Obj::Dict(d) => match get(d, b"Length") {
                        Some(Obj::Num(n)) => Some(*n as usize),
                        // An indirect length: found by `endstream`.
                        _ => None,
                    },
                    _ => None,
                };
                let by_length = length.and_then(|l| start.checked_add(l)).filter(|end| {
                    self.data.get(*end..).is_some_and(|after| {
                        let after = &after[..after.len().min(32)];
                        after.windows(9).any(|w| w == b"endstream")
                    })
                });
                let end = by_length.or_else(|| {
                    self.data
                        .get(start..)?
                        .windows(9)
                        .position(|w| w == b"endstream")
                        .map(|p| {
                            let mut end = start + p;
                            // The end of line before `endstream`.
                            if end > start && self.data[end - 1] == b'\n' {
                                end -= 1;
                            }
                            if end > start && self.data[end - 1] == b'\r' {
                                end -= 1;
                            }
                            end
                        })
                })?;
                Some(self.data.get(start..end)?.to_vec())
            } else {
                None
            };
            return Some(Entry { obj, stream });
        }
        self.packed.get(&number).map(|obj| Entry {
            obj: obj.clone(),
            stream: None,
        })
    }

    /// `obj`, through a reference.
    fn resolve(&mut self, obj: &Obj) -> Obj {
        let mut obj = obj.clone();
        for _ in 0..8 {
            match obj {
                Obj::Ref(r) => match self.entry(r) {
                    Some(e) => obj = e.obj,
                    None => return Obj::Null,
                },
                other => return other,
            }
        }
        Obj::Null
    }

    fn dict(&mut self, obj: Option<&Obj>) -> Dict {
        match obj.map(|o| self.resolve(o)) {
            Some(Obj::Dict(d)) => d,
            _ => Vec::new(),
        }
    }

    /// A stream's bytes, decoded.
    fn stream(&mut self, obj: &Obj) -> Option<(Dict, Vec<u8>)> {
        let Obj::Ref(r) = obj else { return None };
        let Entry {
            obj: Obj::Dict(dict),
            stream: Some(raw),
        } = self.entry(*r)?
        else {
            return None;
        };
        let data = decode(&dict, &raw)?;
        Some((dict, data))
    }

    /// The pages in order, each with its resources (inherited from the
    /// tree when it has none).
    fn pages(&mut self) -> Vec<(Dict, Dict)> {
        let mut root = None;
        let mut numbers: Vec<u32> = self
            .offsets
            .keys()
            .chain(self.packed.keys())
            .copied()
            .collect();
        numbers.sort_unstable();
        numbers.dedup();
        for &n in &numbers {
            if let Some(Entry {
                obj: Obj::Dict(d), ..
            }) = self.entry(n)
                && get(&d, b"Type") == Some(&Obj::Name(b"Catalog".to_vec()))
            {
                root = get(&d, b"Pages").cloned();
            }
        }
        let mut out = Vec::new();
        let mut seen = Vec::new();
        if let Some(pages) = root {
            self.walk_pages(&pages, &Vec::new(), &mut out, &mut seen, 0);
        }
        if out.is_empty() {
            // No catalog found: every page, by number.
            for n in numbers {
                if let Some(Entry {
                    obj: Obj::Dict(d), ..
                }) = self.entry(n)
                    && get(&d, b"Type") == Some(&Obj::Name(b"Page".to_vec()))
                {
                    let resources = self.dict(get(&d, b"Resources"));
                    out.push((d, resources));
                }
            }
        }
        out
    }

    fn walk_pages(
        &mut self,
        node: &Obj,
        inherited: &Dict,
        out: &mut Vec<(Dict, Dict)>,
        seen: &mut Vec<u32>,
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        if let Obj::Ref(r) = node {
            if seen.contains(r) {
                return;
            }
            seen.push(*r);
        }
        let Obj::Dict(d) = self.resolve(node) else {
            return;
        };
        let resources = match get(&d, b"Resources") {
            Some(r) => self.dict(Some(r)),
            None => inherited.clone(),
        };
        match get(&d, b"Kids").map(|k| self.resolve(k)) {
            Some(Obj::Array(kids)) => {
                for kid in kids {
                    self.walk_pages(&kid, &resources, out, seen, depth + 1);
                }
            }
            _ => out.push((d, resources)),
        }
    }
}

/// How a font's codes become text.
#[derive(Debug, Clone, Default)]
struct Font {
    /// From its ToUnicode map.
    map: HashMap<u32, String>,
    /// Bytes per code: 1, or 2 for composite fonts.
    width: usize,
    /// For a simple font without a map: its encoding's differences.
    differences: HashMap<u8, char>,
    /// A composite font without a map: its codes say nothing.
    opaque: bool,
}

impl Font {
    fn decode(&self, bytes: &[u8], out: &mut String) {
        if !self.map.is_empty() {
            for code in bytes.chunks(self.width.max(1)) {
                let code = code.iter().fold(0u32, |v, b| v << 8 | u32::from(*b));
                if let Some(text) = self.map.get(&code) {
                    out.push_str(text);
                } else if self.width == 1 {
                    out.push(latin(code as u8));
                }
            }
        } else if !self.opaque {
            for b in bytes {
                out.push(
                    self.differences
                        .get(b)
                        .copied()
                        .unwrap_or_else(|| latin(*b)),
                );
            }
        }
    }
}

/// A byte of WinAnsi (close to Windows-1252) as a character.
fn latin(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', ' ', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', ' ', 'Ž', ' ', ' ', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', ' ', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        b if b < 0x20 => ' ',
        b => char::from(b),
    }
}

/// A glyph name of an encoding's differences, as text.
fn glyph(name: &[u8]) -> Option<char> {
    let name = std::str::from_utf8(name).ok()?;
    if name.len() == 1 {
        return name.chars().next();
    }
    if let Some(hex) = name.strip_prefix("uni").filter(|h| h.len() == 4) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    Some(match name {
        "space" => ' ',
        "hyphen" | "minus" => '-',
        "period" => '.',
        "comma" => ',',
        "colon" => ':',
        "semicolon" => ';',
        "quoteright" | "quotesingle" => '\'',
        "parenleft" => '(',
        "parenright" => ')',
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        "eacute" => 'é',
        "egrave" => 'è',
        "ecircumflex" => 'ê',
        "edieresis" => 'ë',
        "agrave" => 'à',
        "acircumflex" => 'â',
        "ccedilla" => 'ç',
        "icircumflex" => 'î',
        "idieresis" => 'ï',
        "ocircumflex" => 'ô',
        "ugrave" => 'ù',
        "ucircumflex" => 'û',
        "udieresis" => 'ü',
        "Eacute" => 'É',
        "oe" => 'œ',
        "fi" => 'ﬁ',
        "fl" => 'ﬂ',
        _ => return None,
    })
}

/// A ToUnicode CMap: codes to text, and the codes' width.
fn cmap(data: &[u8]) -> (HashMap<u32, String>, usize) {
    let utf16 = |bytes: &[u8]| -> String {
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|p| u16::from_be_bytes([p[0], *p.get(1).unwrap_or(&0)]))
            .collect();
        String::from_utf16_lossy(&units)
    };
    let code = |bytes: &[u8]| bytes.iter().fold(0u32, |v, b| v << 8 | u32::from(*b));
    let mut map = HashMap::new();
    let mut width = 0;
    let mut lexer = Lexer::new(data, 0);
    let mut stack: Vec<Obj> = Vec::new();
    while let Some(obj) = lexer.object(0).or_else(|| {
        (lexer.pos < data.len()).then(|| {
            lexer.pos += 1;
            Obj::Null
        })
    }) {
        match obj {
            Obj::Op(op) => {
                match op.as_slice() {
                    b"endcodespacerange" => {
                        for o in &stack {
                            if let Obj::Str(s) = o {
                                width = width.max(s.len());
                            }
                        }
                    }
                    b"endbfchar" => {
                        for pair in stack.chunks(2) {
                            if let [Obj::Str(src), Obj::Str(dst)] = pair {
                                map.insert(code(src), utf16(dst));
                            }
                        }
                    }
                    b"endbfrange" => {
                        for triple in stack.chunks(3) {
                            match triple {
                                [Obj::Str(lo), Obj::Str(hi), Obj::Str(dst)] => {
                                    let (lo, hi) = (code(lo), code(hi));
                                    let mut units: Vec<u16> = dst
                                        .chunks(2)
                                        .map(|p| {
                                            u16::from_be_bytes([p[0], *p.get(1).unwrap_or(&0)])
                                        })
                                        .collect();
                                    for c in lo..=hi.min(lo.saturating_add(0xFFFF)) {
                                        map.insert(c, String::from_utf16_lossy(&units));
                                        if let Some(last) = units.last_mut() {
                                            *last = last.wrapping_add(1);
                                        }
                                    }
                                }
                                [Obj::Str(lo), Obj::Str(_), Obj::Array(dsts)] => {
                                    let lo = code(lo);
                                    for (i, d) in dsts.iter().enumerate() {
                                        if let Obj::Str(d) = d {
                                            map.insert(lo + i as u32, utf16(d));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                stack.clear();
            }
            other => stack.push(other),
        }
        if map.len() > 1 << 20 {
            break;
        }
    }
    (map, if width == 0 { 1 } else { width })
}

/// Reads the fonts of `resources`.
fn fonts(doc: &mut Document<'_>, resources: &Dict) -> HashMap<Vec<u8>, Font> {
    let mut out = HashMap::new();
    let list = doc.dict(get(resources, b"Font"));
    for (name, reference) in list {
        let font = doc.dict(Some(&reference));
        let mut f = Font {
            width: 1,
            ..Font::default()
        };
        if let Some(to_unicode) = get(&font, b"ToUnicode").cloned()
            && let Some((_, data)) = doc.stream(&to_unicode)
        {
            let (map, width) = cmap(&data);
            f.map = map;
            f.width = width;
        }
        let composite = get(&font, b"Subtype") == Some(&Obj::Name(b"Type0".to_vec()));
        if composite && f.map.is_empty() {
            f.opaque = true;
        }
        if composite && f.width < 2 {
            f.width = 2;
        }
        if let Obj::Dict(encoding) = get(&font, b"Encoding").map_or(Obj::Null, |e| doc.resolve(e))
            && let Some(Obj::Array(differences)) = get(&encoding, b"Differences")
        {
            let mut code = 0u32;
            for d in differences {
                match d {
                    Obj::Num(n) => code = *n as u32,
                    Obj::Name(glyph_name) => {
                        if let (Ok(c), Some(ch)) = (u8::try_from(code), glyph(glyph_name)) {
                            f.differences.insert(c, ch);
                        }
                        code += 1;
                    }
                    _ => {}
                }
            }
        }
        out.insert(name, f);
    }
    out
}

/// Runs a content stream, adding its text to `out`.
fn run(doc: &mut Document<'_>, content: &[u8], resources: &Dict, out: &mut String, depth: usize) {
    if depth > 4 {
        return;
    }
    let fonts = fonts(doc, resources);
    let mut font: Option<&Font> = None;
    let mut operands: Vec<Obj> = Vec::new();
    let mut lexer = Lexer::new(content, 0);
    let show = |font: Option<&Font>, bytes: &[u8], out: &mut String| match font {
        Some(f) => f.decode(bytes, out),
        None => out.extend(bytes.iter().map(|b| latin(*b))),
    };
    let newline = |out: &mut String| {
        if !out.ends_with('\n') {
            out.push('\n');
        }
    };
    let space = |out: &mut String| {
        if !out.ends_with([' ', '\n']) && !out.is_empty() {
            out.push(' ');
        }
    };
    loop {
        let Some(obj) = lexer.object(0) else {
            if lexer.pos >= content.len() {
                break;
            }
            lexer.pos += 1;
            continue;
        };
        let Obj::Op(op) = obj else {
            operands.push(obj);
            if operands.len() > 64 {
                operands.remove(0);
            }
            continue;
        };
        match op.as_slice() {
            b"Tf" => {
                if let Some(Obj::Name(name)) = operands.first() {
                    font = fonts.get(name);
                }
            }
            b"Tj" => {
                if let Some(Obj::Str(s)) = operands.last() {
                    show(font, s, out);
                }
            }
            b"'" | b"\"" => {
                newline(out);
                if let Some(Obj::Str(s)) = operands.last() {
                    show(font, s, out);
                }
            }
            b"TJ" => {
                if let Some(Obj::Array(items)) = operands.last() {
                    for item in items {
                        match item {
                            Obj::Str(s) => show(font, s, out),
                            // A wide gap between glyphs: a space.
                            Obj::Num(n) if *n < -200.0 => space(out),
                            _ => {}
                        }
                    }
                }
            }
            b"Td" | b"TD" => match operands.as_slice() {
                [.., Obj::Num(_), Obj::Num(ty)] if *ty != 0.0 => newline(out),
                [.., Obj::Num(tx), Obj::Num(_)] if *tx != 0.0 => space(out),
                _ => {}
            },
            b"T*" | b"Tm" => newline(out),
            b"ET" => space(out),
            b"Do" => {
                if let Some(Obj::Name(name)) = operands.first() {
                    let xobjects = doc.dict(get(resources, b"XObject"));
                    if let Some(reference) = get(&xobjects, name).cloned()
                        && let Some((dict, data)) = doc.stream(&reference)
                        && get(&dict, b"Subtype") == Some(&Obj::Name(b"Form".to_vec()))
                    {
                        let own = match get(&dict, b"Resources") {
                            Some(r) => doc.dict(Some(r)),
                            None => resources.clone(),
                        };
                        run(doc, &data, &own, out, depth + 1);
                    }
                }
            }
            b"ID" => {
                // An inline image's bytes, up to `EI` alone.
                let rest = content.get(lexer.pos..).unwrap_or_default();
                let end = rest
                    .windows(4)
                    .position(|w| is_space(w[0]) && &w[1..3] == b"EI" && is_space(w[3]))
                    .map_or(rest.len(), |p| p + 3);
                lexer.pos += end;
            }
            _ => {}
        }
        operands.clear();
    }
}

/// The text of the PDF `data`, pages apart by a blank line; `None` when it
/// is not a PDF or is encrypted.
pub fn text(data: &[u8]) -> Option<String> {
    if !data.starts_with(b"%PDF") {
        return None;
    }
    // An encrypted file: its strings are noise without the key.
    if data.windows(8).any(|w| w == b"/Encrypt") {
        return None;
    }
    let mut doc = Document::new(data);
    let mut out = String::new();
    for (page, resources) in doc.pages() {
        let contents = match get(&page, b"Contents") {
            Some(Obj::Array(parts)) => parts.clone(),
            Some(other) => match doc.resolve(other) {
                Obj::Array(parts) => parts,
                _ => vec![other.clone()],
            },
            None => Vec::new(),
        };
        let mut content = Vec::new();
        for part in contents {
            if let Some((_, data)) = doc.stream(&part) {
                content.extend_from_slice(&data);
                content.push(b'\n');
            }
        }
        run(&mut doc, &content, &resources, &mut out, 0);
        out.push_str("\n\n");
    }
    Some(out)
}

/// A one-page PDF of `lines`, in Helvetica, its content deflated, for
/// tests; `objstm` puts the font and the page in an object stream, and
/// `hex` writes the text as hex strings mapped by a ToUnicode CMap.
#[cfg(test)]
pub fn write(lines: &[&str], objstm: bool, hex: bool) -> Vec<u8> {
    use std::io::Write;
    let deflate = |data: &[u8]| {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = e.write_all(data);
        e.finish().unwrap_or_default()
    };
    // Codes 1, 2, 3… for each character met, mapped back by the CMap.
    let mut codes: Vec<char> = Vec::new();
    let mut content = String::from("BT /F1 12 Tf 72 720 Td\n");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            content.push_str("0 -16 Td\n");
        }
        if hex {
            let mut s = String::from("<");
            for c in line.chars() {
                let n = match codes.iter().position(|k| *k == c) {
                    Some(p) => p + 1,
                    None => {
                        codes.push(c);
                        codes.len()
                    }
                };
                s.push_str(&format!("{n:04X}"));
            }
            s.push_str("> Tj\n");
            content.push_str(&s);
        } else {
            let words: Vec<String> = line.split(' ').map(|w| format!("({w})")).collect();
            content.push_str(&format!("[{}] TJ\n", words.join(" -250 ")));
        }
    }
    content.push_str("ET\n");
    let packed = deflate(content.as_bytes());
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
    );
    cmap.push_str(&format!("{} beginbfchar\n", codes.len()));
    for (i, c) in codes.iter().enumerate() {
        let mut units = [0u16; 2];
        let utf16: String = c
            .encode_utf16(&mut units)
            .iter()
            .map(|u| format!("{u:04X}"))
            .collect();
        cmap.push_str(&format!("<{:04X}> <{utf16}>\n", i + 1));
    }
    cmap.push_str("endbfchar\nendcmap\n");
    let font = if hex {
        "<< /Type /Font /Subtype /Type0 /BaseFont /Sans /Encoding /Identity-H /ToUnicode 6 0 R >>"
    } else {
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
    };
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>";
    let mut out: Vec<u8> = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let obj = |out: &mut Vec<u8>, n: u32, body: &[u8]| {
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, 2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    if objstm {
        let header = format!("3 0 4 {} ", page.len() + 1);
        let body = format!("{header}{page}\n{font}");
        let packed_objs = deflate(body.as_bytes());
        let mut stream = format!(
            "<< /Type /ObjStm /N 2 /First {} /Filter /FlateDecode /Length {} >>\nstream\n",
            header.len(),
            packed_objs.len()
        )
        .into_bytes();
        stream.extend_from_slice(&packed_objs);
        stream.extend_from_slice(b"\nendstream");
        obj(&mut out, 7, &stream);
    } else {
        obj(&mut out, 3, page.as_bytes());
        obj(&mut out, 4, font.as_bytes());
    }
    let mut stream = format!(
        "<< /Filter /FlateDecode /Length {} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    stream.extend_from_slice(&packed);
    stream.extend_from_slice(b"\nendstream");
    obj(&mut out, 5, &stream);
    let mut cmap_stream = format!("<< /Length {} >>\nstream\n", cmap.len()).into_bytes();
    cmap_stream.extend_from_slice(cmap.as_bytes());
    cmap_stream.extend_from_slice(b"\nendstream");
    obj(&mut out, 6, &cmap_stream);
    out.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn the_text_of_a_simple_page_reads_in_order() {
        let pdf = write(
            &["Quarterly bamboo report", "Pandas ate well."],
            false,
            false,
        );
        let text = text(&pdf).expect("a PDF");
        assert_eq!(words(&text), "Quarterly bamboo report Pandas ate well.");
        assert!(text.contains("report\n"), "{text:?}");
    }

    #[test]
    fn object_streams_and_cmaps_of_composite_fonts_are_read() {
        let pdf = write(&["Café crème", "naïve façade"], true, true);
        let text = text(&pdf).expect("a PDF");
        assert_eq!(words(&text), "Café crème naïve façade");
    }

    #[test]
    fn escapes_and_octal_codes_in_literal_strings() {
        let mut lexer = Lexer::new(br"(a\(b\) \101\n(c))", 0);
        assert_eq!(lexer.object(0), Some(Obj::Str(b"a(b) A\n(c)".to_vec())));
        let mut lexer = Lexer::new(b"<< /A 12 0 R /B [1 2.5 /N#20x] >>", 0);
        assert_eq!(
            lexer.object(0),
            Some(Obj::Dict(vec![
                (b"A".to_vec(), Obj::Ref(12)),
                (
                    b"B".to_vec(),
                    Obj::Array(vec![
                        Obj::Num(1.0),
                        Obj::Num(2.5),
                        Obj::Name(b"N x".to_vec())
                    ])
                ),
            ]))
        );
    }

    #[test]
    fn what_is_not_a_readable_pdf_gives_no_text() {
        assert_eq!(text(b"hello"), None);
        assert_eq!(text(b"%PDF-1.4\n1 0 obj << /Encrypt 2 0 R >> endobj"), None);
        // Cut in the middle: what reads, reads.
        let pdf = write(&["Bamboo"], false, false);
        let _ = text(&pdf[..pdf.len() / 2]);
        assert_eq!(text(b"%PDF-1.4\n").as_deref(), Some(""));
        // Nested without end: no stack overflow.
        let mut deep = b"%PDF-1.4\n1 0 obj << /Type /Page /Contents ".to_vec();
        deep.extend(std::iter::repeat_n(b'[', 200_000));
        deep.extend_from_slice(b" >> endobj\n");
        deep.extend(std::iter::repeat_n(b'<', 200_000));
        let _ = text(&deep);
    }
}
