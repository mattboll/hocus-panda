//! The secrets corpus (AC50): the first pass alone, code detectors only,
//! over `fixtures/secrets-corpus.toml`, then both passes. Their recall and
//! false positives are reported (`--no-capture` shows them) and held at
//! least where they are: a rule change that loses a secret, or masks a
//! sentence it did not, fails here.
//!
//! The second pass (the local model, §14.1, 2) is meant to catch what
//! code cannot ("le portail s'ouvre avec 4821"); together they must reach
//! 99 %. In CI its answers are recorded (`fixtures/secrets-pass2.toml`,
//! scripted by hand until a bench run records a model's): each item goes
//! through the real path, the first pass, then `Redacted::second_pass`
//! with the detector's quotes, then what may leave (`outbound`).

use hocus_sources::{
    Content, Item, Notebook, Outbound, Pointer, Quoted, Redactor, SourceKey, SourceKind, item::Mail,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Corpus {
    secret: Vec<Secret>,
    clean: Vec<Clean>,
}

#[derive(Deserialize)]
struct Secret {
    text: String,
    secrets: Vec<String>,
}

#[derive(Deserialize)]
struct Clean {
    text: String,
}

/// The detector's recorded answers.
#[derive(Deserialize)]
struct Recorded {
    answer: Vec<Answer>,
}

#[derive(Deserialize)]
struct Answer {
    sentence: String,
    secrets: Vec<Flagged>,
}

#[derive(Deserialize)]
struct Flagged {
    text: String,
    #[allow(dead_code, reason = "the schema's, read for its shape")]
    kind: String,
}

/// A detector that answers from the recording.
struct Scripted(Recorded);

impl Scripted {
    fn quotes(&self, sentence: &str) -> Quoted {
        Quoted::new(
            self.0
                .answer
                .iter()
                .filter(|a| a.sentence == sentence)
                .flat_map(|a| a.secrets.iter().map(|s| s.text.clone())),
        )
    }
}

/// A sentence as a mail's text, through both passes: the text that may
/// leave.
fn both_passes(redactor: &Redactor, detector: &Scripted, sentence: &str) -> String {
    let item = Item {
        pointer: Pointer {
            source: SourceKey::new("corpus", SourceKind::Mail),
            id: "1".to_owned(),
            label: "le mail de test".to_owned(),
            open: None,
        },
        notebook: Notebook::Pro,
        at: jiff::Timestamp::UNIX_EPOCH,
        content: Content::Mail(Mail {
            subject: "corpus".to_owned(),
            body: Some(sentence.to_owned()),
            ..Mail::default()
        }),
    };
    let mut redacted = redactor.redact(item);
    assert!(
        matches!(redacted.outbound(), Outbound::MetadataOnly { .. }),
        "no text leaves before the second pass"
    );
    let mut pass = detector.quotes(sentence);
    redacted.second_pass(&mut pass).expect("a recorded answer");
    match redacted.outbound() {
        Outbound::Checked(item) => match &item.content {
            Content::Mail(mail) => mail.body.clone().unwrap_or_default(),
            _ => String::new(),
        },
        Outbound::MetadataOnly { .. } => panic!("checked by both passes"),
    }
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether `secret` is gone from `masked`: a short one entirely, a long
/// one down to no 8 characters in a row.
fn gone(secret: &str, masked: &str) -> bool {
    let (secret, masked) = (compact(secret), compact(masked));
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return !masked.contains(&secret);
    }
    chars
        .windows(8)
        .all(|w| !masked.contains(&w.iter().collect::<String>()))
}

#[test]
fn the_first_pass_over_the_corpus() -> anyhow::Result<()> {
    // `{{}}` keeps a forge's secret scanning off the made-up tokens.
    let corpus: Corpus =
        toml::from_str(&include_str!("fixtures/secrets-corpus.toml").replace("{{}}", ""))?;
    let redactor = Redactor::new();
    let source = "le mail de test";
    let mut missed = Vec::new();
    for case in &corpus.secret {
        let masked = redactor.mask_text(&case.text, source);
        if !case.secrets.iter().all(|s| gone(s, &masked)) {
            missed.push((case.text.as_str(), masked));
        }
    }
    let mut flagged = Vec::new();
    for case in &corpus.clean {
        let masked = redactor.mask_text(&case.text, source);
        if masked != case.text {
            flagged.push((case.text.as_str(), masked));
        }
    }
    let (secrets, clean) = (corpus.secret.len(), corpus.clean.len());
    let caught = secrets - missed.len();
    let recall = caught as f64 / secrets as f64 * 100.0;
    let false_positives = flagged.len() as f64 / clean as f64 * 100.0;
    println!(
        "secrets corpus, pass 1 (code only), rules v{}:",
        redactor.version()
    );
    println!("  recall: {caught}/{secrets} ({recall:.1} %)");
    println!(
        "  false positives: {}/{clean} ({false_positives:.1} %)",
        flagged.len()
    );
    for (text, masked) in &missed {
        println!("  missed: {text:?} → {masked:?}");
    }
    for (text, masked) in &flagged {
        println!("  flagged: {text:?} → {masked:?}");
    }
    assert!(
        secrets >= 100 && clean >= 100,
        "the corpus holds about 100 of each"
    );
    assert!(recall >= 95.0, "pass 1's recall fell to {recall:.1} %");
    assert!(
        false_positives <= 2.0,
        "pass 1's false positives rose to {false_positives:.1} %"
    );
    Ok(())
}

#[test]
fn both_passes_over_the_corpus() -> anyhow::Result<()> {
    let corpus: Corpus =
        toml::from_str(&include_str!("fixtures/secrets-corpus.toml").replace("{{}}", ""))?;
    let detector = Scripted(toml::from_str(include_str!("fixtures/secrets-pass2.toml"))?);
    let redactor = Redactor::new();
    let mut missed = Vec::new();
    let mut by_pass_2 = 0;
    for case in &corpus.secret {
        let first = redactor.mask_text(&case.text, "le mail de test");
        let both = both_passes(&redactor, &detector, &case.text);
        if !case.secrets.iter().all(|s| gone(s, &both)) {
            missed.push((case.text.as_str(), both));
        } else if !case.secrets.iter().all(|s| gone(s, &first)) {
            by_pass_2 += 1;
        }
    }
    let mut flagged = Vec::new();
    for case in &corpus.clean {
        let both = both_passes(&redactor, &detector, &case.text);
        if both != case.text {
            flagged.push((case.text.as_str(), both));
        }
    }
    let (secrets, clean) = (corpus.secret.len(), corpus.clean.len());
    let caught = secrets - missed.len();
    let recall = caught as f64 / secrets as f64 * 100.0;
    let false_positives = flagged.len() as f64 / clean as f64 * 100.0;
    println!(
        "secrets corpus, passes 1 and 2 (rules v{}, pass 2 recorded: {} answers, scripted):",
        redactor.version(),
        detector.0.answer.len()
    );
    println!("  recall: {caught}/{secrets} ({recall:.1} %), {by_pass_2} by pass 2 alone");
    println!(
        "  false positives: {}/{clean} ({false_positives:.1} %)",
        flagged.len()
    );
    for (text, masked) in &missed {
        println!("  missed: {text:?} → {masked:?}");
    }
    for (text, masked) in &flagged {
        println!("  flagged: {text:?} → {masked:?}");
    }
    assert!(recall >= 99.0, "passes 1 and 2's recall is {recall:.1} %");
    assert!(
        false_positives <= 2.0,
        "passes 1 and 2's false positives rose to {false_positives:.1} %"
    );
    Ok(())
}
