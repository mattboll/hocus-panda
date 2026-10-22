//! Unit conversion for the launcher: `12 km in miles`, `70 f to c`,
//! `2 go en mo`. Length, mass, temperature, volume, data, time and speed;
//! "in", "to" and the French "en" between the two units.

/// A unit: its kind, and how to go to and from the kind's base.
struct Unit {
    names: &'static [&'static str],
    kind: Kind,
    /// Base per unit; temperatures use `offset` too.
    factor: f64,
    offset: f64,
    /// How it reads in the answer.
    shown: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Length,
    Mass,
    Temperature,
    Volume,
    Data,
    Time,
    Speed,
}

const fn unit(
    names: &'static [&'static str],
    kind: Kind,
    factor: f64,
    shown: &'static str,
) -> Unit {
    Unit {
        names,
        kind,
        factor,
        offset: 0.0,
        shown,
    }
}

const UNITS: &[Unit] = &[
    unit(
        &[
            "mm",
            "millimetre",
            "millimetres",
            "millimeter",
            "millimeters",
        ],
        Kind::Length,
        0.001,
        "mm",
    ),
    unit(
        &[
            "cm",
            "centimetre",
            "centimetres",
            "centimeter",
            "centimeters",
        ],
        Kind::Length,
        0.01,
        "cm",
    ),
    unit(
        &["m", "metre", "metres", "meter", "meters", "mètre", "mètres"],
        Kind::Length,
        1.0,
        "m",
    ),
    unit(
        &[
            "km",
            "kilometre",
            "kilometres",
            "kilometer",
            "kilometers",
            "kilomètre",
            "kilomètres",
        ],
        Kind::Length,
        1000.0,
        "km",
    ),
    unit(
        &["in", "inch", "inches", "pouce", "pouces"],
        Kind::Length,
        0.0254,
        "in",
    ),
    unit(
        &["ft", "foot", "feet", "pied", "pieds"],
        Kind::Length,
        0.3048,
        "ft",
    ),
    unit(&["yd", "yard", "yards"], Kind::Length, 0.9144, "yd"),
    unit(&["mi", "mile", "miles"], Kind::Length, 1609.344, "miles"),
    unit(
        &["g", "gram", "grams", "gramme", "grammes"],
        Kind::Mass,
        0.001,
        "g",
    ),
    unit(
        &[
            "kg",
            "kilogram",
            "kilograms",
            "kilogramme",
            "kilogrammes",
            "kilo",
            "kilos",
        ],
        Kind::Mass,
        1.0,
        "kg",
    ),
    unit(&["t", "tonne", "tonnes"], Kind::Mass, 1000.0, "t"),
    unit(
        &["oz", "ounce", "ounces", "once", "onces"],
        Kind::Mass,
        0.028_349_523_125,
        "oz",
    ),
    unit(
        &["lb", "lbs", "pound", "pounds", "livre", "livres"],
        Kind::Mass,
        0.453_592_37,
        "lb",
    ),
    Unit {
        names: &["c", "°c", "celsius"],
        kind: Kind::Temperature,
        factor: 1.0,
        offset: 273.15,
        shown: "°C",
    },
    Unit {
        names: &["f", "°f", "fahrenheit"],
        kind: Kind::Temperature,
        factor: 5.0 / 9.0,
        offset: 459.67 * 5.0 / 9.0,
        shown: "°F",
    },
    unit(&["k", "kelvin"], Kind::Temperature, 1.0, "K"),
    unit(
        &[
            "ml",
            "millilitre",
            "millilitres",
            "milliliter",
            "milliliters",
        ],
        Kind::Volume,
        0.001,
        "ml",
    ),
    unit(
        &["cl", "centilitre", "centilitres"],
        Kind::Volume,
        0.01,
        "cl",
    ),
    unit(
        &["l", "litre", "litres", "liter", "liters"],
        Kind::Volume,
        1.0,
        "l",
    ),
    unit(
        &["gal", "gallon", "gallons"],
        Kind::Volume,
        3.785_411_784,
        "gal",
    ),
    unit(
        &["b", "byte", "bytes", "octet", "octets", "o"],
        Kind::Data,
        1.0,
        "B",
    ),
    unit(
        &["kb", "ko", "kilobyte", "kilobytes"],
        Kind::Data,
        1e3,
        "kB",
    ),
    unit(
        &["mb", "mo", "megabyte", "megabytes"],
        Kind::Data,
        1e6,
        "MB",
    ),
    unit(
        &["gb", "go", "gigabyte", "gigabytes"],
        Kind::Data,
        1e9,
        "GB",
    ),
    unit(
        &["tb", "to", "terabyte", "terabytes"],
        Kind::Data,
        1e12,
        "TB",
    ),
    unit(&["kib", "kio"], Kind::Data, 1024.0, "KiB"),
    unit(&["mib", "mio"], Kind::Data, 1_048_576.0, "MiB"),
    unit(&["gib", "gio"], Kind::Data, 1_073_741_824.0, "GiB"),
    unit(
        &["s", "sec", "second", "seconds", "seconde", "secondes"],
        Kind::Time,
        1.0,
        "s",
    ),
    unit(&["min", "minute", "minutes"], Kind::Time, 60.0, "min"),
    unit(
        &["h", "hour", "hours", "heure", "heures"],
        Kind::Time,
        3600.0,
        "h",
    ),
    unit(
        &["d", "day", "days", "jour", "jours"],
        Kind::Time,
        86400.0,
        "days",
    ),
    unit(&["km/h", "kmh", "kph"], Kind::Speed, 1.0 / 3.6, "km/h"),
    unit(&["m/s"], Kind::Speed, 1.0, "m/s"),
    unit(&["mph"], Kind::Speed, 0.447_04, "mph"),
];

fn find(name: &str) -> Option<&'static Unit> {
    let name = name.trim().to_lowercase();
    UNITS.iter().find(|u| u.names.contains(&name.as_str()))
}

/// The conversion `input` asks for, as it reads (`7.46 miles`), and the
/// bare number to copy.
pub fn convert(input: &str) -> Option<(String, String)> {
    let text = input.trim();
    // The number first, its unit after: "12 km in miles", "12km to mi".
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-'))
        .filter(|&i| i > 0)?;
    let value: f64 = text[..split].replace(',', ".").parse().ok()?;
    let rest = text[split..].trim_start();
    let lower = rest.to_lowercase();
    let (from, to) = [" in ", " to ", " en ", " → ", " -> "]
        .iter()
        .find_map(|sep| {
            lower
                .find(sep)
                .map(|i| (&rest[..i], &rest[i + sep.len()..]))
        })?;
    let (from, to) = (find(from)?, find(to)?);
    if from.kind != to.kind {
        return None;
    }
    let base = value * from.factor + from.offset;
    let result = (base - to.offset) / to.factor;
    let number = format(result);
    Some((format!("{number} {}", to.shown), number))
}

/// Two decimals at most, no trailing zeros.
fn format(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(q: &str) -> Option<String> {
        convert(q).map(|(s, _)| s)
    }

    #[test]
    fn lengths_masses_and_data_convert() {
        assert_eq!(shown("12 km in miles").as_deref(), Some("7.46 miles"));
        assert_eq!(shown("1 mile to km").as_deref(), Some("1.61 km"));
        assert_eq!(shown("2 lb en kg").as_deref(), Some("0.91 kg"));
        assert_eq!(shown("1 gib to mb").as_deref(), Some("1073.74 MB"));
        assert_eq!(shown("90 min in h").as_deref(), Some("1.5 h"));
        assert_eq!(shown("100 km/h in mph").as_deref(), Some("62.14 mph"));
    }

    #[test]
    fn temperatures_have_offsets() {
        assert_eq!(shown("100 c in f").as_deref(), Some("212 °F"));
        assert_eq!(shown("32 f to c").as_deref(), Some("0 °C"));
        assert_eq!(shown("0 k en c").as_deref(), Some("-273.15 °C"));
    }

    #[test]
    fn nonsense_is_left_alone() {
        assert_eq!(convert("12 km in kg"), None);
        assert_eq!(convert("firefox"), None);
        assert_eq!(convert("12 parsecs in miles"), None);
        assert_eq!(convert("km in miles"), None);
    }
}
