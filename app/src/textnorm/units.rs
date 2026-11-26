//! Unit symbols ("10 m", "3 s", "km/h", "m²") and Greek letters ("τ"), spelled in French.

pub struct UnitDef {
    abbr: &'static str,
    singular: &'static str,
    plural: &'static str,
    feminine: bool,
    /// Read even without a number before it ("en cm"). Off for symbols that are also words or
    /// letters ("m", "s", "Go").
    standalone: bool,
}

const fn u(abbr: &'static str, singular: &'static str, plural: &'static str) -> UnitDef {
    UnitDef { abbr, singular, plural, feminine: false, standalone: false }
}

const fn f(abbr: &'static str, singular: &'static str, plural: &'static str) -> UnitDef {
    UnitDef { abbr, singular, plural, feminine: true, standalone: false }
}

const fn alone(def: UnitDef) -> UnitDef {
    UnitDef { standalone: true, ..def }
}

const UNITS: &[UnitDef] = &[
    alone(u("nm", "nanomètre", "nanomètres")),
    alone(u("µm", "micromètre", "micromètres")),
    alone(u("μm", "micromètre", "micromètres")),
    alone(u("mm", "millimètre", "millimètres")),
    alone(u("cm", "centimètre", "centimètres")),
    alone(u("dm", "décimètre", "décimètres")),
    u("m", "mètre", "mètres"),
    alone(u("km", "kilomètre", "kilomètres")),
    alone(u("mg", "milligramme", "milligrammes")),
    u("g", "gramme", "grammes"),
    alone(u("kg", "kilogramme", "kilogrammes")),
    f("t", "tonne", "tonnes"),
    alone(f("ns", "nanoseconde", "nanosecondes")),
    alone(f("µs", "microseconde", "microsecondes")),
    alone(f("μs", "microseconde", "microsecondes")),
    alone(f("ms", "milliseconde", "millisecondes")),
    f("s", "seconde", "secondes"),
    f("min", "minute", "minutes"),
    f("h", "heure", "heures"),
    u("j", "jour", "jours"),
    alone(u("mL", "millilitre", "millilitres")),
    alone(u("ml", "millilitre", "millilitres")),
    alone(u("cL", "centilitre", "centilitres")),
    alone(u("cl", "centilitre", "centilitres")),
    alone(u("dL", "décilitre", "décilitres")),
    alone(u("dl", "décilitre", "décilitres")),
    u("L", "litre", "litres"),
    u("l", "litre", "litres"),
    alone(u("Hz", "hertz", "hertz")),
    alone(u("kHz", "kilohertz", "kilohertz")),
    alone(u("MHz", "mégahertz", "mégahertz")),
    alone(u("GHz", "gigahertz", "gigahertz")),
    u("W", "watt", "watts"),
    alone(u("kW", "kilowatt", "kilowatts")),
    u("MW", "mégawatt", "mégawatts"),
    u("Wh", "wattheure", "wattheures"),
    alone(u("kWh", "kilowattheure", "kilowattheures")),
    u("J", "joule", "joules"),
    alone(u("kJ", "kilojoule", "kilojoules")),
    f("cal", "calorie", "calories"),
    alone(f("kcal", "kilocalorie", "kilocalories")),
    u("V", "volt", "volts"),
    alone(u("mV", "millivolt", "millivolts")),
    alone(u("kV", "kilovolt", "kilovolts")),
    alone(u("mA", "milliampère", "milliampères")),
    alone(u("mAh", "milliampère-heure", "milliampères-heures")),
    u("Ω", "ohm", "ohms"),
    alone(u("kΩ", "kilo-ohm", "kilo-ohms")),
    alone(u("MΩ", "mégohm", "mégohms")),
    u("Pa", "pascal", "pascals"),
    alone(u("hPa", "hectopascal", "hectopascals")),
    alone(u("kPa", "kilopascal", "kilopascals")),
    u("bar", "bar", "bars"),
    alone(u("mbar", "millibar", "millibars")),
    u("N", "newton", "newtons"),
    u("ko", "kilo-octet", "kilo-octets"),
    u("Ko", "kilo-octet", "kilo-octets"),
    u("Mo", "mégaoctet", "mégaoctets"),
    u("Go", "gigaoctet", "gigaoctets"),
    u("To", "téraoctet", "téraoctets"),
    u("KB", "kilo-octet", "kilo-octets"),
    u("MB", "mégaoctet", "mégaoctets"),
    u("GB", "gigaoctet", "gigaoctets"),
    u("TB", "téraoctet", "téraoctets"),
    u("kb", "kilobit", "kilobits"),
    u("Mb", "mégabit", "mégabits"),
    u("Gb", "gigabit", "gigabits"),
    alone(u("px", "pixel", "pixels")),
    alone(u("dB", "décibel", "décibels")),
    u("ha", "hectare", "hectares"),
    u("tr", "tour", "tours"),
    u("°", "degré", "degrés"),
    u("°C", "degré Celsius", "degrés Celsius"),
    u("°F", "degré Fahrenheit", "degrés Fahrenheit"),
    alone(u("Md", "milliard", "milliards")),
    alone(u("Mds", "milliard", "milliards")),
];

/// A unit as written: "m", "m²", "km/h", "m/s²".
pub struct Unit {
    def: &'static UnitDef,
    power: Option<u8>,
    per: Option<(&'static UnitDef, Option<u8>)>,
}

impl Unit {
    pub fn feminine(&self) -> bool {
        self.def.feminine
    }

    pub fn standalone(&self) -> bool {
        self.def.standalone || self.power.is_some() || self.per.is_some()
    }

    pub fn starts_with_vowel(&self) -> bool {
        self.def.singular.starts_with(['a', 'e', 'é', 'i', 'o', 'u'])
    }

    pub fn spoken(&self, plural: bool) -> String {
        let mut out = (if plural { self.def.plural } else { self.def.singular }).to_string();
        if let Some(power) = self.power {
            out.push(' ');
            out.push_str(power_word(power, self.def.feminine, plural));
        }
        if let Some((per, power)) = self.per {
            out.push_str(" par ");
            out.push_str(per.singular);
            if let Some(power) = power {
                out.push(' ');
                out.push_str(power_word(power, per.feminine, false));
            }
        }
        out
    }
}

fn power_word(power: u8, feminine: bool, plural: bool) -> &'static str {
    match (power, feminine, plural) {
        (2, false, false) => "carré",
        (2, false, true) => "carrés",
        (2, true, false) => "carrée",
        (2, true, true) => "carrées",
        (_, _, false) => "cube",
        (_, _, true) => "cubes",
    }
}

fn lookup(abbr: &str) -> Option<&'static UnitDef> {
    UNITS.iter().find(|d| d.abbr == abbr)
}

/// Reads one unit symbol at `start`: "°" then letters, or letters alone.
fn read_symbol(chars: &[char], start: usize) -> Option<(&'static UnitDef, usize)> {
    let mut j = start;
    if chars.get(j) == Some(&'°') {
        j += 1;
    }
    while chars.get(j).is_some_and(|c| c.is_alphabetic()) {
        j += 1;
    }
    if j == start {
        return None;
    }
    let abbr: String = chars[start..j].iter().collect();
    lookup(&abbr).map(|def| (def, j))
}

/// "²", "³", or "2"/"3" right after a length ("m2"), when nothing alphanumeric follows.
fn read_power(chars: &[char], j: usize, def: &UnitDef) -> (Option<u8>, usize) {
    match chars.get(j) {
        Some('²') => (Some(2), j + 1),
        Some('³') => (Some(3), j + 1),
        Some(c @ ('2' | '3'))
            if def.singular.ends_with("mètre") && !chars.get(j + 1).is_some_and(|c| c.is_alphanumeric()) =>
        {
            (Some(if *c == '2' { 2 } else { 3 }), j + 1)
        }
        _ => (None, j),
    }
}

/// Reads a unit at `start`. Returns it and the index just past it, only when it ends on a word
/// boundary: "m" in "10 m." but not in "10 m'a" or "10 mai".
pub fn read_unit(chars: &[char], start: usize) -> Option<(Unit, usize)> {
    let (def, j) = read_symbol(chars, start)?;
    let (power, mut j) = read_power(chars, j, def);
    let mut per = None;
    if chars.get(j) == Some(&'/')
        && let Some((per_def, k)) = read_symbol(chars, j + 1)
    {
        let (per_power, k) = read_power(chars, k, per_def);
        per = Some((per_def, per_power));
        j = k;
    }
    let boundary = chars
        .get(j)
        .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '\'' | '’'));
    boundary.then_some((Unit { def, power, per }, j))
}

/// French name of a Greek letter, lower or upper case. "µ" (micro sign) reads as "mu".
pub fn greek_name(c: char) -> Option<&'static str> {
    let lower = c.to_lowercase().next().unwrap_or(c);
    Some(match lower {
        'α' => "alpha",
        'β' => "bêta",
        'γ' => "gamma",
        'δ' => "delta",
        'ε' | 'ϵ' => "epsilon",
        'ζ' => "dzêta",
        'η' => "êta",
        'θ' | 'ϑ' => "thêta",
        'ι' => "iota",
        'κ' => "kappa",
        'λ' => "lambda",
        'μ' | 'µ' => "mu",
        'ν' => "nu",
        'ξ' => "ksi",
        'ο' => "omicron",
        'π' => "pi",
        'ρ' => "rhô",
        'σ' | 'ς' => "sigma",
        'τ' => "tau",
        'υ' => "upsilon",
        'φ' | 'ϕ' => "phi",
        'χ' => "khi",
        'ψ' => "psi",
        'ω' => "oméga",
        _ => return None,
    })
}
