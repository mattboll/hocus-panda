//! The desktop's language and formats (6.2, #402): the compositor says
//! them as the shell connects and whenever `[locale]` changes in the file
//! ([`hocus_link::ToShell::Locale`]). A language chosen there wins over the
//! session's locale for the shell's own words ([`Lang::of_messages`]); the
//! time is written over 24 hours or 12 ([`Hours`]), the date as the file
//! says ([`DateFormat`]). Every surface is then drawn again.

use std::sync::{PoisonError, RwLock};

use hocus_link::locale::{DateFormat, Language, Locale, TimeFormat};
use hocus_words::Lang;

use crate::popover::calendar::words::Hours;

static NOW: RwLock<Locale> = RwLock::new(Locale {
    language: Language::System,
    date: DateFormat::Long,
    time: None,
});

/// The locale from now on; whether it changed.
pub fn set(locale: Locale) -> bool {
    let mut now = NOW.write().unwrap_or_else(PoisonError::into_inner);
    let changed = *now != locale;
    *now = locale;
    Lang::choose(locale.language.lang());
    changed
}

pub fn now() -> Locale {
    *NOW.read().unwrap_or_else(PoisonError::into_inner)
}

/// The hours the file chooses, if it chooses them.
pub fn chosen_hours() -> Option<Hours> {
    now().time.map(|time| match time {
        TimeFormat::H24 => Hours::TwentyFour,
        TimeFormat::H12 => Hours::Twelve,
    })
}

pub fn date_format() -> DateFormat {
    now().date
}
