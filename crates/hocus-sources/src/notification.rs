//! An application's notification as one of her items (lot 7, §10.1): the
//! shell's server receives it, forwards it to her only when its
//! application is consented to, and she makes it an item here, to go
//! through the first pass and the triage like a mail.
//!
//! Nothing here, nor anywhere of hers, can act on a notification: its
//! actions are not even kept. Slack marks a channel read when one of its
//! notifications' actions (or its default action) is invoked; she reads
//! what it says and nothing more, and no request ever reaches Slack.

use jiff::{Timestamp, tz::TimeZone};

use crate::{
    consent::Grant,
    item::{Content, Item, Notification, Pointer},
    mail::short_date,
};

/// A notification of `grant`'s application as an item, its text not yet
/// redacted: `id` its id among hers (unique: the shell numbers its
/// notifications from 1 again when it starts), at `at`.
pub fn item(grant: &Grant, id: String, notification: Notification, at: Timestamp) -> Item {
    let day = short_date(at.to_zoned(TimeZone::system()).date());
    let app = if notification.app.trim().is_empty() {
        grant.identity().to_owned()
    } else {
        notification.app.trim().to_owned()
    };
    Item {
        pointer: Pointer {
            source: grant.source().clone(),
            id,
            label: format!("la notification de {app}, {day}"),
            // A notification opens nothing of hers: only its application,
            // which the user opens.
            open: None,
        },
        notebook: grant.notebook(),
        at,
        content: Content::Notification(Notification {
            app,
            ..notification
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Check, ConsentBook, Notebook, Outbound, Redactor, SourceKey};

    #[test]
    fn a_notification_is_redacted_like_a_mail_and_leaves_as_its_title_before_the_check() {
        let mut book = ConsentBook::default();
        book.allow(
            SourceKey::notifications("Slack", Some("slack")),
            "Slack",
            Notebook::Pro,
            Timestamp::UNIX_EPOCH,
        );
        let grant = book.notifications("Slack", Some("slack")).expect("granted");
        let at: Timestamp = "2026-11-09T09:12:00Z".parse().expect("a time");
        let redacted = Redactor::new().redact(item(
            &grant,
            "1794215520000-3".to_owned(),
            Notification {
                app: "Slack".to_owned(),
                title: "Karim (#prod-alertes)".to_owned(),
                body: "le token de la CI : ghp_0123456789abcdefghijABCDEFGHIJ012345, tu peux regarder ?"
                    .to_owned(),
            },
            at,
        ));
        assert_eq!(redacted.check, Check::FirstPass);
        assert_eq!(redacted.item.notebook, Notebook::Pro);
        assert_eq!(
            redacted.item.pointer.source.to_string(),
            "slack:notifications"
        );
        assert!(redacted.item.pointer.open.is_none());
        let Content::Notification(n) = &redacted.item.content else {
            panic!("a notification");
        };
        assert!(!n.body.contains("ghp_"), "{}", n.body);
        assert!(
            n.body
                .contains("[secret masqué — voir la notification de Slack, 9 nov.]"),
            "{}",
            n.body
        );
        match redacted.outbound() {
            Outbound::MetadataOnly { from, subject, .. } => {
                assert_eq!(from, Some("Slack"));
                assert_eq!(subject, "Karim (#prod-alertes)");
            }
            Outbound::Checked(_) => panic!("not checked yet"),
        }
    }
}
