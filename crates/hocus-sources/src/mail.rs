//! A mail read into an [`Item`]: its headers decoded, its text part (the
//! HTML one turned to text when it has no other), the names of its
//! attachments, never their content.

use jiff::{Timestamp, civil::Date};
use mail_parser::{AddressList, MessageParser};

use crate::{
    consent::Grant,
    item::{Content, Item, Mail, Pointer},
};

const MONTHS: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

/// "3 nov.", as a pointer says it.
pub fn short_date(date: Date) -> String {
    let month = MONTHS[usize::try_from(date.month() - 1).unwrap_or(0)];
    format!("{} {month}", date.day())
}

/// `text` as one segment of a URL's path.
pub fn segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~@".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn addresses(list: Option<AddressList<'_>>) -> Vec<String> {
    list.map(|list| {
        list.mailboxes()
            .map(|m| match (m.name(), m.address()) {
                (Some(name), Some(address)) => format!("{name} <{address}>"),
                (None, Some(address)) => address.to_owned(),
                (Some(name), None) => name.to_owned(),
                (None, None) => String::new(),
            })
            .filter(|a| !a.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Who a mail is from, as the user would say it ("Free", "Léa Martin").
fn sender_name(list: Option<AddressList<'_>>) -> String {
    list.and_then(|list| list.first())
        .and_then(|m| {
            m.name().map(str::to_owned).or_else(|| {
                m.address()
                    .map(|a| a.split('@').nth(1).unwrap_or(a).to_owned())
            })
        })
        .unwrap_or_else(|| "inconnu".to_owned())
}

/// Where a mail is, at its source.
#[derive(Debug, Clone)]
pub struct Place<'a> {
    pub mailbox: &'a str,
    pub sent: bool,
    pub uidvalidity: u32,
    pub uid: u32,
    /// Gmail's own id (`X-GM-MSGID`), for the link that opens it.
    pub gmail_id: Option<u64>,
    pub seen: bool,
    pub internal_date: Option<Timestamp>,
}

/// The mail in `raw` (its header only, or all of it), at `place`.
pub fn item(grant: &Grant, place: &Place<'_>, raw: &[u8], zone: &jiff::tz::TimeZone) -> Item {
    let parsed = MessageParser::default().parse(raw);
    let mut mail = Mail {
        mailbox: place.mailbox.to_owned(),
        sent: place.sent,
        seen: place.seen,
        ..Mail::default()
    };
    let mut at = place.internal_date;
    let mut sender = "inconnu".to_owned();
    if let Some(message) = &parsed {
        mail.from = addresses(message.from())
            .into_iter()
            .next()
            .unwrap_or_default();
        sender = sender_name(message.from());
        mail.to = addresses(message.to());
        mail.cc = addresses(message.cc());
        mail.subject = message.subject().unwrap_or_default().to_owned();
        mail.message_id = message.message_id().map(str::to_owned);
        mail.in_reply_to = message
            .in_reply_to()
            .and_then(|l| l.first().map(str::to_owned));
        if let Some(date) = message.date()
            && date.is_valid()
        {
            at = at.or_else(|| Timestamp::from_second(date.to_timestamp()).ok());
        }
        if message.text_body().len() > 0 || message.html_body().len() > 0 {
            mail.body = message.body_text(0).map(|b| b.into_owned());
        }
        mail.attachments = message
            .attachments()
            .map(|a| a.attachment_name().unwrap_or("pièce jointe").to_owned())
            .collect();
    }
    let at = at.unwrap_or(Timestamp::UNIX_EPOCH);
    let day = short_date(at.to_zoned(zone.clone()).date());
    let label = if place.sent {
        format!("ton mail envoyé du {day}")
    } else {
        format!("le mail de {sender}, {day}")
    };
    Item {
        pointer: Pointer {
            source: grant.source().clone(),
            id: format!("{}/{}/{}", place.mailbox, place.uidvalidity, place.uid),
            label,
            open: place.gmail_id.map(|id| {
                format!(
                    "https://mail.google.com/mail/u/{}/#all/{id:x}",
                    segment(grant.identity())
                )
            }),
        },
        notebook: grant.notebook(),
        at,
        content: Content::Mail(mail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_dates_are_french() {
        assert_eq!(short_date(jiff::civil::date(2026, 11, 3)), "3 nov.");
        assert_eq!(short_date(jiff::civil::date(2026, 2, 14)), "14 févr.");
    }
}
