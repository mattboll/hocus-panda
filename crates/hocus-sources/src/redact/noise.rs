//! A mail's noise, removed before any model (§14): the quoted reply and
//! what follows its header, the signature, "Envoyé de mon iPhone",
//! unsubscribe footers, and tracking links (their parameters dropped, a
//! long redirect named by its host).

use std::sync::LazyLock;

use regex::Regex;

/// The line that opens a quoted reply: "Le jeu. 5 nov. 2026 à 10:12, Léa
/// <lea@…> a écrit :", "On Thu, Nov 5, 2026 at 10:12 AM Léa wrote:",
/// Outlook's "-----Original Message-----" or its "De : … Envoyé : …".
static REPLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^(?:>?\s*le\s.{4,200}?\sa\s+écrit\s*:\s*$|>?\s*on\s.{4,200}?\swrote:\s*$|-{2,}\s*(?:original\s+message|message\s+d['’]origine|forwarded\s+message|message\s+transféré)\s*-{2,}\s*$|_{20,}\s*$|(?:de|from)\s*:\s.*\n(?:.*\n){0,2}?(?:envoyé|sent|date)\s*:\s)",
    )
    .expect("a regex")
});

static FOOTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:envoyé\s+(?:de|depuis)\s+mon\s+\w+|sent\s+from\s+my\s+\w+|get\s+outlook\s+for|se\s+désabonner|se\s+désinscrire|désinscription|désabonnement|unsubscribe|voir\s+la\s+version\s+en\s+ligne|view\s+(?:this\s+email\s+)?in\s+(?:your\s+)?browser|vous\s+recevez\s+ce\s+(?:mail|message|courriel)|you\s+are\s+receiving\s+this)",
    )
    .expect("a regex")
});

static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bhttps?://[^\s<>"')\]]+"#).expect("a regex"));

/// The parameters that only track.
const TRACKING: &[&str] = &[
    "utm_", "mc_cid", "mc_eid", "fbclid", "gclid", "dclid", "msclkid", "_hsenc", "_hsmi",
    "mkt_tok", "trk", "tracking", "oly_", "vero_", "ss_",
];

/// A link longer than this, through a redirector, is named by its host.
const LONG: usize = 120;

/// `body` without its noise.
pub fn denoise(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    // Everything from the reply's header on is the previous message.
    let mut text = match REPLY.find(&body) {
        Some(m) => body[..m.start()].to_owned(),
        None => body.clone(),
    };
    // The signature: after "-- " on a line of its own.
    if let Some(at) = text.find("\n-- \n").or_else(|| text.find("\n--\n")) {
        text.truncate(at);
    }
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .filter(|line| !FOOTER.is_match(line))
        .collect();
    let mut text = kept.join("\n");
    text = LINK
        .replace_all(&text, |c: &regex::Captures<'_>| link(&c[0]))
        .into_owned();
    // Blank lines left in a row.
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_owned()
}

/// A link without its tracking parameters; a long one by its host only.
fn link(url: &str) -> String {
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let p = p.to_ascii_lowercase();
            !TRACKING.iter().any(|t| p.starts_with(t))
        })
        .collect();
    let url = if kept.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{}", kept.join("&"))
    };
    if url.len() > LONG {
        let host = base
            .split("://")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .unwrap_or("");
        format!("[lien : {host}]")
    } else {
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quoted_reply_and_the_signature_go() {
        let body = "Oui, jeudi me va.\n\n-- \nLéa Martin\n06 12 34 56 78\n";
        assert_eq!(denoise(body), "Oui, jeudi me va.");
        let body =
            "Parfait.\n\nLe jeu. 5 nov. 2026 à 10:12, Léa <lea@example.org> a écrit :\n> Jeudi ?\n";
        assert_eq!(denoise(body), "Parfait.");
        let body = "Ok\n\nEnvoyé de mon iPhone";
        assert_eq!(denoise(body), "Ok");
    }

    #[test]
    fn tracking_parameters_go() {
        assert_eq!(
            denoise("Voir https://example.org/a?id=3&utm_source=mail&utm_medium=x"),
            "Voir https://example.org/a?id=3"
        );
    }
}
