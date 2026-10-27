#!/usr/bin/env python3
"""Builds tests/scenarios/xkb, the scenarios' own copy of a part of XKB's
registry of keyboard layouts and of its French translations (#251), so
that the settings list the same layouts on every machine.

From xkeyboard-config (MIT/Expat licence,
https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config):
rules/evdev.xml, and the French message catalogue its translators keep.

Usage: scripts/xkb-excerpt.py /usr/share/X11/xkb/rules/evdev.xml \
    /usr/share/locale/fr/LC_MESSAGES/xkeyboard-config.mo tests/scenarios/xkb

Only the layouts below, with all their variants, are kept; the catalogue
keeps the translations of their names, written again as a .mo file
(Python's standard library only).
"""
import gettext
import os
import struct
import sys
import xml.etree.ElementTree as ET

LAYOUTS = ["us", "gb", "fr", "be", "ch", "ca", "de", "es", "it"]

HEADER = """<?xml version="1.0" encoding="UTF-8"?>
<!-- An excerpt of xkeyboard-config's rules/evdev.xml (MIT/Expat licence),
     made by scripts/xkb-excerpt.py for the scenarios. -->
"""


def write_mo(path, pairs):
    """A GNU message catalogue, little-endian, its strings sorted."""
    pairs = sorted(pairs.items())
    n = len(pairs)
    originals = b""
    translations = b""
    o_table = []
    t_table = []
    start = 28 + 16 * n
    for original, _ in pairs:
        data = original.encode()
        o_table.append((len(data), start + len(originals)))
        originals += data + b"\0"
    start += len(originals)
    for _, translation in pairs:
        data = translation.encode()
        t_table.append((len(data), start + len(translations)))
        translations += data + b"\0"
    out = struct.pack("<7I", 0x950412DE, 0, n, 28, 28 + 8 * n, 0, 0)
    for length, at in o_table + t_table:
        out += struct.pack("<2I", length, at)
    with open(path, "wb") as f:
        f.write(out + originals + translations)


def main():
    rules, catalogue, out = sys.argv[1:4]
    root = ET.parse(rules).getroot()
    keep = ET.Element("xkbConfigRegistry", version=root.get("version", "1.1"))
    layout_list = ET.SubElement(keep, "layoutList")
    names = []
    for layout in root.iter("layout"):
        item = layout.find("configItem")
        if item.findtext("name") not in LAYOUTS:
            continue
        layout_list.append(layout)
        names += [d.text for d in layout.iter("description")]
    ET.indent(keep, space="  ")
    os.makedirs(os.path.join(out, "rules"), exist_ok=True)
    with open(os.path.join(out, "rules", "evdev.xml"), "w", encoding="utf-8") as f:
        f.write(HEADER + ET.tostring(keep, encoding="unicode") + "\n")
    with open(catalogue, "rb") as f:
        french = gettext.GNUTranslations(f)
    pairs = {"": "Content-Type: text/plain; charset=UTF-8\n"}
    for name in names:
        translated = french.gettext(name)
        if translated != name:
            pairs[name] = translated
    messages = os.path.join(out, "locale", "fr", "LC_MESSAGES")
    os.makedirs(messages, exist_ok=True)
    write_mo(os.path.join(messages, "xkeyboard-config.mo"), pairs)


main()
