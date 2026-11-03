#!/usr/bin/env python3
"""Makes the emoji picker's table (docs/design/emoji.md) from Unicode's and
CLDR's data files.

    scripts/emoji/generate.py DIR > crates/hocus-shell/data/emoji/emoji.tsv

DIR holds, as published by Unicode (Unicode License v3, see
crates/hocus-shell/data/emoji/LICENSE-UNICODE):

- emoji-test.txt, Unicode 15.1
  (unicode-org/unicodetools, unicodetools/data/emoji/15.1/emoji-test.txt);
- ann-en.xml, ann-fr.xml: CLDR 44's common/annotations/{en,fr}.xml;
- der-en.xml, der-fr.xml: CLDR 44's common/annotationsDerived/{en,fr}.xml.

One line per emoji a keyboard offers (fully qualified, its skin tones
aside), in Unicode's order, tab separated:

    group  emoji  light  en-name  en-keywords  fr-name  fr-keywords

`group` is the picker's (smileys, gestures, hearts, animals, food,
objects, symbols, flags); `light` is the emoji with the light skin tone on
each of its people (every 🏻 then swapped for the tone chosen), `-` when it
has no tones; keywords are separated by `|`. Only Python's standard
library.
"""

import sys
import xml.etree.ElementTree as ET
from pathlib import Path

TONES = ["\U0001F3FB", "\U0001F3FC", "\U0001F3FD", "\U0001F3FE", "\U0001F3FF"]
LIGHT = TONES[0]
VS16 = "️"


def group_of(group, subgroup):
    if group == "Smileys & Emotion":
        return "hearts" if subgroup == "heart" else "smileys"
    return {
        "People & Body": "gestures",
        "Animals & Nature": "animals",
        "Food & Drink": "food",
        "Travel & Places": "objects",
        "Activities": "objects",
        "Objects": "objects",
        "Symbols": "symbols",
        "Flags": "flags",
    }.get(group)


def read_test(path):
    """The fully-qualified emoji, in order, with their group and name."""
    group = subgroup = None
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("# group: "):
            group = line[len("# group: "):].strip()
            continue
        if line.startswith("# subgroup: "):
            subgroup = line[len("# subgroup: "):].strip()
            continue
        if not line or line.startswith("#"):
            continue
        points, rest = line.split(";", 1)
        status, comment = rest.split("#", 1)
        if status.strip() != "fully-qualified":
            continue
        emoji = "".join(chr(int(p, 16)) for p in points.split())
        # "😀 E1.0 grinning face"
        name = comment.strip().split(" ", 2)[2]
        out.append((group, subgroup, emoji, name))
    return out


def read_annotations(*paths):
    """CLDR's names (tts) and keywords, by emoji without its VS16."""
    names, keywords = {}, {}
    for path in paths:
        root = ET.parse(path).getroot()
        for a in root.iter("annotation"):
            cp = a.get("cp")
            text = (a.text or "").strip()
            if not cp or not text or text == "↑↑↑":
                continue
            if a.get("type") == "tts":
                names.setdefault(cp, text)
            else:
                words = [w.strip() for w in text.split("|") if w.strip()]
                keywords.setdefault(cp, words)
    return names, keywords


def clean(text):
    return text.replace("\t", " ").replace("|", "/").strip()


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    src = Path(sys.argv[1])
    test = read_test(src / "emoji-test.txt")
    every = {emoji for _, _, emoji, _ in test}
    langs = {}
    for lang in ("en", "fr"):
        langs[lang] = read_annotations(src / f"ann-{lang}.xml", src / f"der-{lang}.xml")
    bare = lambda e: e.replace(VS16, "")
    # The toned forms, by their emoji without tones nor VS16.
    toned = {}
    for _, _, emoji, _ in test:
        if any(t in emoji for t in TONES):
            plain = bare(emoji)
            for t in TONES:
                plain = plain.replace(t, "")
            toned.setdefault(plain, []).append(emoji)
    missing = 0
    count = 0
    for group, subgroup, emoji, name in test:
        ours = group_of(group, subgroup)
        if ours is None or any(t in emoji for t in TONES):
            continue
        light = "-"
        for variant in toned.get(bare(emoji), []):
            if all(t not in variant for t in TONES[1:]) and LIGHT in variant:
                light = variant
                break
        if light != "-":
            for t in TONES[1:]:
                assert light.replace(LIGHT, t) in every, (emoji, t)
        fields = [ours, emoji, light]
        for lang in ("en", "fr"):
            names, keywords = langs[lang]
            key = bare(emoji)
            shown = names.get(key) or names.get(emoji)
            if shown is None:
                missing += 1
                shown = name
            words = keywords.get(key) or keywords.get(emoji) or []
            words = [clean(w) for w in words if clean(w) and clean(w) != clean(shown)]
            fields += [clean(shown), "|".join(words)]
        print("\t".join(fields))
        count += 1
    print(f"{count} emoji, {missing} names from emoji-test.txt", file=sys.stderr)


if __name__ == "__main__":
    main()
