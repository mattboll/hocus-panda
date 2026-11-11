# 0006 — One language type, and the words in one catalogue file a language

Date: 2026-11-11 · Status: accepted (#775), migrating crate by crate

## Context

The November 2026 audit counted five French-or-English enums (the
shell's clock, the Settings, hocus-perf, hocus-link, and the
compositor's `french` flag), 27 `Words` structs, 15 identical
`fn pick(self, fr, en)`, about 400 `pick(` calls in the shell and 500
`Lang::French` arms in the Settings. Plurals were written per string
(`if n > 1 { "s" }`). A third language meant touching a thousand sites.

## Options

- **Fluent** (`fluent-bundle`, Apache-2.0 OR MIT, with `fluent-syntax`,
  `intl_pluralrules`, `unic-langid`, `intl-memoizer` and a few smaller
  crates). A complete system with CLDR plurals for every language, but
  the messages are parsed at run time, looked up by string, and their
  arguments passed in a map: a missing key or a misspelt argument is seen
  on screen, not at build time. It adds about ten crates to the shell and
  the Settings.
- **A generated table**: one TOML file a language, compiled into Rust by a
  build script. Typed keys and arguments; nothing to parse at run time;
  `toml` (already a dependency) at build time only.

## Decision

A generated table, in the crate `hocus-words`:

- `words/en.toml` is the reference; every other file (`words/fr.toml`)
  has the same keys, no more, and only the placeholders English has. Its
  `[language]` table names the language (`code`, `variant`, `name`) and
  its plural rule (`n=1`, `n<2`, `none`). The `Lang` enum is generated
  from the files: adding a language is adding a file.
- A plain text is a constant (`hocus_words::perf::MEMORY.get(lang)`); a
  text with `{placeholders}` or plural forms (`{ one = …, other = … }`) a
  function taking the language and its argument
  (`hocus_words::perf::windows_opened(lang, 3)`), or, with several, a
  struct of named fields, so that two of one type cannot be swapped
  (`perf::signal::MemoryLine { end, rate, since, start }`); a list (the
  months) a constant of one length in every language.
- What the files disagree on stops the build, and says which key in which
  file; a key the code asks for and no file has does not compile; a
  placeholder English gains changes the function, so every caller is
  seen.
- Plurals follow each language's rule, French counting 0 and 1 as
  singular. A language with more forms (few, many) adds them to `Plural`
  and its rule to the build script.
- `Lang` also holds how the language is chosen: the session's locale
  (`of_messages`, `of_time`, `of_numbers`) unless a program chose one
  (`Lang::choose`, the shell's `[locale] language`).

Formatting that is not words (decimal commas, times, `{n} %`) stays in
code, keyed on `Lang` where it differs.

## Consequences

- Each crate's surfaces move to the catalogue one at a time. Each keeps
  a test that renders what it says, in both languages, against what it
  said before the move (`tests/fixtures/words-*.txt`), so the text stays
  byte for byte the same.
- No code chooses between French and English any more: CI builds the
  workspace with a third language file (`HOCUS_WORDS_TEST_LANGUAGE=1`),
  and a `match lang` that forgets one stops that build.
