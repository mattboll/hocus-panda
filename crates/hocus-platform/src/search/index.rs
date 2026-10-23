//! The words index: for each word (lower case, without accents), the
//! documents holding it and how often; ranked by BM25, the last word of a
//! query taken as the start of one (it is still being typed). With a
//! model that embeds, each document has a few vectors too, and both
//! rankings are fused (reciprocal rank fusion).
//!
//! Kept in one file of the state directory, readable by the user only:
//! for each document its path, date, size, words and counts, the start of
//! its text (for snippets) and its vectors.

use std::{
    collections::{BTreeMap, HashMap},
    io::{self, Read, Write},
    ops::Bound,
    path::{Path, PathBuf},
};

/// The start of a document's text kept for snippets.
const HEAD: usize = 16 * 1024;
/// Words longer than this are not words.
const MAX_WORD: usize = 40;
/// How many words a prefix stands for, at most.
const MAX_EXPANSIONS: usize = 64;
/// Reciprocal rank fusion's constant.
const RRF_K: f32 = 60.0;
/// How close a document's meaning must be to the query's to count.
pub const MIN_SIMILARITY: f32 = 0.45;
const MAGIC: &[u8; 8] = b"HPSRCH01";

/// A word without its case and accents, as the index keeps it.
pub fn fold(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        for c in c.to_lowercase() {
            match c {
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
                'ç' => out.push('c'),
                'è' | 'é' | 'ê' | 'ë' => out.push('e'),
                'ì' | 'í' | 'î' | 'ï' => out.push('i'),
                'ñ' => out.push('n'),
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => out.push('o'),
                'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
                'ý' | 'ÿ' => out.push('y'),
                'œ' => out.push_str("oe"),
                'æ' => out.push_str("ae"),
                'ß' => out.push_str("ss"),
                c => out.push(c),
            }
        }
    }
    out
}

/// The words of `text` with where each is, folded.
pub fn words(text: &str) -> impl Iterator<Item = (usize, usize, String)> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (start, c) = chars.next()?;
            if !c.is_alphanumeric() {
                continue;
            }
            let mut end = start + c.len_utf8();
            while let Some(&(i, c)) = chars.peek() {
                if !c.is_alphanumeric() {
                    break;
                }
                end = i + c.len_utf8();
                chars.next();
            }
            let word = &text[start..end];
            if word.len() <= MAX_WORD {
                return Some((start, end, fold(word)));
            }
        }
    })
}

/// A document as the index keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub path: PathBuf,
    /// Its modification time, nanoseconds since the epoch, and size: a
    /// change of either reads it again.
    pub modified: i128,
    pub size: u64,
    /// The start of its text.
    pub head: String,
    /// Its words, by id, and how often each comes.
    terms: Vec<(u32, u16)>,
    /// How many words it has.
    length: u32,
    /// Unit vectors of the start of its text, quantized.
    pub vectors: Vec<Vec<i8>>,
}

impl Doc {
    #[cfg(test)]
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// A part of a snippet, matched by the query or not.
pub type Piece = (String, bool);

/// A document found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    /// A line around the first words matched, those marked.
    pub snippet: Vec<Piece>,
}

#[derive(Debug, Default)]
pub struct Index {
    docs: Vec<Option<Doc>>,
    free: Vec<usize>,
    by_path: HashMap<PathBuf, usize>,
    /// Words to their ids, in order for prefixes.
    words: BTreeMap<String, u32>,
    /// By word id: the documents holding it, and how often.
    postings: Vec<Vec<(u32, u16)>>,
    total_length: u64,
    /// The model the vectors come from; vectors of another are dropped.
    pub model: String,
}

impl Index {
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn get(&self, path: &Path) -> Option<&Doc> {
        self.docs.get(*self.by_path.get(path)?)?.as_ref()
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.by_path.keys().cloned().collect()
    }

    /// How many documents have vectors of the model.
    pub fn embedded(&self) -> usize {
        self.docs
            .iter()
            .flatten()
            .filter(|d| !d.vectors.is_empty())
            .count()
    }

    /// Documents without vectors yet, with their text's start.
    pub fn unembedded(&self, at_most: usize) -> Vec<(PathBuf, String)> {
        self.docs
            .iter()
            .flatten()
            .filter(|d| d.vectors.is_empty() && !d.head.trim().is_empty())
            .take(at_most)
            .map(|d| (d.path.clone(), d.head.clone()))
            .collect()
    }

    pub fn set_vectors(&mut self, path: &Path, vectors: &[Vec<f32>]) {
        if let Some(&slot) = self.by_path.get(path)
            && let Some(Some(doc)) = self.docs.get_mut(slot)
        {
            doc.vectors = vectors.iter().map(|v| quantize(v)).collect();
        }
    }

    /// Vectors of another model go.
    pub fn set_model(&mut self, model: &str) {
        if self.model != model {
            self.model = model.to_owned();
            for doc in self.docs.iter_mut().flatten() {
                doc.vectors.clear();
            }
        }
    }

    fn word_id(&mut self, word: &str) -> u32 {
        if let Some(&id) = self.words.get(word) {
            return id;
        }
        let id = self.postings.len() as u32;
        self.words.insert(word.to_owned(), id);
        self.postings.push(Vec::new());
        id
    }

    /// Adds or replaces a document: its name's words count three times.
    pub fn insert(&mut self, path: PathBuf, modified: i128, size: u64, text: &str) {
        let mut counts: HashMap<String, u32> = HashMap::new();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
        for (_, _, w) in words(stem) {
            *counts.entry(w).or_default() += 3;
        }
        let mut length = 0u32;
        for (_, _, w) in words(text) {
            *counts.entry(w).or_default() += 1;
            length = length.saturating_add(1);
        }
        let terms: Vec<(String, u16)> = counts
            .into_iter()
            .map(|(w, n)| (w, n.min(u32::from(u16::MAX)) as u16))
            .collect();
        let mut head = text;
        if head.len() > HEAD {
            let mut end = HEAD;
            while !head.is_char_boundary(end) {
                end -= 1;
            }
            head = &head[..end];
        }
        self.insert_terms(
            path,
            modified,
            size,
            head.to_owned(),
            terms,
            length,
            Vec::new(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_terms(
        &mut self,
        path: PathBuf,
        modified: i128,
        size: u64,
        head: String,
        terms: Vec<(String, u16)>,
        length: u32,
        vectors: Vec<Vec<i8>>,
    ) {
        self.remove(&path);
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.docs.push(None);
                self.docs.len() - 1
            }
        };
        let ids: Vec<(u32, u16)> = terms
            .into_iter()
            .map(|(w, n)| (self.word_id(&w), n))
            .collect();
        for (id, n) in &ids {
            if let Some(list) = self.postings.get_mut(*id as usize) {
                list.push((slot as u32, *n));
            }
        }
        self.total_length += u64::from(length);
        self.by_path.insert(path.clone(), slot);
        self.docs[slot] = Some(Doc {
            path,
            modified,
            size,
            head,
            terms: ids,
            length,
            vectors,
        });
    }

    pub fn remove(&mut self, path: &Path) {
        let Some(slot) = self.by_path.remove(path) else {
            return;
        };
        if let Some(doc) = self.docs.get_mut(slot).and_then(Option::take) {
            for (id, _) in &doc.terms {
                if let Some(list) = self.postings.get_mut(*id as usize) {
                    list.retain(|(d, _)| *d as usize != slot);
                }
            }
            self.total_length = self.total_length.saturating_sub(u64::from(doc.length));
        }
        self.free.push(slot);
    }

    /// The word ids the query's words stand for: each word itself, the
    /// last one also as the start of longer ones. Each comes with a
    /// weight: a whole word counts more than one it starts.
    fn expand(&self, query: &str) -> Vec<Vec<(u32, f32)>> {
        let mut terms: Vec<String> = words(query).map(|(_, _, w)| w).collect();
        // A last letter alone says nothing yet.
        if terms.len() > 1 && terms.last().is_some_and(|w| w.chars().count() < 2) {
            terms.pop();
        }
        let last = terms.len().saturating_sub(1);
        terms
            .iter()
            .enumerate()
            .map(|(i, term)| {
                if i == last && term.chars().count() >= 2 {
                    self.words
                        .range::<str, _>((Bound::Included(term.as_str()), Bound::Unbounded))
                        .take_while(|(w, _)| w.starts_with(term.as_str()))
                        .take(MAX_EXPANSIONS)
                        .map(|(w, id)| (*id, if w == term { 1.0 } else { 0.7 }))
                        .collect()
                } else {
                    self.words
                        .get(term)
                        .map(|id| vec![(*id, 1.0)])
                        .unwrap_or_default()
                }
            })
            .collect()
    }

    /// Documents holding every word of `query`, best first (BM25).
    pub fn by_words(&self, query: &str, at_most: usize) -> Vec<usize> {
        let expanded = self.expand(query);
        if expanded.is_empty() || expanded.iter().any(Vec::is_empty) {
            return Vec::new();
        }
        let n = self.len().max(1) as f32;
        let average = (self.total_length as f32 / n).max(1.0);
        let mut scores: HashMap<u32, (f32, usize)> = HashMap::new();
        for (i, ids) in expanded.iter().enumerate() {
            let mut best: HashMap<u32, f32> = HashMap::new();
            for (id, weight) in ids {
                let Some(list) = self.postings.get(*id as usize) else {
                    continue;
                };
                let df = list.len() as f32;
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                for (doc, tf) in list {
                    let length = self.docs[*doc as usize]
                        .as_ref()
                        .map_or(1.0, |d| d.length as f32);
                    let tf = f32::from(*tf);
                    let score =
                        weight * idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * length / average));
                    let slot = best.entry(*doc).or_default();
                    *slot = slot.max(score);
                }
            }
            for (doc, score) in best {
                let entry = scores.entry(doc).or_default();
                // Every word must match: counted by the word's place.
                if entry.1 == i {
                    entry.0 += score;
                    entry.1 += 1;
                }
            }
        }
        let mut ranked: Vec<(u32, f32)> = scores
            .into_iter()
            .filter(|(_, (_, matched))| *matched == expanded.len())
            .map(|(doc, (score, _))| (doc, score))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(at_most)
            .map(|(d, _)| d as usize)
            .collect()
    }

    /// Documents closest in meaning to the unit vector `query`, best
    /// first, those close enough only.
    pub fn by_meaning(&self, query: &[f32], at_most: usize) -> Vec<usize> {
        let mut ranked: Vec<(usize, f32)> = self
            .docs
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let d = d.as_ref()?;
                let best = d
                    .vectors
                    .iter()
                    .map(|v| similarity(query, v))
                    .fold(f32::MIN, f32::max);
                (best >= MIN_SIMILARITY).then_some((i, best))
            })
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.into_iter().take(at_most).map(|(i, _)| i).collect()
    }

    /// The documents found for `query`: by words, and by meaning when a
    /// vector of it is given, fused.
    pub fn search(&self, query: &str, vector: Option<&[f32]>, at_most: usize) -> Vec<Found> {
        let by_words = self.by_words(query, 50);
        let by_meaning = vector.map(|v| self.by_meaning(v, 20)).unwrap_or_default();
        let mut fused: HashMap<usize, f32> = HashMap::new();
        for list in [&by_words, &by_meaning] {
            for (rank, doc) in list.iter().enumerate() {
                *fused.entry(*doc).or_default() += 1.0 / (RRF_K + rank as f32 + 1.0);
            }
        }
        let mut ranked: Vec<(usize, f32)> = fused.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let terms: Vec<String> = words(query).map(|(_, _, w)| w).collect();
        ranked
            .into_iter()
            .take(at_most)
            .filter_map(|(slot, _)| {
                let doc = self.docs.get(slot)?.as_ref()?;
                Some(Found {
                    path: doc.path.clone(),
                    snippet: snippet(&doc.head, &terms),
                })
            })
            .collect()
    }

    /// Writes the index to `to`.
    pub fn save(&self, to: &mut impl Write) -> io::Result<()> {
        let names: HashMap<u32, &str> =
            self.words.iter().map(|(w, id)| (*id, w.as_str())).collect();
        to.write_all(MAGIC)?;
        put_str(to, &self.model)?;
        put_u32(to, self.len() as u32)?;
        for doc in self.docs.iter().flatten() {
            put_bytes(to, path_bytes(&doc.path))?;
            to.write_all(&doc.modified.to_le_bytes())?;
            to.write_all(&doc.size.to_le_bytes())?;
            put_str(to, &doc.head)?;
            put_u32(to, doc.length)?;
            put_u32(to, doc.terms.len() as u32)?;
            for (id, n) in &doc.terms {
                put_str(to, names.get(id).copied().unwrap_or_default())?;
                to.write_all(&n.to_le_bytes())?;
            }
            put_u32(to, doc.vectors.len() as u32)?;
            for v in &doc.vectors {
                put_u32(to, v.len() as u32)?;
                to.write_all(&v.iter().map(|x| *x as u8).collect::<Vec<u8>>())?;
            }
        }
        Ok(())
    }

    /// Reads an index written by [`Index::save`]; documents `keep` refuses
    /// are left out.
    pub fn load(from: &mut impl Read, keep: impl Fn(&Path) -> bool) -> io::Result<Self> {
        let mut magic = [0u8; 8];
        from.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not an index"));
        }
        let mut index = Self {
            model: get_str(from)?,
            ..Self::default()
        };
        let count = get_u32(from)?;
        for _ in 0..count {
            let path = bytes_path(get_bytes(from)?);
            let mut modified = [0u8; 16];
            from.read_exact(&mut modified)?;
            let mut size = [0u8; 8];
            from.read_exact(&mut size)?;
            let head = get_str(from)?;
            let length = get_u32(from)?;
            let n_terms = get_u32(from)?;
            let mut terms = Vec::with_capacity(n_terms.min(1 << 16) as usize);
            for _ in 0..n_terms {
                let word = get_str(from)?;
                let mut n = [0u8; 2];
                from.read_exact(&mut n)?;
                terms.push((word, u16::from_le_bytes(n)));
            }
            let n_vectors = get_u32(from)?;
            let mut vectors = Vec::new();
            for _ in 0..n_vectors {
                let dims = get_u32(from)? as usize;
                if dims > 1 << 16 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a vector too long",
                    ));
                }
                let mut v = vec![0u8; dims];
                from.read_exact(&mut v)?;
                vectors.push(v.into_iter().map(|x| x as i8).collect());
            }
            if keep(&path) {
                index.insert_terms(
                    path,
                    i128::from_le_bytes(modified),
                    u64::from_le_bytes(size),
                    head,
                    terms,
                    length,
                    vectors,
                );
            }
        }
        Ok(index)
    }
}

/// A unit vector in bytes: each part times 127.
fn quantize(v: &[f32]) -> Vec<i8> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return vec![0; v.len()];
    }
    v.iter()
        .map(|x| (x / norm * 127.0).round().clamp(-127.0, 127.0) as i8)
        .collect()
}

/// The cosine of a unit vector and a quantized one.
fn similarity(query: &[f32], v: &[i8]) -> f32 {
    if query.len() != v.len() {
        return f32::MIN;
    }
    let dot: f32 = query.iter().zip(v).map(|(q, x)| q * f32::from(*x)).sum();
    let norm = v.iter().map(|x| f32::from(*x).powi(2)).sum::<f32>().sqrt();
    if norm == 0.0 { 0.0 } else { dot / norm }
}

/// Makes `v` a unit vector.
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
}

/// About this many characters of `text` around the first word `terms`
/// match (the last as a start), the words matched marked; the start of
/// the text when none is.
pub fn snippet(text: &str, terms: &[String]) -> Vec<Piece> {
    const BEFORE: usize = 30;
    const LENGTH: usize = 110;
    let matches = |w: &str| {
        terms.iter().enumerate().any(|(i, t)| {
            w == t || (i + 1 == terms.len() && t.chars().count() >= 2 && w.starts_with(t.as_str()))
        })
    };
    let all: Vec<(usize, usize, bool)> = words(text)
        .take(20_000)
        .map(|(s, e, w)| (s, e, matches(&w)))
        .collect();
    let first = all.iter().position(|(_, _, m)| *m);
    let start = match first {
        Some(i) => {
            let at = all[i].0;
            let beginning = all.first().map_or(0, |(s, _, _)| *s);
            if at - beginning <= LENGTH / 2 {
                // Near the beginning: from there.
                beginning
            } else {
                // Back to a word start about BEFORE characters earlier.
                all[..i]
                    .iter()
                    .rev()
                    .take_while(|(s, _, _)| at - s <= BEFORE)
                    .last()
                    .map_or(at, |(s, _, _)| *s)
            }
        }
        None => all.first().map_or(0, |(s, _, _)| *s),
    };
    let mut end = text.len().min(start + LENGTH);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    // Not in the middle of a word.
    if let Some((s, e, _)) = all.iter().find(|(s, e, _)| *s < end && *e > end) {
        end = if *s > start { *s } else { *e };
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut push = |text: &str, matched: bool| {
        // Runs of spaces and lines as one space.
        let mut clean = String::with_capacity(text.len());
        let mut space = false;
        for c in text.chars() {
            if c.is_whitespace() {
                space = true;
            } else {
                if space {
                    clean.push(' ');
                    space = false;
                }
                clean.push(c);
            }
        }
        if space {
            clean.push(' ');
        }
        match pieces.last_mut() {
            Some((last, m)) if *m == matched => last.push_str(&clean),
            _ => pieces.push((clean, matched)),
        }
    };
    // Words before: the line starts in the middle.
    if all.first().is_some_and(|(s, _, _)| *s < start) {
        push("… ", false);
    }
    let shown: Vec<&(usize, usize, bool)> = all
        .iter()
        .filter(|(s, e, _)| *s >= start && *e <= end)
        .collect();
    let mut at = start;
    for (i, (s, e, m)) in shown.iter().enumerate() {
        if *s > at {
            // Between two words matched, the gap is part of the match.
            let joined = *m && i > 0 && shown[i - 1].2;
            push(&text[at..*s], joined);
        }
        push(&text[*s..*e], *m);
        at = *e;
    }
    let gap = &text[at..end.max(at)];
    if end < text.trim_end().len() {
        push(if gap.trim().is_empty() { " " } else { gap }, false);
        let spaced = gap.trim().is_empty() || gap.ends_with(char::is_whitespace);
        push(if spaced { "…" } else { " …" }, false);
    } else {
        push(gap, false);
    }
    // No space at the very start or end.
    if let Some((first, _)) = pieces.first_mut() {
        *first = first.trim_start().to_owned();
    }
    if let Some((last, _)) = pieces.last_mut() {
        *last = last.trim_end().to_owned();
    }
    pieces.retain(|(t, _)| !t.is_empty());
    pieces
}

fn put_u32(to: &mut impl Write, n: u32) -> io::Result<()> {
    to.write_all(&n.to_le_bytes())
}

fn put_bytes(to: &mut impl Write, b: &[u8]) -> io::Result<()> {
    put_u32(to, b.len() as u32)?;
    to.write_all(b)
}

fn put_str(to: &mut impl Write, s: &str) -> io::Result<()> {
    put_bytes(to, s.as_bytes())
}

fn get_u32(from: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    from.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn get_bytes(from: &mut impl Read) -> io::Result<Vec<u8>> {
    let len = get_u32(from)? as usize;
    if len > 64 << 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a field too long",
        ));
    }
    let mut b = vec![0u8; len];
    from.read_exact(&mut b)?;
    Ok(b)
}

fn get_str(from: &mut impl Read) -> io::Result<String> {
    String::from_utf8(get_bytes(from)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn path_bytes(path: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes()
}

fn bytes_path(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        let mut index = Index::default();
        index.insert(
            "/d/Bamboo plan.md".into(),
            1,
            10,
            "Grow bamboo near the river. Pandas eat bamboo every day.",
        );
        index.insert(
            "/d/budget.txt".into(),
            1,
            10,
            "The quarterly budget for the café: tea, cakes and bamboo shoots.",
        );
        index.insert("/d/other.txt".into(), 1, 10, "Nothing to see here.");
        index
    }

    fn names(index: &Index, found: &[Found]) -> Vec<String> {
        found
            .iter()
            .filter_map(|f| index.get(&f.path).map(Doc::name))
            .collect()
    }

    #[test]
    fn words_fold_case_and_accents() {
        let w: Vec<String> = words("Café-Crème, NAÏVE œuvre 42")
            .map(|(_, _, w)| w)
            .collect();
        assert_eq!(w, ["cafe", "creme", "naive", "oeuvre", "42"]);
    }

    #[test]
    fn every_word_must_match_and_the_last_may_be_a_start() {
        let index = index();
        let found = index.search("bamboo", None, 5);
        assert_eq!(names(&index, &found), ["Bamboo plan.md", "budget.txt"]);
        let found = index.search("cafe bamb", None, 5);
        assert_eq!(names(&index, &found), ["budget.txt"]);
        assert!(index.search("bamboo zebra", None, 5).is_empty());
        assert!(
            index.search("b", None, 5).is_empty(),
            "one letter is too little"
        );
    }

    #[test]
    fn the_snippet_marks_the_words_matched() {
        let index = index();
        let found = index.search("quarterly bud", None, 5);
        assert_eq!(
            found[0].snippet,
            vec![
                ("The ".into(), false),
                ("quarterly budget".into(), true),
                (" for the café: tea, cakes and bamboo shoots.".into(), false),
            ]
        );
        let long = format!("{} needle {}", "hay ".repeat(40), "hay ".repeat(40));
        let pieces = snippet(&long, &["needle".into()]);
        let text: String = pieces.iter().map(|(t, _)| t.as_str()).collect();
        assert!(text.starts_with("… hay"), "{text}");
        assert!(text.ends_with("hay …"), "{text}");
        assert!(pieces.contains(&("needle".into(), true)), "{pieces:?}");
    }

    #[test]
    fn a_document_read_again_replaces_its_words() {
        let mut index = index();
        index.insert("/d/budget.txt".into(), 2, 10, "Now about zebras.");
        assert_eq!(
            names(&index, &index.search("bamboo", None, 5)),
            ["Bamboo plan.md"]
        );
        assert_eq!(
            names(&index, &index.search("zebras", None, 5)),
            ["budget.txt"]
        );
        index.remove(Path::new("/d/budget.txt"));
        assert!(index.search("zebras", None, 5).is_empty());
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn meaning_and_words_are_fused() {
        let mut index = index();
        index.set_model("m");
        index.set_vectors(Path::new("/d/other.txt"), &[vec![1.0, 0.0]]);
        index.set_vectors(Path::new("/d/budget.txt"), &[vec![0.0, 1.0]]);
        // Only "other" is close in meaning; "bamboo" matches two by words.
        let found = index.search("bamboo", Some(&[1.0, 0.0]), 5);
        assert_eq!(
            names(&index, &found),
            ["Bamboo plan.md", "other.txt", "budget.txt"]
        );
        let found = index.search("zzz", Some(&[1.0, 0.0]), 5);
        assert_eq!(names(&index, &found), ["other.txt"]);
        assert_eq!(index.embedded(), 2);
        index.set_model("another");
        assert_eq!(index.embedded(), 0, "another model's vectors go");
    }

    #[test]
    fn it_reads_back_what_it_wrote() -> io::Result<()> {
        let mut index = index();
        index.set_model("m");
        index.set_vectors(Path::new("/d/other.txt"), &[vec![0.6, 0.8]]);
        let mut file = Vec::new();
        index.save(&mut file)?;
        let back = Index::load(&mut file.as_slice(), |p| !p.ends_with("budget.txt"))?;
        assert_eq!(back.len(), 2, "what is refused is left out");
        assert_eq!(back.model, "m");
        assert_eq!(
            names(&back, &back.search("bamboo", None, 5)),
            ["Bamboo plan.md"]
        );
        assert_eq!(
            back.get(Path::new("/d/other.txt")).map(|d| d.vectors.len()),
            Some(1)
        );
        assert!(Index::load(&mut &b"garbage"[..], |_| true).is_err());
        assert!(Index::load(&mut &file[..file.len() / 2], |_| true).is_err());
        Ok(())
    }
}
