# Fixtures

Data the scenarios use. `applications/` holds desktop entries, read by the
launcher when a scenario sets `data_dirs = ["../fixtures"]`: a few entries
named like real applications (their command does nothing), one hidden one,
and one that writes the environment it was started with into the
compositor's runtime directory. `icons/` is their icon theme: Files has an
icon (a folder drawn for the project, SVG), the others none, to show the
glyph the dock draws instead.

`wallpapers/` holds two small pictures for the wallpaper scenarios: one
mostly orange (with a blue corner), one blue.

`system/` is for the settings' Applications page, with `data_dirs =
["../fixtures/system/data"]` and `config_dirs = ["../fixtures/system/config"]`:
applications that open web links, mail, folders, text, pictures and PDF
files (two web browsers), the system's `mimeapps.list` choosing some, and
two autostart entries, one turned off the GNOME way.

`search-home/` is the private home of the document search scenarios (#45),
copied into a temporary directory by `home = "../fixtures/search-home"`:
documents of every kind searched (Markdown, text, HTML, PDF, OpenDocument,
Word) and a screenshot read by the OCR stand-in, beside what is never read
(`.ssh` with a fake key, a project's `.env`, a `.pem` file; none holds a
real secret). They were written from scratch by `make-search-home.py`
(Python's standard library only), so they are the project's own; `.env` is
added with `git add -f`, as many ignore files leave it out.
