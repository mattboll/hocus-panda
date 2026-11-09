//! Contacts' birthdays and anniversaries, nothing else of the cards
//! (§10.1), over CardDAV with Online Accounts' token.
//!
//! Why CardDAV and not the People API: the token Online Accounts hands is
//! for GNOME's own OAuth client, whose scopes include CardDAV's
//! (`https://www.googleapis.com/auth/carddav`, since GNOME 3.10; without
//! it Google answers `401`, not `400`); Evolution reads Google
//! contacts with it over CardDAV. Whether the People API is enabled for
//! that client is not ours to know, so CardDAV it is.
//!
//! Google's CardDAV implements only some reports
//! (developers.google.com/people/carddav): `addressbook-multiget` and
//! `sync-collection`, not `addressbook-query`, which it answers `400`
//! (#754: what a Google Workspace account said). So, as its guide does
//! it: a `PROPFIND` of depth 1 on the default address book lists its
//! cards (`getetag`), then `addressbook-multiget` `REPORT`s ask for them,
//! a hundred at a time, whole (`address-data` with no `prop` in it: a
//! server that would not sort the properties would refuse those too).
//! Each card's `FN`, `N`, `BDAY`, `ANNIVERSARY` and Apple's dated labels
//! (`X-ABDATE` with `X-ABLabel` `_$!<Anniversary>!$_`) are read; the rest
//! is dropped as it is read. Read once a day: birthdays change rarely.

use std::time::Duration;

use hocus_day::google::AccessToken;
use quick_xml::events::Event as Xml;

use crate::{
    Result, SourceError,
    consent::Grant,
    item::{Anniversary, Birthday, Content, Item, Pointer},
};

/// Google's CardDAV.
pub const API: &str = "https://www.googleapis.com/carddav/v1";
/// For tests only: CardDAV at an address on this machine.
pub const API_ENV: &str = "HOCUS_TEST_CARDDAV";

pub fn api(test: Option<&str>) -> String {
    match test {
        Some(test) if crate::http::local(test) => test.trim_end_matches('/').to_owned(),
        _ => API.to_owned(),
    }
}

/// The address book's cards, by their `href` and `getetag`.
pub const LIST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:getetag/>
    <D:resourcetype/>
  </D:prop>
</D:propfind>"#;

/// How many cards one `addressbook-multiget` asks for.
const BATCH: usize = 100;

/// The `addressbook-multiget` body for `hrefs`, their cards whole.
pub fn multiget(hrefs: &[String]) -> String {
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?>
<C:addressbook-multiget xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:carddav">
  <D:prop>
    <D:getetag/>
    <C:address-data/>
  </D:prop>
"#,
    );
    for href in hrefs {
        let href = href
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        body.push_str(&format!("  <D:href>{href}</D:href>\n"));
    }
    body.push_str("</C:addressbook-multiget>");
    body
}

/// One WebDAV request with the account's token, asked again once on a
/// `401`: the answer's text.
fn ask(
    agent: &ureq::Agent,
    tokens: &mut dyn AccessToken,
    method: &str,
    url: &str,
    depth: &str,
    body: &str,
) -> Result<String> {
    let mut asked_again = false;
    loop {
        let (token, _) = tokens.get()?;
        let request = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("Authorization", format!("Bearer {}", token.expose()))
            .header("Depth", depth)
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body)
            .map_err(|err| SourceError::Invalid(err.to_string()))?;
        let (status, text) = crate::http::read(agent.run(request), "CardDAV", 32 * 1024 * 1024)?;
        match status {
            207 | 200 => return Ok(text),
            401 if !asked_again => asked_again = true,
            401 => {
                return Err(SourceError::SignIn(
                    "CardDAV refuses the account's token".to_owned(),
                ));
            }
            429 | 500..=599 => {
                return Err(SourceError::Offline(format!("CardDAV answers {status}")));
            }
            _ => {
                return Err(SourceError::Refused(format!(
                    "CardDAV answers {status} to {method}"
                )));
            }
        }
    }
}

/// The path of `url` (`https://host/a/b/` → `/a/b/`).
fn path_of(url: &str) -> &str {
    url.split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|at| &rest[at..]))
        .unwrap_or(url)
}

/// `grant`'s account's birthdays, from CardDAV at `api`.
pub fn read(
    grant: &Grant,
    tokens: &mut dyn AccessToken,
    api: &str,
    today: jiff::civil::Date,
) -> Result<Vec<Item>> {
    let agent = crate::http::agent(Duration::from_secs(60));
    let url = format!(
        "{api}/principals/{}/lists/default/",
        crate::mail::segment(grant.identity())
    );
    let listed = ask(&agent, tokens, "PROPFIND", &url, "1", LIST)?;
    let book = path_of(&url).trim_end_matches('/').to_owned();
    // Its cards: every response but the address book's own.
    let hrefs: Vec<String> = responses(&listed)
        .into_iter()
        .map(|(href, _)| href)
        .filter(|href| {
            !href.is_empty() && !href.ends_with('/') && path_of(href).trim_end_matches('/') != book
        })
        .collect();
    let mut cards = Vec::new();
    for batch in hrefs.chunks(BATCH) {
        let body = ask(&agent, tokens, "REPORT", &url, "1", &multiget(batch))?;
        cards.extend(multistatus(&body));
    }
    let mut items = Vec::new();
    for (href, card) in cards {
        for (i, birthday) in birthdays(&card).into_iter().enumerate() {
            let at = birthday
                .next(today)
                .and_then(|(date, _)| date.to_zoned(jiff::tz::TimeZone::UTC).ok())
                .map_or(jiff::Timestamp::UNIX_EPOCH, |z| z.timestamp());
            items.push(Item {
                pointer: Pointer {
                    source: grant.source().clone(),
                    id: format!("{href}#{i}"),
                    label: format!("la fiche de {}", birthday.name),
                    open: None,
                },
                notebook: grant.notebook(),
                at,
                content: Content::Birthday(birthday),
            });
        }
    }
    Ok(items)
}

/// Each response's `href` and `address-data`, those with a card.
pub fn multistatus(xml: &str) -> Vec<(String, String)> {
    responses(xml)
        .into_iter()
        .filter(|(_, data)| !data.is_empty())
        .collect()
}

/// Each response's `href` and `address-data` (empty without one).
pub fn responses(xml: &str) -> Vec<(String, String)> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    let (mut href, mut data) = (String::new(), String::new());
    let mut inside: Option<&'static str> = None;
    loop {
        match reader.read_event() {
            Ok(Xml::Start(e)) => match e.local_name().as_ref() {
                b"response" => {
                    href.clear();
                    data.clear();
                }
                b"href" => inside = Some("href"),
                b"address-data" => inside = Some("data"),
                _ => {}
            },
            Ok(Xml::End(e)) => match e.local_name().as_ref() {
                b"href" | b"address-data" => inside = None,
                b"response" => {
                    out.push((href.trim().to_owned(), std::mem::take(&mut data)));
                }
                _ => {}
            },
            Ok(Xml::Text(t)) => {
                if let Ok(text) = t.xml10_content() {
                    push(inside, &text, &mut href, &mut data);
                }
            }
            Ok(Xml::CData(t)) => {
                if let Ok(text) = t.decode() {
                    push(inside, &text, &mut href, &mut data);
                }
            }
            Ok(Xml::GeneralRef(r)) => {
                let text = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => match r.decode().as_deref() {
                        Ok("amp") => "&".to_owned(),
                        Ok("lt") => "<".to_owned(),
                        Ok("gt") => ">".to_owned(),
                        Ok("quot") => "\"".to_owned(),
                        Ok("apos") => "'".to_owned(),
                        _ => String::new(),
                    },
                };
                push(inside, &text, &mut href, &mut data);
            }
            Ok(Xml::Eof) => break,
            Err(err) => {
                tracing::info!(%err, "CardDAV's answer does not read past here");
                break;
            }
            Ok(_) => {}
        }
    }
    out
}

fn push(inside: Option<&str>, text: &str, href: &mut String, data: &mut String) {
    match inside {
        Some("href") => href.push_str(text),
        Some("data") => data.push_str(text),
        _ => {}
    }
}

/// A card's birthdays and anniversaries.
pub fn birthdays(card: &str) -> Vec<Birthday> {
    // Lines folded on the next line's leading space are one.
    let mut lines: Vec<String> = Vec::new();
    for line in card.replace("\r\n", "\n").split('\n') {
        match line.strip_prefix([' ', '\t']) {
            Some(rest) if !lines.is_empty() => {
                if let Some(last) = lines.last_mut() {
                    last.push_str(rest);
                }
            }
            _ => lines.push(line.to_owned()),
        }
    }
    let mut name = String::new();
    let mut structured = String::new();
    let mut dates: Vec<(Anniversary, String)> = Vec::new();
    let mut labelled: Vec<(String, String)> = Vec::new();
    let mut labels: Vec<(String, String)> = Vec::new();
    for line in &lines {
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let (group, property) = match head.split(';').next().unwrap_or(head).split_once('.') {
            Some((group, property)) => (group.to_owned(), property.to_ascii_uppercase()),
            None => (
                String::new(),
                head.split(';').next().unwrap_or(head).to_ascii_uppercase(),
            ),
        };
        match property.as_str() {
            "FN" => name = value.trim().replace("\\,", ","),
            "N" => structured = value.to_owned(),
            "BDAY" => dates.push((Anniversary::Birthday, value.to_owned())),
            "ANNIVERSARY" | "X-ANNIVERSARY" => {
                dates.push((Anniversary::Anniversary, value.to_owned()));
            }
            "X-ABDATE" => labelled.push((group, value.to_owned())),
            "X-ABLABEL" => labels.push((group, value.to_owned())),
            _ => {}
        }
    }
    for (group, value) in labelled {
        let anniversary = labels
            .iter()
            .any(|(g, label)| *g == group && label.to_ascii_lowercase().contains("anniversary"));
        if anniversary {
            dates.push((Anniversary::Anniversary, value));
        }
    }
    if name.is_empty() {
        // "Martin;Claire;;;" → "Claire Martin".
        let parts: Vec<&str> = structured.split(';').collect();
        name = [parts.get(1), parts.first()]
            .into_iter()
            .flatten()
            .filter(|p| !p.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
    }
    if name.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Birthday> = dates
        .into_iter()
        .filter_map(|(kind, value)| {
            let (year, month, day) = date(&value)?;
            Some(Birthday {
                name: name.clone(),
                kind,
                month,
                day,
                year,
            })
        })
        .collect();
    out.dedup();
    out
}

/// `1985-04-12`, `19850412`, `--0412`, `--04-12`, a time after a `T`;
/// Apple's 1604 says "no year".
pub fn date(value: &str) -> Option<(Option<i16>, i8, i8)> {
    let value = value.trim();
    let value = value.split('T').next().unwrap_or(value);
    let (year, rest) = if let Some(rest) = value.strip_prefix("--") {
        (None, rest.replace('-', ""))
    } else {
        let digits = value.replace('-', "");
        if digits.len() != 8 {
            return None;
        }
        let year: i16 = digits[..4].parse().ok()?;
        (
            Some(year).filter(|y| *y != 1604 && *y > 1000),
            digits[4..].to_owned(),
        )
    };
    if rest.len() != 4 {
        return None;
    }
    let month: i8 = rest[..2].parse().ok()?;
    let day: i8 = rest[2..].parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((year, month, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn birthdays_and_anniversaries_are_read_and_the_rest_dropped() {
        let card = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Claire Martin\r\nTEL:+33 6 12 34 56 78\r\nBDAY:1990-03-02\r\nitem1.X-ABDATE:2015-06-20\r\nitem1.X-ABLabel:_$!<Anniversary>!$_\r\nNOTE:mot de passe du wifi : chienchat\r\nEND:VCARD\r\n";
        let found = birthdays(card);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "Claire Martin");
        assert_eq!(
            (found[0].year, found[0].month, found[0].day),
            (Some(1990), 3, 2)
        );
        assert_eq!(found[1].kind, Anniversary::Anniversary);
    }

    #[test]
    fn dates_without_a_year() {
        assert_eq!(date("--0412"), Some((None, 4, 12)));
        assert_eq!(date("--04-12"), Some((None, 4, 12)));
        assert_eq!(date("1604-04-12"), Some((None, 4, 12)));
        assert_eq!(date("19850412"), Some((Some(1985), 4, 12)));
        assert_eq!(date("demain"), None);
    }
}
