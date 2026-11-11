//! The window's language, and the words it says in it: every string the
//! user reads is a [`Words`], a text of the catalogue.

/// The window's language: the workspace's one.
pub use hocus_words::Lang;

/// A text in every language: the catalogue's (`hocus_words::settings`).
pub use hocus_words::Text as Words;

#[cfg(test)]
mod tests {

    /// Every key the Settings moved to the catalogue says, byte for byte,
    /// what its pair said in the code before (#775).
    #[test]
    fn the_catalogue_says_what_the_code_said() {
        hocus_words::check_before(crate::words_before::BEFORE, crate::words_before::FORMATS);
    }
}
