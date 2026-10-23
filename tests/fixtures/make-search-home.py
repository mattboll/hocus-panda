#!/usr/bin/env python3
"""Makes tests/fixtures/search-home: the private home of the document search
scenarios (#45). Its documents are written here from scratch with Python's
standard library (zipfile, zlib, struct), so they are the project's own
(MIT). Run from anywhere: python3 -I tests/fixtures/make-search-home.py"""

import os
import struct
import zipfile
import zlib

HOME = os.path.join(os.path.dirname(os.path.abspath(__file__)), "search-home")
DOCS = os.path.join(HOME, "Documents")


def write(path, data):
    path = os.path.join(HOME, path)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data if isinstance(data, bytes) else data.encode())


def pdf(lines):
    """A one-page PDF in Helvetica, its content deflated, with a valid
    cross-reference table."""
    content = "BT /F1 18 Tf 72 720 Td\n"
    for i, line in enumerate(lines):
        if i:
            content += "0 -26 Td\n"
        words = " -250 ".join("(%s)" % w for w in line.split(" "))
        content += "[%s] TJ\n" % words
    content += "ET\n"
    packed = zlib.compress(content.encode("latin-1"))
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        b"<< /Filter /FlateDecode /Length %d >>\nstream\n" % len(packed)
        + packed
        + b"\nendstream",
    ]
    out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"
    offsets = []
    for n, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % n + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for o in offsets:
        out += b"%010d 00000 n \n" % o
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objects) + 1,
        xref,
    )
    return out


def office(path, parts):
    full = os.path.join(HOME, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with zipfile.ZipFile(full, "w") as z:
        for name, data, method in parts:
            info = zipfile.ZipInfo(name, date_time=(2026, 10, 21, 14, 37, 0))
            info.compress_type = method
            z.writestr(info, data)


def png(width, height, rgb):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\0" + bytes(rgb) * width for _ in range(height))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


ODT_MANIFEST = """<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">
 <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>
"""

ODT_CONTENT = """<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3">
 <office:body>
  <office:text>
   <text:h text:outline-level="1">Greenhouse minutes</text:h>
   <text:p>We agreed to water the ferns twice a week<text:s/>and to open the roof on warm days.</text:p>
  </office:text>
 </office:body>
</office:document-content>
"""

DOCX_TYPES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="xml" ContentType="application/xml"/>
 <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>
"""

DOCX_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>
"""

DOCX_DOCUMENT = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
 <w:body>
  <w:p><w:r><w:t>Dear keeper,</w:t></w:r></w:p>
  <w:p><w:r><w:t xml:space="preserve">Thank you for the </w:t></w:r><w:r><w:t>zookeeper training last Tuesday.</w:t></w:r></w:p>
 </w:body>
</w:document>
"""

write(
    "Documents/Bamboo harvest.md",
    "# Bamboo harvest\n\nThe spring harvest gave 340 kg of fresh shoots.\nStore them cool and dry.\n",
)
write(
    "Documents/Invoice March.txt",
    "Invoice for 120 kg of bamboo shoots, delivered on 3 March.\n",
)
write(
    "Documents/Panda care.html",
    "<!doctype html><html><head><title>Panda care</title><style>p { margin: 0 }</style></head>"
    "<body><p>Feed the pandas at dawn &amp; dusk.</p></body></html>\n",
)
write(
    "Documents/Quarterly report.pdf",
    pdf(["Quarterly report", "Visitors to the panda house rose by 12 percent."]),
)
office(
    "Documents/Greenhouse minutes.odt",
    [
        ("mimetype", "application/vnd.oasis.opendocument.text", zipfile.ZIP_STORED),
        ("META-INF/manifest.xml", ODT_MANIFEST, zipfile.ZIP_DEFLATED),
        ("content.xml", ODT_CONTENT, zipfile.ZIP_DEFLATED),
    ],
)
office(
    "Documents/Letter to the keeper.docx",
    [
        ("[Content_Types].xml", DOCX_TYPES, zipfile.ZIP_DEFLATED),
        ("_rels/.rels", DOCX_RELS, zipfile.ZIP_DEFLATED),
        ("word/document.xml", DOCX_DOCUMENT, zipfile.ZIP_DEFLATED),
    ],
)
# Never read, whatever the configuration says; not real secrets.
write("Documents/.ssh/id_ed25519", "Not a key: a fixture the document search never reads. bamboo\n")
write("Documents/.ssh/notes.txt", "bamboo notes kept with the keys\n")
write("Documents/project/.env", "PANDA_TOKEN=bamboo-not-a-secret\n")
write("Documents/project/server.pem", "Not a certificate: a fixture. bamboo\n")
write("Documents/project/plan.md", "Plan the bamboo nursery.\n")
# A screenshot: the OCR stand-in reads its text.
write("Pictures/Screenshots/Screenshot build error.png", png(4, 4, (40, 44, 52)))
