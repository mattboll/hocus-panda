//! The secrets corpus (AC50, its first half): the first pass alone, code
//! detectors only, over `fixtures/secrets-corpus.toml`. Its recall and
//! false positives are reported (`--no-capture` shows them) and held at
//! least where they are: a rule change that loses a secret, or masks a
//! sentence it did not, fails here.
//!
//! The second pass (the local model, lot 4) is meant to catch what code
//! cannot ("le portail s'ouvre avec 4821"); together they must reach 99 %.

use hocus_sources::Redactor;
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
