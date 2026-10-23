//! The text of a document, by its kind: plain text and Markdown, HTML
//! (without its markup, scripts and styles), PDF (its text layer),
//! OpenDocument and OOXML office files (XML in a ZIP), and pictures in
//! the screenshots folder through the OCR program.

use std::{io::Read, path::Path, process::Command};

use quick_xml::{Reader, events::Event as Xml};

use super::{pdf, zip};

/// Bigger files are left out.
pub const MAX_FILE: u64 = 64 << 20;
/// What an office file's XML may inflate to.
const MAX_XML: usize = 32 << 20;
/// The text kept for the index, at most.
pub const MAX_TEXT: usize = 1 << 20;

/// What a file is, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Html,
    Pdf,
    /// OpenDocument: text, spreadsheet, presentation.
    OpenDocument,
    Docx,
    Xlsx,
    Pptx,
    /// A picture, read by OCR (in the screenshots folder only).
    Picture,
}

impl Format {
    /// From a file name's extension; `None` for what is not searched.
    pub fn of(name: &str) -> Option<Self> {
        let (_, extension) = name.rsplit_once('.')?;
        Some(match extension.to_lowercase().as_str() {
            "txt" | "text" | "md" | "markdown" | "org" | "rst" | "adoc" | "tex" | "csv" => {
                Self::Text
            }
            "html" | "htm" | "xhtml" => Self::Html,
            "pdf" => Self::Pdf,
            "odt" | "ods" | "odp" | "fodt" => Self::OpenDocument,
            "docx" => Self::Docx,
            "xlsx" => Self::Xlsx,
            "pptx" => Self::Pptx,
            "png" | "jpg" | "jpeg" | "webp" => Self::Picture,
            _ => return None,
        })
    }
}

/// The text of a file read whole into `data`.
pub fn text(format: Format, data: &[u8]) -> Option<String> {
    let mut text = match format {
        Format::Text => plain(data)?,
        Format::Html => html(&String::from_utf8_lossy(data)),
        Format::Pdf => pdf::text(data)?,
        Format::OpenDocument if data.starts_with(b"<?xml") => xml_text(data, &[]),
        Format::OpenDocument => office(data, |name| name == "content.xml")?,
        Format::Docx => office(data, |name| {
            name == "word/document.xml"
                || (name.starts_with("word/header") || name.starts_with("word/footer"))
                    && name.ends_with(".xml")
        })?,
        Format::Xlsx => office(data, |name| name == "xl/sharedStrings.xml")?,
        Format::Pptx => office(data, |name| {
            name.starts_with("ppt/slides/slide") && name.ends_with(".xml")
        })?,
        Format::Picture => return None,
    };
    if text.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Some(text)
}

/// Text, unless it looks binary.
fn plain(data: &[u8]) -> Option<String> {
    let head = &data[..data.len().min(8192)];
    if head.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(data).into_owned())
}

/// The words of an HTML page: the title and the body's text, without
/// tags, comments, scripts and styles; entities decoded.
pub fn html(source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(lt) = rest.find('<') {
        out.push_str(&entities(&rest[..lt]));
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        let Some(gt) = rest.find('>') else {
            rest = "";
            break;
        };
        let closing = rest[1..gt].starts_with('/');
        let tag = rest[1..gt].trim_start_matches('/');
        let name: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        rest = &rest[gt + 1..];
        if (name == "script" || name == "style") && !closing {
            let close = format!("</{name}");
            rest = find_ignoring_case(rest, &close).map_or("", |end| &rest[end..]);
            continue;
        }
        if matches!(
            name.as_str(),
            "p" | "br" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "title"
        ) {
            out.push('\n');
        } else {
            out.push(' ');
        }
    }
    out.push_str(&entities(rest));
    out
}

/// Where `needle` (ASCII) first is in `hay`, case aside.
fn find_ignoring_case(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// HTML's character references decoded: the common names and numbers.
fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let end = rest.bytes().take(12).position(|b| b == b';');
        let decoded = end.and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                "eacute" => 'é',
                "egrave" => 'è',
                "ecirc" => 'ê',
                "agrave" => 'à',
                "ccedil" => 'ç',
                "rsquo" | "lsquo" => '\'',
                "ldquo" | "rdquo" => '"',
                "hellip" => '…',
                "mdash" => '—',
                "ndash" => '–',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let value = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(value)?
                }
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text of the entries of an office file whose names `wanted` takes,
/// in the archive's order.
fn office(data: &[u8], wanted: impl Fn(&str) -> bool) -> Option<String> {
    let entries = zip::entries(data)?;
    let mut parts: Vec<&zip::Entry> = entries.iter().filter(|e| wanted(&e.name)).collect();
    // Slides by number: slide2 before slide10.
    parts.sort_by_key(|e| {
        let digits: String = e.name.chars().filter(char::is_ascii_digit).collect();
        (digits.len(), digits)
    });
    if parts.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut budget = MAX_XML;
    for entry in parts {
        let xml = zip::read(data, entry, budget)?;
        budget = budget.saturating_sub(xml.len());
        out.push_str(&xml_text(&xml, &["instrText", "delText"]));
        out.push('\n');
    }
    Some(out)
}

/// The text nodes of an office XML part, paragraphs on lines of their
/// own; the elements of `skipped` left out (field codes, deletions).
fn xml_text(xml: &[u8], skipped: &[&str]) -> String {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = String::new();
    let mut skipping = 0usize;
    let separator = |name: &[u8], out: &mut String| match name {
        b"p" | b"h" | b"br" | b"line-break" | b"tr" | b"si" => out.push('\n'),
        b"tab" | b"s" | b"tc" | b"table-cell" | b"c" => out.push(' '),
        _ => {}
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Xml::Start(e)) => {
                let name = e.local_name();
                if skipped.iter().any(|s| s.as_bytes() == name.as_ref()) {
                    skipping += 1;
                }
            }
            Ok(Xml::End(e)) => {
                let name = e.local_name();
                if skipped.iter().any(|s| s.as_bytes() == name.as_ref()) {
                    skipping = skipping.saturating_sub(1);
                }
                separator(name.as_ref(), &mut out);
            }
            Ok(Xml::Empty(e)) => separator(e.local_name().as_ref(), &mut out),
            Ok(Xml::Text(t)) if skipping == 0 => {
                if let Ok(text) = t.xml10_content() {
                    out.push_str(&text);
                }
            }
            Ok(Xml::GeneralRef(r)) if skipping == 0 => {
                let name = String::from_utf8_lossy(&r).into_owned();
                match name.as_str() {
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "amp" => out.push('&'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    _ => {
                        if let Ok(Some(c)) = r.resolve_char_ref() {
                            out.push(c);
                        }
                    }
                }
            }
            Ok(Xml::CData(t)) if skipping == 0 => out.push_str(&String::from_utf8_lossy(&t)),
            Ok(Xml::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
        if out.len() > MAX_TEXT {
            break;
        }
    }
    out
}

/// The text in a picture, through tesseract or what replaces it
/// (`PROGRAM IMAGE -` prints it); `None` when there is none.
pub fn ocr(program: &str, path: &Path) -> Option<String> {
    let mut words = program.split_whitespace();
    let mut command = Command::new(words.next()?);
    command.args(words);
    let out = command
        .arg(path)
        .arg("-")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// Reads what a file of `format` needs: the start of a text, a whole
/// PDF or office file (at most `MAX_FILE` bytes).
pub fn read_all(file: std::fs::File, format: Format) -> std::io::Result<Vec<u8>> {
    let limit = match format {
        Format::Text | Format::Html => 4 * MAX_TEXT as u64,
        _ => MAX_FILE,
    };
    let mut data = Vec::new();
    file.take(limit).read_to_end(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn formats_come_from_extensions() {
        assert_eq!(Format::of("notes.MD"), Some(Format::Text));
        assert_eq!(Format::of("Report.odt"), Some(Format::OpenDocument));
        assert_eq!(Format::of("a.tar.gz"), None);
        assert_eq!(Format::of("README"), None);
    }

    #[test]
    fn html_loses_its_markup_scripts_and_styles() {
        let page = "<html><head><title>Bamboo &amp; tea</title><style>p{color:red}</style>\
            <script>var x = '<p>no</p>';</script></head><body><!-- hidden -->\
            <p>Caf&eacute; at&nbsp;9&#58;30</p><p>Panda&#x27;s day</p></body></html>";
        assert_eq!(words(&html(page)), "Bamboo & tea Café at 9:30 Panda's day");
    }

    #[test]
    fn office_files_give_their_paragraphs() {
        let odt = zip::write(
            &[
                ("mimetype", b"application/vnd.oasis.opendocument.text"),
                (
                    "content.xml",
                    br#"<office:document-content xmlns:office="o" xmlns:text="t"><office:body><office:text><text:h>Bamboo plan</text:h><text:p>Grow<text:s/>it &amp; eat it</text:p></office:text></office:body></office:document-content>"#,
                ),
            ],
            true,
        );
        assert_eq!(
            text(Format::OpenDocument, &odt).as_deref().map(words),
            Some("Bamboo plan Grow it & eat it".into())
        );
        let docx = zip::write(
            &[(
                "word/document.xml",
                br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Quarterly</w:t></w:r><w:r><w:instrText>PAGE</w:instrText><w:t xml:space="preserve"> report</w:t></w:r></w:p><w:p><w:r><w:t>Done</w:t></w:r></w:p></w:body></w:document>"#,
            )],
            true,
        );
        let got = text(Format::Docx, &docx).expect("a docx");
        assert_eq!(words(&got), "Quarterly report Done");
        assert!(got.contains("report\n"), "{got:?}");
        let pptx = zip::write(
            &[
                (
                    "ppt/slides/slide10.xml",
                    br#"<p:sld xmlns:a="a" xmlns:p="p"><a:t>last</a:t></p:sld>"#,
                ),
                (
                    "ppt/slides/slide2.xml",
                    br#"<p:sld xmlns:a="a" xmlns:p="p"><a:t>first</a:t></p:sld>"#,
                ),
            ],
            false,
        );
        assert_eq!(
            text(Format::Pptx, &pptx).as_deref().map(words),
            Some("first last".into())
        );
        assert_eq!(text(Format::Docx, b"not a zip"), None);
    }

    #[test]
    fn binary_files_are_not_text() {
        assert_eq!(text(Format::Text, b"\x7fELF\0\0\0"), None);
        assert_eq!(
            text(Format::Text, "Pandas é".as_bytes()).as_deref(),
            Some("Pandas é")
        );
    }
}
