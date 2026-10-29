//! The desktops in the bar (bar.md §4 group 8, §5; 3.9): a tile per
//! desktop of the bar's screen with a sketch of its windows in the full
//! form, a dot per desktop in the mini form; a click goes to that desktop,
//! the wheel to the next or the previous one.
//!
//! The compositor owns the desktops: it says, for each bar's screen, how
//! many there are, which one shows, and each one's windows as rectangles
//! in shares of the screen, as the shell connects and when they change (at
//! most once a frame). The link keeps them here without a message and wakes
//! the desktops' surface, nothing else; a change of their number resizes
//! it ([`draw`]).

pub mod draw;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{DESKTOPS_NAMESPACE, Desktops};

use crate::popover;

static DESKTOPS: Mutex<Option<HashMap<String, Desktops>>> = Mutex::new(None);

/// Bar `bar`'s screen's desktops as last said.
pub fn snapshot(bar: &str) -> Option<Desktops> {
    DESKTOPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(bar)
        .cloned()
}

/// The compositor says `desktops`: kept, the desktops' surfaces woken, and
/// resized when their number changes the rows.
pub fn set(desktops: Desktops) {
    let output = desktops.output.clone();
    let count = desktops.count;
    let before = DESKTOPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(output.clone(), desktops)
        .map(|d| d.count);
    if before != Some(count) {
        draw::count(&output, usize::from(count));
    }
    popover::send(vec![popover::Effect::Wake(DESKTOPS_NAMESPACE)]);
}
