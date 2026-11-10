//! Without a keyring, her memory's key is in a file beside it (#768):
//! her status says where it is, and "Ce qu'elle sait" carries it, so that
//! the drawer and the Settings never say "encrypted" alone.

use crate::common;

use hocus_secretary::{
    brain::Options,
    key::{self, Place},
    store::Store,
};
use hocus_secretary_link::{
    FromSecretary, KEY_IN_FILE, ToSecretary, key_beside_said, knows::Query,
};

fn told(place: Place) -> (String, bool) {
    let options = Options {
        key: place.word().to_owned(),
        ..common::options(Vec::new())
    };
    let mut driver =
        common::Driver::start(Store::in_memory(common::clock()).expect("a store"), options);
    let status = driver.status();
    let view = match driver.request(|id| ToSecretary::Knows {
        id,
        query: Some(Query::default()),
    }) {
        FromSecretary::Knows { view, .. } => view,
        other => panic!("not what she knows: {other:?}"),
    };
    driver.stop();
    (status.key, view.key_beside)
}

#[test]
fn a_key_file_beside_her_memory_is_said_where_her_memory_is_shown() {
    // No bus: the key is made in a file beside her memory, as her process
    // does at its first start.
    let dir = tempfile::tempdir().expect("a folder");
    let (_, place) = key::key(dir.path(), None, false).expect("a key");
    assert_eq!(place, Place::File);
    assert!(dir.path().join(key::FILE).exists());
    assert_eq!(told(place), (KEY_IN_FILE.to_owned(), true));
    // In a keyring: nothing to say.
    assert_eq!(told(Place::Keyring), ("keyring".to_owned(), false));
}

#[test]
fn the_sentence_says_where_the_key_is_and_what_it_means() {
    let fr = key_beside_said(true);
    for part in [
        "Sa mémoire est chiffrée",
        "sa clé est rangée à côté, sur ce disque",
        "ton trousseau",
        "Qui peut lire tes fichiers peut donc lire sa mémoire",
    ] {
        assert!(fr.contains(part), "{part}: {fr}");
    }
    let en = key_beside_said(false);
    for part in [
        "Her memory is encrypted",
        "its key is kept beside it, on this disk",
        "your keyring",
        "Whoever can read your files can therefore read her memory",
    ] {
        assert!(en.contains(part), "{part}: {en}");
    }
}
