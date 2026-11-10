//! The field's model, one test per criterion of #763's keyboard and
//! pointer, on its text's own lines ([`Paragraphs`]).

use super::*;

fn field(text: &str) -> Field {
    let mut field = Field::new(Kind::Lines, 2000);
    field.set(text);
    field
}

/// `act` on `field`, on its paragraphs.
fn act(field: &mut Field, act: Act) -> Outcome {
    let text = field.text().to_owned();
    field.apply(act, &Paragraphs(&text))
}

fn go(field: &mut Field, motion: Motion) -> Outcome {
    act(
        field,
        Act::Move {
            motion,
            extend: false,
        },
    )
}

fn select(field: &mut Field, motion: Motion) -> Outcome {
    act(
        field,
        Act::Move {
            motion,
            extend: true,
        },
    )
}

/// The text before the caret, and what is selected.
fn before_caret(field: &Field) -> &str {
    &field.text()[..field.caret()]
}

fn selected(field: &Field) -> &str {
    &field.text()[field.selection()]
}

#[test]
fn left_and_right_move_a_character_and_collapse_a_selection() {
    let mut f = field("café");
    assert_eq!(go(&mut f, Motion::Left), Outcome::Moved);
    assert_eq!(before_caret(&f), "caf", "over « é », two bytes");
    go(&mut f, Motion::Right);
    assert_eq!(go(&mut f, Motion::Right), Outcome::Same, "at the end");
    select(&mut f, Motion::Left);
    select(&mut f, Motion::Left);
    assert_eq!(selected(&f), "fé");
    go(&mut f, Motion::Right);
    assert_eq!(
        before_caret(&f),
        "café",
        "Right goes to the selection's end"
    );
    assert!(f.selection().is_empty());
}

#[test]
fn ctrl_left_and_right_move_by_word() {
    let mut f = field("Bonjour Léa, à demain");
    go(&mut f, Motion::WordLeft);
    assert_eq!(before_caret(&f), "Bonjour Léa, à ");
    go(&mut f, Motion::WordLeft);
    go(&mut f, Motion::WordLeft);
    assert_eq!(before_caret(&f), "Bonjour ", "over « , » and « à »");
    go(&mut f, Motion::WordRight);
    assert_eq!(before_caret(&f), "Bonjour Léa", "to the word's end");
    go(&mut f, Motion::Start);
    assert_eq!(go(&mut f, Motion::WordLeft), Outcome::Same);
}

#[test]
fn home_and_end_go_to_the_lines_ends_and_ctrl_to_the_texts() {
    let mut f = field("un\ndeux trois\nquatre");
    go(&mut f, Motion::Up);
    go(&mut f, Motion::LineStart);
    assert_eq!(before_caret(&f), "un\n");
    go(&mut f, Motion::LineEnd);
    assert_eq!(before_caret(&f), "un\ndeux trois");
    go(&mut f, Motion::Start);
    assert_eq!(f.caret(), 0);
    go(&mut f, Motion::End);
    assert_eq!(f.caret(), f.text().len());
}

#[test]
fn up_and_down_go_between_lines_and_past_them_to_the_ends() {
    let mut f = field("abcdef\nab\nabcdef");
    go(&mut f, Motion::Up);
    assert_eq!(before_caret(&f), "abcdef\nab", "the short line's end");
    go(&mut f, Motion::Up);
    assert_eq!(before_caret(&f), "abcdef", "back to the column");
    go(&mut f, Motion::Up);
    assert_eq!(f.caret(), 0, "above the first line: its start");
    go(&mut f, Motion::Down);
    go(&mut f, Motion::Down);
    go(&mut f, Motion::Down);
    assert_eq!(f.caret(), f.text().len(), "under the last: its end");
}

#[test]
fn shift_with_any_motion_selects() {
    let mut f = field("un deux\ntrois");
    select(&mut f, Motion::WordLeft);
    assert_eq!(selected(&f), "trois");
    select(&mut f, Motion::Up);
    assert_eq!(selected(&f), "un deux\ntrois", "its column, 0, a line up");
    // Back the other way past the anchor.
    select(&mut f, Motion::End);
    assert!(f.selection().is_empty());
    select(&mut f, Motion::Left);
    assert_eq!(selected(&f), "s");
}

#[test]
fn ctrl_a_selects_it_all() {
    let mut f = field("tout ça");
    assert_eq!(act(&mut f, Act::SelectAll), Outcome::Moved);
    assert_eq!(selected(&f), "tout ça");
    assert_eq!(act(&mut f, Act::SelectAll), Outcome::Same);
}

#[test]
fn backspace_and_delete_take_a_character_and_with_ctrl_a_word() {
    let mut f = field("Bonjour Léa, ça va");
    act(&mut f, Act::Backspace { word: false });
    assert_eq!(f.text(), "Bonjour Léa, ça v");
    act(&mut f, Act::Backspace { word: true });
    assert_eq!(f.text(), "Bonjour Léa, ça ");
    act(&mut f, Act::Backspace { word: true });
    assert_eq!(f.text(), "Bonjour Léa, ", "the space and « ça »");
    go(&mut f, Motion::Start);
    act(&mut f, Act::Delete { word: false });
    assert_eq!(f.text(), "onjour Léa, ");
    act(&mut f, Act::Delete { word: true });
    assert_eq!(f.text(), " Léa, ");
    act(&mut f, Act::Delete { word: true });
    assert_eq!(f.text(), ", ", "the space and « Léa »");
    go(&mut f, Motion::Start);
    assert_eq!(act(&mut f, Act::Backspace { word: false }), Outcome::Same);
}

#[test]
fn backspace_and_delete_take_the_selection() {
    let mut f = field("un deux trois");
    select(&mut f, Motion::WordLeft);
    act(&mut f, Act::Backspace { word: true });
    assert_eq!(f.text(), "un deux ", "the selection, not a word more");
    select(&mut f, Motion::Start);
    act(&mut f, Act::Delete { word: false });
    assert_eq!(f.text(), "");
}

#[test]
fn typing_replaces_the_selection() {
    let mut f = field("Je relis ton article");
    select(&mut f, Motion::WordLeft);
    act(&mut f, Act::Insert("mail".to_owned()));
    assert_eq!(f.text(), "Je relis ton mail");
    assert!(f.selection().is_empty());
    assert_eq!(before_caret(&f), "Je relis ton mail");
}

#[test]
fn a_field_takes_its_most_and_on_lines_keeps_its_breaks() {
    let mut f = Field::new(Kind::Lines, 5);
    assert_eq!(act(&mut f, Act::Insert("abcdef".to_owned())), Outcome::Same);
    assert_eq!(f.paste("ab\r\ncd"), Outcome::Edited);
    assert_eq!(f.text(), "ab\ncd");
    assert_eq!(
        act(&mut f, Act::Insert("e".to_owned())),
        Outcome::Same,
        "full"
    );
    let mut key = Field::new(Kind::Secret, 50);
    key.paste("sk-ant\n-xx\t");
    assert_eq!(key.text(), "sk-ant-xx", "on one line, nothing but the key");
}

#[test]
fn copy_and_cut_give_the_selection_and_paste_asks_for_the_clipboard() {
    let mut f = field("Léa relit");
    assert_eq!(act(&mut f, Act::Copy), Outcome::Same, "nothing selected");
    select(&mut f, Motion::WordLeft);
    assert_eq!(act(&mut f, Act::Copy), Outcome::Copy("relit".to_owned()));
    assert_eq!(f.text(), "Léa relit");
    assert_eq!(act(&mut f, Act::Cut), Outcome::Cut("relit".to_owned()));
    assert_eq!(f.text(), "Léa ");
    assert_eq!(act(&mut f, Act::Paste), Outcome::Paste);
    f.paste("écrit");
    assert_eq!(f.text(), "Léa écrit");
}

#[test]
fn a_secret_is_never_copied_nor_cut_out_and_keeps_nothing_to_undo() {
    let mut key = Field::new(Kind::Secret, 512);
    act(&mut key, Act::Insert("sk-ant-api03".to_owned()));
    act(&mut key, Act::SelectAll);
    assert_eq!(act(&mut key, Act::Copy), Outcome::Same);
    assert_eq!(act(&mut key, Act::Cut), Outcome::Same);
    assert_eq!(key.text(), "sk-ant-api03", "nothing left it");
    // Its words are its whole: Ctrl+Backspace empties it.
    go(&mut key, Motion::End);
    act(&mut key, Act::Backspace { word: true });
    assert_eq!(key.text(), "");
    assert_eq!(act(&mut key, Act::Undo), Outcome::Same, "nothing kept");
    // A double click takes it all.
    act(&mut key, Act::Insert("ab-cd".to_owned()));
    key.press(1, 2, false);
    assert_eq!(key.selection(), 0..5);
}

#[test]
fn ctrl_z_undoes_a_word_at_a_time_and_ctrl_shift_z_redoes() {
    let mut f = Field::new(Kind::Lines, 100);
    for c in "Bonjour Léa".chars() {
        act(&mut f, Act::Insert(c.to_string()));
    }
    act(&mut f, Act::Backspace { word: false });
    act(&mut f, Act::Backspace { word: false });
    assert_eq!(f.text(), "Bonjour L");
    act(&mut f, Act::Undo);
    assert_eq!(f.text(), "Bonjour Léa", "the deletions at once");
    act(&mut f, Act::Undo);
    assert_eq!(f.text(), "Bonjour", "then the word typed");
    assert_eq!(before_caret(&f), "Bonjour");
    act(&mut f, Act::Undo);
    assert_eq!(f.text(), "");
    assert_eq!(act(&mut f, Act::Undo), Outcome::Same);
    act(&mut f, Act::Redo);
    act(&mut f, Act::Redo);
    assert_eq!(f.text(), "Bonjour Léa");
    // An edit forgets what was undone.
    act(&mut f, Act::Undo);
    act(&mut f, Act::Insert("!".to_owned()));
    assert_eq!(act(&mut f, Act::Redo), Outcome::Same);
    assert_eq!(f.text(), "Bonjour!");
}

#[test]
fn a_move_ends_a_step_of_typing() {
    let mut f = field("ab");
    act(&mut f, Act::Insert("c".to_owned()));
    go(&mut f, Motion::Start);
    act(&mut f, Act::Insert("x".to_owned()));
    act(&mut f, Act::Undo);
    assert_eq!(f.text(), "abc");
}

#[test]
fn a_click_places_the_caret_a_drag_selects_and_shift_click_extends() {
    let mut f = field("Bonjour Léa, à demain");
    f.press(3, 1, false);
    assert_eq!(f.caret(), 3);
    assert!(f.selection().is_empty());
    assert!(f.drag(12));
    assert_eq!(selected(&f), "jour Léa");
    f.release();
    assert_eq!(f.gesture, Gesture::Idle);
    assert!(!f.drag(1), "no press held");
    // Shift+click: from the anchor to there.
    f.press(0, 1, true);
    assert_eq!(selected(&f), "Bon");
    // Back past the anchor while dragging.
    f.press(14, 1, false);
    f.drag(8);
    assert_eq!(selected(&f), "Léa, ");
    assert_eq!(f.caret(), 8);
}

#[test]
fn a_double_click_takes_a_word_and_a_triple_its_line() {
    let mut f = field("Bonjour Léa,\nà demain");
    f.press(9, 2, false);
    assert_eq!(selected(&f), "Léa");
    // Dragged on: word by word.
    f.drag(1);
    assert_eq!(selected(&f), "Bonjour Léa");
    f.release();
    f.press(16, 3, false);
    assert_eq!(selected(&f), "à demain");
}

#[test]
fn a_preedit_shows_until_it_is_committed() {
    let mut f = field("le ");
    assert_eq!(f.compose("に", Some(0..3)), Outcome::Edited);
    assert_eq!(f.preedit().map(|p| p.text.as_str()), Some("に"));
    assert_eq!(f.text(), "le ", "not in the text yet");
    act(&mut f, Act::Insert("日本".to_owned()));
    assert_eq!(f.text(), "le 日本");
    assert!(f.preedit().is_none());
    assert_eq!(f.compose("", None), Outcome::Same);
}

#[test]
fn set_can_be_undone_take_empties_and_forgets() {
    let mut f = field("brouillon");
    f.set("autre chose");
    act(&mut f, Act::Undo);
    assert_eq!(f.text(), "brouillon");
    assert_eq!(f.take(), "brouillon");
    assert!(f.is_empty());
    assert_eq!(act(&mut f, Act::Undo), Outcome::Same);
}

#[test]
fn words_and_lines_around_a_place() {
    let text = "porte-monnaie, ici\nlà";
    assert_eq!(&text[word_around(text, 3)], "porte-monnaie");
    assert_eq!(
        &text[word_around(text, 13)],
        "porte-monnaie",
        "just past it"
    );
    assert_eq!(&text[word_around(text, 14)], " ");
    assert_eq!(&text[line_around(text, 16)], "porte-monnaie, ici");
    assert_eq!(&text[line_around(text, text.len())], "là");
    assert_eq!(word_around("", 0), 0..0);
}
