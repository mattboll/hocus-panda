//! Values a label names, in French and English: "mot de passe : X",
//! "mdp du wifi : X", "le code du portail c'est 4821", "PIN 0912",
//! "482913 est votre code", "IBAN : …", "n° fiscal : …", and the lines
//! after "codes de récupération".
//!
//! A label, an optional qualifier ("du wifi", "de la carte", "of the
//! gate"), a separator, then the value. What the value must look like
//! depends on the label: a password can be any word; a code must hold a
//! digit (so "le code de la v2 est prêt" is not one); an identity number
//! must hold a digit and be long enough. A verbal separator ("est",
//! "c'est", "is") asks more of a password than a colon does, since
//! "le mot de passe est oublié" says nothing secret.

use std::sync::LazyLock;

use regex::Regex;

use super::{Found, SecretKind};

/// What a label's value must look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Any word (a password, a passphrase, a username).
    Word,
    /// A code: 3 to 12 characters holding a digit.
    Code,
    /// An identity number: 6 or more characters, digits among them,
    /// spaces between groups allowed.
    Number,
}

struct Label {
    regex: Regex,
    class: Class,
    kind: SecretKind,
    /// Only `:`, `=` or an arrow separate it from its value.
    strict: bool,
}

/// What may stand between a label and its value: a qualifier of up to four
/// words ("du wifi", "de la porte d'entrée"), then a separator.
const QUALIFIER: &str = r"(?:\s+(?:du|de\s+la|de\s+l['’]|des|de|d['’]|pour\s+(?:le|la|l['’])|of\s+the|of|for\s+the|for|to\s+the|chez|à\s+la|au)\s*[\p{L}\p{N}][\p{L}\p{N}'’\-]*(?:\s+[\p{L}\p{N}][\p{L}\p{N}'’\-]*){0,6}?)?";
/// Up to two words between a label and its separator, without a
/// preposition ("mdp Netflix :", "code Vélib' c'est", "mot de passe
/// temporaire est").
const BARE: &str = r"(?:\s+[\p{L}\p{N}][\p{L}\p{N}'’\-]*){1,2}?";
const STRICT: &str = r"\s*(?:[:=]|->|→|=>)\s*";
const LOOSE: &str = r"(?:\s*(?:[:=]|->|→|=>)\s*|\s*,?\s+(?:c['’]est|ce\s+sera|est|sont|reste|vaut|is|are|was|will\s+be)(?:\s+(?:toujours|encore|bien|maintenant|désormais|now|still))?\s*:?\s+)";

/// The words that, right after "code", say it is not a secret.
const NOT_A_SECRET_CODE: &str = r"postal|postale|promo|promotionnel|source|sources|client|article|produit|couleur|erreur|d['’]erreur|retour|http|de\s+la\s+route|de\s+conduite|du\s+travail|civil|pénal|iata|bic|swift|banque|guichet|insee|ape|naf|commune|pays|département|rome|réduction|de\s+réduction|avantage|parrainage|barre|barres|qr|review|freeze|couleur|cadeau|de\s+parrainage|de\s+la\s+consommation|des\s+impôts|naf|projet|du\s+projet|de\s+la\s+v\d|of\s+conduct|review|base|mort|morse|vestimentaire|génétique|de\s+suivi|tracking|de\s+réservation|booking|de\s+dossier|dossier|de\s+commande|commande|order|reference|référence";

fn label(words: &str, class: Class, kind: SecretKind, strict: bool) -> Label {
    let separator = if strict { STRICT } else { LOOSE };
    let pattern =
        format!(r"(?i)(?:^|[^\p{{L}}\p{{N}}_])(?P<label>{words})(?:{QUALIFIER}){separator}");
    labelled(&pattern, class, kind, strict)
}

/// A label followed by bare words before its separator.
fn bare(words: &str, class: Class, kind: SecretKind) -> Label {
    let pattern = format!(r"(?i)(?:^|[^\p{{L}}\p{{N}}_])(?P<label>{words}){BARE}{LOOSE}");
    labelled(&pattern, class, kind, false)
}

fn labelled(pattern: &str, class: Class, kind: SecretKind, strict: bool) -> Label {
    Label {
        regex: Regex::new(pattern).expect("a label's regex"),
        class,
        kind,
        strict,
    }
}

const PASSWORDS: &str = r"mots?\s+de\s+passe|mdp|passwords?|passwd|pwd|passphrase|clé\s+wpa|clé\s+wifi|clé\s+wi-fi|clé\s+de\s+sécurité(?:\s+réseau)?";
const CODES: &str = r"codes?(?:\s+pin)?|pin|digicode|combinaison|otp|passcode";

static LABELS: LazyLock<Vec<Label>> = LazyLock::new(|| {
    use Class::*;
    use SecretKind::*;
    vec![
        bare(PASSWORDS, Word, Labelled),
        bare(CODES, Code, Labelled),
        label(
            r"mots?\s+de\s+passe|mots?-de-passe|mdp|passwords?|passwd|pwd|passphrase|phrase\s+secrète|phrase\s+de\s+passe|mot\s+secret|clé\s+wpa|clé\s+wifi|clé\s+wi-fi|clé\s+du\s+wifi|clé\s+de\s+sécurité(?:\s+réseau)?|wifi\s+password|wi-fi\s+password|network\s+key",
            Word,
            Labelled,
            false,
        ),
        label(
            r"pass|login|identifiant|user(?:name)?|nom\s+d['’]utilisateur|clé|clef|key|secret|jeton|token",
            Word,
            Labelled,
            true,
        ),
        label(
            r"codes?(?:\s+pin)?(?:\s+(?:d['’]accès|d['’]entrée|secret|confidentiel|personnel|de\s+sécurité|de\s+vérification|de\s+confirmation|d['’]activation|de\s+validation|à\s+usage\s+unique|temporaire|wifi|porte|portail|alarme|immeuble))?",
            Code,
            Labelled,
            false,
        ),
        label(
            r"pin|digicode|combinaison|cryptogramme|cvv|cvc|cvv2|otp|passcode|access\s+code|door\s+code|gate\s+code|alarm\s+code|verification\s+code|security\s+code|confirmation\s+code|one-time\s+(?:pass)?code|login\s+code|sign-in\s+code|2fa\s+code|code\s+2fa",
            Code,
            Labelled,
            false,
        ),
        label(r"iban|rib", Number, Iban, false),
        label(
            r"(?:numéro|n°|no|num)\s+(?:de\s+)?(?:sécu|sécurité\s+sociale|ss|nir)|sécu|nir|carte\s+vitale|social\s+security\s+(?:number|no)|ssn",
            Number,
            SocialSecurity,
            false,
        ),
        label(
            r"(?:numéro\s+de\s+|n°\s*(?:de\s+)?)?passeport|passport(?:\s+(?:number|no))?",
            Number,
            Passport,
            false,
        ),
        label(
            r"(?:numéro\s+de\s+|n°\s*(?:de\s+)?)?(?:cni|carte\s+(?:nationale\s+)?d['’]identité|pièce\s+d['’]identité)|(?:id|identity)\s+card(?:\s+number)?",
            Number,
            IdentityCard,
            false,
        ),
        label(
            r"(?:numéro|n°|no|identifiant)\s+(?:d['’]identification\s+)?fiscal|numéro\s+spi|spi|nif|numéro\s+de\s+télédéclarant|référence\s+de\s+l['’]avis|numéro\s+fiscal\s+de\s+référence|tax\s+(?:id|number)",
            Number,
            TaxNumber,
            false,
        ),
    ]
});

/// "482913 est votre code", "G-123456 is your verification code".
static CODE_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b((?:[A-Z]-)?\d{4,8})\b\s+(?:est|sera|is|will\s+be)\s+(?:votre|ton|ta|your|le|la|the)\s+(?:[\p{L}\-]+\s+){0,3}?(?:code|pin|mot\s+de\s+passe|password|passcode|otp)\b",
    )
    .expect("a regex")
});

/// "Code PIN 0912", "le code 4821", "digicode 12A45": a code right after
/// its label, without a separator.
static CODE_NEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[^\p{L}\p{N}_])(?P<label>codes?(?:\s+(?:pin|d['’]accès|d['’]entrée|de\s+la\s+porte|du\s+portail|de\s+l['’]alarme|de\s+vérification|de\s+confirmation|de\s+sécurité|wifi))?|pin|digicode|passcode|otp)(?:\s+[\p{L}][\p{L}'’\-]*){0,2}?\s+(?P<value>\d{4,8}[A-Z#*]?|\d{1,4}[A-Z]\d{1,4}|\d{3}\s\d{3})\b",
    )
    .expect("a regex")
});

/// "cryptogramme 412", "CVV 0312": a card's code right after its label.
static CARD_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[^\p{L}\p{N}_])(?:cryptogramme|cvv2?|cvc)\s*:?\s+(\d{3,4})\b")
        .expect("a regex")
});

/// "s'ouvre avec 0619", "tu tapes 1793": a code entered.
static ENTERED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:s['’]ouvre|ouvre|déverrouille|désactive|désarme|opens?|unlocks?)\s+avec\s+(?:le\s+)?(\d{3,8}[#*]?)|(?:^|[^\p{L}])(?:tapes?|tapez|composes?|composez|saisis|saisissez|type|enter)\s+(?:le\s+)?(\d{4,8}[#*]?)\b",
    )
    .expect("a regex")
});

/// "admin / P@ssw0rd123", "x@gmail.com / Vacances2026!": a login and its
/// password side by side.
static PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[\w.+\-]+@[\w.\-]+\.\w+|\b[A-Za-z][\w.\-]{2,})\s+/\s+(\S{6,})")
        .expect("a regex")
});

/// "mdp azerty123", "clé 4FDA9E2C1B7A": a password right after its label,
/// holding a digit.
static PASSWORD_NEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[^\p{L}\p{N}_])(?:mots?\s+de\s+passe|mdp|password|pwd|passcode|clé(?:\s+(?:wifi|wi-fi|wpa))?|key)\s+([^\s]*\d[^\s]*)",
    )
    .expect("a regex")
});

/// "phrase de récupération : lune soleil …": the words that follow, to the
/// end of the line.
static SEED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:phrase\s+de\s+récupération|phrase\s+secrète|seed\s+phrase|recovery\s+phrase|mnemonic)\s*:?\s*([^\n]+)",
    )
    .expect("a regex")
});

/// "code postal", "code promo": a code that is no secret.
static NOT_A_SECRET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)^\s+(?:{NOT_A_SECRET_CODE})\b")).expect("a regex"));

/// "codes de récupération", "backup codes": the codes listed after.
static RECOVERY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)codes?\s+de\s+(?:récupération|secours|sauvegarde|recouvrement|réserve)|(?:recovery|backup)\s+codes?|clé\s+de\s+récupération|recovery\s+key|phrase\s+de\s+récupération|seed\s+phrase|recovery\s+phrase",
    )
    .expect("a regex")
});

static RECOVERY_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[A-Za-z0-9]{4,6}(?:[\- ][A-Za-z0-9]{4,6}){1,7}|[A-Za-z0-9]{8,12})\b")
        .expect("a regex")
});

/// Words that, after a verbal separator, say the sentence is about the
/// password, not its value.
const NOT_A_VALUE: &[&str] = &[
    "le",
    "la",
    "les",
    "l",
    "un",
    "une",
    "des",
    "du",
    "de",
    "d",
    "dans",
    "sur",
    "sous",
    "en",
    "à",
    "au",
    "aux",
    "avec",
    "pour",
    "par",
    "pas",
    "plus",
    "très",
    "trop",
    "assez",
    "toujours",
    "encore",
    "bien",
    "mal",
    "déjà",
    "jamais",
    "ici",
    "là",
    "celui",
    "celle",
    "ce",
    "cet",
    "cette",
    "son",
    "sa",
    "ses",
    "mon",
    "ma",
    "mes",
    "ton",
    "ta",
    "tes",
    "notre",
    "votre",
    "leur",
    "même",
    "nouveau",
    "nouvelle",
    "ancien",
    "ancienne",
    "faux",
    "fausse",
    "bon",
    "bonne",
    "mauvais",
    "correct",
    "incorrect",
    "valide",
    "invalide",
    "requis",
    "obligatoire",
    "vide",
    "différent",
    "identique",
    "simple",
    "compliqué",
    "long",
    "court",
    "sécurisé",
    "fort",
    "faible",
    "nul",
    "perdu",
    "inconnu",
    "personnel",
    "secret",
    "confidentiel",
    "le même",
    "the",
    "a",
    "an",
    "not",
    "too",
    "very",
    "required",
    "wrong",
    "invalid",
    "expired",
    "same",
    "new",
    "old",
    "empty",
    "weak",
    "strong",
    "missing",
    "incorrect",
    "correct",
    "in",
    "on",
    "at",
    "under",
    "your",
    "my",
    "our",
    "their",
    "his",
    "her",
    "its",
    "this",
    "that",
    "here",
    "there",
    "voir",
    "cf",
    "dispo",
    "disponible",
    "envoyé",
    "affiché",
    "écrit",
    "noté",
    "indiqué",
    "ci-dessous",
    "ci-dessus",
    "below",
    "above",
    "attached",
    "enclosed",
    "pareil",
    "inchangé",
    "personnalisé",
    "temporaire",
    "provisoire",
    "unique",
    "partagé",
    "stocké",
    "gardé",
    "changé",
    "oublié",
    "expiré",
    "bloqué",
    "réinitialisé",
    "modifié",
    "demandé",
    "envoyée",
    "reçu",
    "reçue",
    "affichée",
    "valable",
    "actif",
    "active",
    "désactivé",
    "sent",
    "shown",
    "changed",
    "reset",
    "locked",
    "forgotten",
    "needed",
    "set",
    "and",
    "cliquez",
    "cliquer",
    "click",
    "suivez",
    "follow",
    "renseignez",
    "saisissez",
    "enter",
    "et",
    "ou",
    "or",
    "qui",
    "que",
    "quoi",
    "comment",
    "où",
    "?",
    "!",
    "…",
];

pub(super) fn find(text: &str, out: &mut Vec<Found>) {
    for label in LABELS.iter() {
        for c in label.regex.captures_iter(text) {
            let (Some(m), Some(named)) = (c.get(0), c.name("label")) else {
                continue;
            };
            if label.class == Class::Code && NOT_A_SECRET.is_match(&text[named.end()..]) {
                continue;
            }
            let verbal = !label.strict && !separated_strictly(m.as_str());
            if let Some((start, end)) = value(text, m.end(), label.class, verbal) {
                out.push(Found::new(start, end, label.kind));
            }
        }
    }
    for c in CODE_FIRST.captures_iter(text) {
        if let Some(code) = c.get(1) {
            out.push(Found::new(
                code.start(),
                code.end(),
                SecretKind::OneTimeCode,
            ));
        }
    }
    for c in CODE_NEXT.captures_iter(text) {
        if let (Some(named), Some(code)) = (c.name("label"), c.name("value"))
            && !NOT_A_SECRET.is_match(&text[named.end()..])
        {
            out.push(Found::new(code.start(), code.end(), SecretKind::Labelled));
        }
    }
    for c in CARD_CODE.captures_iter(text) {
        if let Some(code) = c.get(1) {
            out.push(Found::new(code.start(), code.end(), SecretKind::Card));
        }
    }
    for c in ENTERED.captures_iter(text) {
        if let Some(code) = c.get(1).or_else(|| c.get(2)) {
            out.push(Found::new(code.start(), code.end(), SecretKind::Labelled));
        }
    }
    for c in PAIR.captures_iter(text) {
        if let Some(value) = c.get(1) {
            let token = value.as_str().trim_end_matches(['.', ',', ';', ')']);
            let digit = token.chars().any(|c| c.is_ascii_digit());
            let letter = token.chars().any(char::is_alphabetic);
            if digit && letter && !version(token) {
                out.push(Found::new(
                    value.start(),
                    value.start() + token.len(),
                    SecretKind::Labelled,
                ));
            }
        }
    }
    for c in PASSWORD_NEXT.captures_iter(text) {
        if let Some(value) = c.get(1) {
            let token = value
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '»', '"', '\'']);
            let date_like = token
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '/' | '-' | '.'));
            if token.chars().count() >= 6 && !version(token) && !date_like {
                out.push(Found::new(
                    value.start(),
                    value.start() + token.len(),
                    SecretKind::Labelled,
                ));
            }
        }
    }
    for c in SEED.captures_iter(text) {
        if let Some(words) = c.get(1) {
            let trimmed = words.as_str().trim_end();
            out.push(Found::new(
                words.start(),
                words.start() + trimmed.len(),
                SecretKind::RecoveryCode,
            ));
        }
    }
    for m in RECOVERY.find_iter(text) {
        // The codes in the few lines after the label.
        let from = m.end();
        let to = text[from..]
            .char_indices()
            .nth(600)
            .map_or(text.len(), |(i, _)| from + i);
        let window = &text[from..to];
        let window = window.split("\n\n\n").next().unwrap_or(window);
        for code in RECOVERY_CODE.find_iter(window) {
            let token = code.as_str();
            if token.chars().any(|c| c.is_ascii_digit()) {
                out.push(Found::new(
                    from + code.start(),
                    from + code.end(),
                    SecretKind::RecoveryCode,
                ));
            }
        }
    }
}

fn separated_strictly(matched: &str) -> bool {
    let tail = matched.trim_end();
    tail.ends_with(':')
        || tail.ends_with('=')
        || tail.ends_with("->")
        || tail.ends_with('→')
        || tail.ends_with("=>")
}

/// The value at `at`, as `class` wants it: its span, or `None` when what
/// follows is not one.
fn value(text: &str, at: usize, class: Class, verbal: bool) -> Option<(usize, usize)> {
    let rest = &text[at..];
    let first = rest.chars().next()?;
    // Quoted: all of it.
    let quotes = [('"', '"'), ('«', '»'), ('“', '”'), ('\'', '\''), ('`', '`')];
    if let Some((_, close)) = quotes.iter().find(|(open, _)| *open == first) {
        let inner = &rest[first.len_utf8()..];
        let inner = inner.strip_prefix(' ').unwrap_or(inner);
        let offset = rest.len() - inner.len();
        let end = inner.find(*close)?;
        let quoted = inner[..end].trim_end();
        if quoted.is_empty() || quoted.contains('\n') || quoted.chars().count() > 80 {
            return None;
        }
        return Some((at + offset, at + offset + quoted.len()));
    }
    let token_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let mut token = &rest[..token_end];
    // Trailing punctuation is the sentence's.
    token = token.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '»', '"', '\'', '…']);
    if token.is_empty() {
        return None;
    }
    let has_digit = token.chars().any(|c| c.is_ascii_digit());
    match class {
        Class::Word => {
            let folded = token.to_lowercase();
            if NOT_A_VALUE.contains(&folded.as_str()) {
                return None;
            }
            if token.starts_with("http://") || token.starts_with("https://") {
                return None;
            }
            // After "est", a word ending as a participle or an adjective
            // describes the password: only a word that looks made up is
            // taken.
            if verbal && !has_digit && !looks_made_up(token) {
                return None;
            }
            Some((at, at + token.len()))
        }
        Class::Code => {
            if !has_digit || version(token) || token.chars().count() > 16 {
                return None;
            }
            // "12 34 56": the groups of digits that follow.
            let mut end = at + token.len();
            let mut rest = &text[end..];
            while let Some(next) = rest.strip_prefix(' ')
                && let Some(group) = next.split(|c: char| !c.is_ascii_digit()).next()
                && (2..=4).contains(&group.len())
                && token.chars().all(|c| c.is_ascii_digit())
            {
                end += 1 + group.len();
                rest = &text[end..];
            }
            Some((at, end))
        }
        Class::Number => {
            // Groups of letters and digits, spaces between them.
            let mut end = at;
            let mut digits = 0;
            for (i, c) in rest.char_indices() {
                if c.is_ascii_alphanumeric() {
                    digits += usize::from(c.is_ascii_digit());
                    end = at + i + 1;
                } else if c == ' ' || c == '-' || c == '.' {
                    let next = rest[i + 1..].chars().next();
                    if !next.is_some_and(|n| n.is_ascii_alphanumeric()) {
                        break;
                    }
                } else {
                    break;
                }
                if end - at > 40 {
                    break;
                }
            }
            (digits >= 4 && end - at >= 6).then_some((at, end))
        }
    }
}

/// `1.2`, `v2`, `4.2.1`: a version.
fn version(token: &str) -> bool {
    let token = token.strip_prefix(['v', 'V']).unwrap_or(token);
    token.contains('.')
        && token
            .split('.')
            .all(|p| p.chars().all(|c| c.is_ascii_digit()))
        || token.chars().all(|c| c.is_ascii_digit()) && token.len() <= 2
}

/// A word that is not a dictionary's: mixed case inside, a symbol, or no
/// vowel in a row of four letters. "chienchat" is not one by this test;
/// after "c'est" a lowercase word is taken all the same when it is not a
/// common word (the caller's list).
fn looks_made_up(token: &str) -> bool {
    let letters = token.chars().filter(|c| c.is_alphabetic()).count();
    let inner_upper = token.chars().skip(1).any(char::is_uppercase);
    let symbol = token
        .chars()
        .any(|c| !c.is_alphanumeric() && !matches!(c, '-' | '\'' | '’'));
    let participle = {
        let lower = token.to_lowercase();
        [
            "é", "ée", "és", "ées", "ed", "ble", "ant", "ment", "if", "ive", "ique", "eux", "euse",
        ]
        .iter()
        .any(|end| lower.ends_with(end))
    };
    inner_upper || symbol || (letters >= 4 && !participle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        find(text, &mut out);
        super::super::merge(out)
            .iter()
            .map(|f| &text[f.start..f.end])
            .collect()
    }

    #[test]
    fn a_labelled_password_is_found_in_french_and_english() {
        assert_eq!(spans("mdp du wifi : chienchat"), ["chienchat"]);
        assert_eq!(spans("Mot de passe: Tr0ub4dor&3."), ["Tr0ub4dor&3"]);
        assert_eq!(
            spans("the password is « horse battery »"),
            ["horse battery"]
        );
        assert_eq!(spans("le mot de passe est oublié"), Vec::<&str>::new());
    }

    #[test]
    fn a_code_must_hold_a_digit() {
        assert_eq!(spans("le code du portail c'est 4821"), ["4821"]);
        assert_eq!(spans("le PIN de la carte est 0912"), ["0912"]);
        assert!(spans("le code de la v2 est prêt").is_empty());
        assert!(spans("le code postal est 75011").is_empty());
        assert_eq!(spans("482913 est votre code de vérification"), ["482913"]);
    }

    #[test]
    fn recovery_codes_are_found_after_their_label() {
        let found = spans("Vos codes de récupération :\n1234-5678\n8765-4321\n");
        assert_eq!(found, ["1234-5678", "8765-4321"]);
    }
}
