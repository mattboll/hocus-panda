#!/usr/bin/env python3
"""Makes the time zone picker's table (docs/design/settings.md, "Date et
heure") from the system's tzdata and CLDR's names, as Babel carries them.

    scripts/zones/generate.py > crates/hocus-settings/data/zones/zones.tsv

The zones are tzdata's `zone.tab` (one line per country and zone, so that
Monaco or Brussels is found by its own name), the cities' and countries'
names CLDR's in French and English (Babel 2.18, CLDR 47; Unicode License
v3, see crates/hocus-settings/data/zones/LICENSE-UNICODE). A few large
cities that share another's zone (Montréal, Mumbai) are added by hand
below, as CLDR names zones, not cities. Tab separated, two kinds of lines:

    Z  zone  country  city-fr  city-en
    C  country  name-fr  name-en
"""

import sys
from pathlib import Path

from babel import Locale
from babel.dates import get_timezone_location

ZONE_TAB = Path("/usr/share/zoneinfo/zone.tab")

# Cities living in another's zone: (zone, country, French, English).
EXTRA = [
    ("America/Toronto", "CA", "Montréal", "Montreal"),
    ("America/Toronto", "CA", "Ottawa", "Ottawa"),
    ("America/New_York", "US", "Washington", "Washington"),
    ("America/New_York", "US", "Boston", "Boston"),
    ("America/New_York", "US", "Miami", "Miami"),
    ("America/Los_Angeles", "US", "San Francisco", "San Francisco"),
    ("America/Los_Angeles", "US", "Seattle", "Seattle"),
    ("America/Chicago", "US", "Houston", "Houston"),
    ("America/Sao_Paulo", "BR", "Rio de Janeiro", "Rio de Janeiro"),
    ("Asia/Kolkata", "IN", "Mumbai", "Mumbai"),
    ("Asia/Kolkata", "IN", "New Delhi", "New Delhi"),
    ("Asia/Kolkata", "IN", "Bangalore", "Bangalore"),
    ("Asia/Shanghai", "CN", "Pékin", "Beijing"),
    ("Asia/Shanghai", "CN", "Shenzhen", "Shenzhen"),
    ("Europe/Zurich", "CH", "Genève", "Geneva"),
    ("Europe/Berlin", "DE", "Munich", "Munich"),
    ("Europe/Rome", "IT", "Milan", "Milan"),
    ("Europe/Madrid", "ES", "Barcelone", "Barcelona"),
    ("Australia/Sydney", "AU", "Canberra", "Canberra"),
    ("Asia/Tokyo", "JP", "Osaka", "Osaka"),
]


def main():
    fr, en = Locale("fr"), Locale("en")
    zones = []
    for line in ZONE_TAB.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        country, _, zone = line.split("\t")[:3]
        zones.append((zone, country))
    zones.sort(key=lambda z: z[0])
    out = sys.stdout
    countries = set()
    for zone, country in zones:
        city_fr = get_timezone_location(zone, locale=fr, return_city=True)
        city_en = get_timezone_location(zone, locale=en, return_city=True)
        out.write(f"Z\t{zone}\t{country}\t{city_fr}\t{city_en}\n")
        countries.add(country)
    for zone, country, city_fr, city_en in EXTRA:
        out.write(f"Z\t{zone}\t{country}\t{city_fr}\t{city_en}\n")
        countries.add(country)
    for country in sorted(countries):
        out.write(f"C\t{country}\t{fr.territories[country]}\t{en.territories[country]}\n")


if __name__ == "__main__":
    main()
