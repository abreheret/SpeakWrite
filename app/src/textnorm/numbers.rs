//! French number spelling: cardinals, ordinals, decimals, digit strings.

const UNITS: [&str; 17] = [
    "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze",
    "douze", "treize", "quatorze", "quinze", "seize",
];
const TENS: [&str; 7] = ["", "dix", "vingt", "trente", "quarante", "cinquante", "soixante"];

/// Largest value spelled as a number; longer digit strings are read digit by digit.
pub const MAX_CARDINAL: u64 = 999_999_999_999;

/// `n` in 0..100. `final_word`: "quatre-vingts" takes an "s" only when nothing follows
/// (or a noun like "millions" follows).
fn below_100(n: u64, final_word: bool) -> String {
    match n {
        0..=16 => UNITS[n as usize].to_string(),
        17..=19 => format!("dix-{}", UNITS[(n - 10) as usize]),
        20..=69 => {
            let (t, u) = ((n / 10) as usize, n % 10);
            match u {
                0 => TENS[t].to_string(),
                1 => format!("{} et un", TENS[t]),
                _ => format!("{}-{}", TENS[t], UNITS[u as usize]),
            }
        }
        71 => "soixante et onze".to_string(),
        70..=79 => format!("soixante-{}", below_100(n - 60, final_word)),
        80 if final_word => "quatre-vingts".to_string(),
        80 => "quatre-vingt".to_string(),
        81..=99 => format!("quatre-vingt-{}", below_100(n - 80, final_word)),
        _ => unreachable!("below_100 called with {n}"),
    }
}

/// `n` in 1..1000. Same meaning for `final_word` as in [`below_100`], applied to "cents".
fn below_1000(n: u64, final_word: bool) -> String {
    let (h, r) = (n / 100, n % 100);
    let mut out = match h {
        0 => String::new(),
        1 => "cent".to_string(),
        _ => format!("{} cent", UNITS[h as usize]),
    };
    if r == 0 {
        if h > 1 && final_word {
            out.push('s');
        }
    } else {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&below_100(r, final_word));
    }
    out
}

/// Spells a cardinal number in French, e.g. `2026` -> "deux mille vingt-six".
/// Values above [`MAX_CARDINAL`] are read digit by digit.
pub fn cardinal(n: u64) -> String {
    if n == 0 {
        return UNITS[0].to_string();
    }
    if n > MAX_CARDINAL {
        return digits(&n.to_string());
    }
    let billions = n / 1_000_000_000;
    let millions = (n / 1_000_000) % 1000;
    let thousands = (n / 1000) % 1000;
    let rest = n % 1000;

    let mut parts: Vec<String> = Vec::new();
    for (value, noun) in [(billions, "milliard"), (millions, "million")] {
        if value > 0 {
            // Nouns: "deux cents millions" keeps the plural "s".
            let plural = if value > 1 { "s" } else { "" };
            parts.push(format!("{} {noun}{plural}", below_1000(value, true)));
        }
    }
    match thousands {
        0 => {}
        // "mille", never "un mille"; "cent"/"vingt" stay singular before "mille".
        1 => parts.push("mille".to_string()),
        _ => parts.push(format!("{} mille", below_1000(thousands, false))),
    }
    if rest > 0 {
        parts.push(below_1000(rest, true));
    }
    parts.join(" ")
}

/// Grammatical gender, for "premier" / "première".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gender {
    Masculine,
    Feminine,
}

/// Spells an ordinal number in French, e.g. `21` -> "vingt et unième".
pub fn ordinal(n: u64, gender: Gender) -> String {
    if n == 1 {
        return match gender {
            Gender::Masculine => "premier",
            Gender::Feminine => "première",
        }
        .to_string();
    }
    let words = cardinal(n);
    let split = words.rfind([' ', '-']).map_or(0, |i| i + 1);
    let (head, last) = words.split_at(split);
    let last = match last {
        "un" => "unième".to_string(),
        "cinq" => "cinquième".to_string(),
        "neuf" => "neuvième".to_string(),
        w if w.ends_with('e') => format!("{}ième", &w[..w.len() - 1]),
        // "quatre-vingts", "deux cents", "millions", "milliards"
        w if w.ends_with('s') && w.len() > 1 => format!("{}ième", &w[..w.len() - 1]),
        w => format!("{w}ième"),
    };
    format!("{head}{last}")
}

/// Reads a string of ASCII digits one by one: "06" -> "zéro six".
pub fn digits(s: &str) -> String {
    s.chars()
        .filter_map(|c| c.to_digit(10))
        .map(|d| UNITS[d as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads an integer written with ASCII digits, keeping leading zeros audible:
/// "2026" -> "deux mille vingt-six", "007" -> "zéro zéro sept".
pub fn integer_str(s: &str) -> String {
    let trimmed = s.trim_start_matches('0');
    let zeros = s.len() - trimmed.len();
    if trimmed.is_empty() {
        return digits(s);
    }
    let rest = match trimmed.parse::<u64>() {
        Ok(n) if n <= MAX_CARDINAL => cardinal(n),
        _ => digits(trimmed),
    };
    if zeros == 0 {
        rest
    } else {
        format!("{} {rest}", digits(&s[..zeros]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_numbers() {
        let cases = [
            (0, "zéro"),
            (1, "un"),
            (16, "seize"),
            (17, "dix-sept"),
            (20, "vingt"),
            (21, "vingt et un"),
            (22, "vingt-deux"),
            (31, "trente et un"),
            (61, "soixante et un"),
            (70, "soixante-dix"),
            (71, "soixante et onze"),
            (72, "soixante-douze"),
            (77, "soixante-dix-sept"),
            (80, "quatre-vingts"),
            (81, "quatre-vingt-un"),
            (90, "quatre-vingt-dix"),
            (91, "quatre-vingt-onze"),
            (99, "quatre-vingt-dix-neuf"),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(n), expected, "n = {n}");
        }
    }

    #[test]
    fn hundreds_and_more() {
        let cases = [
            (100, "cent"),
            (101, "cent un"),
            (200, "deux cents"),
            (201, "deux cent un"),
            (280, "deux cent quatre-vingts"),
            (1000, "mille"),
            (1001, "mille un"),
            (1990, "mille neuf cent quatre-vingt-dix"),
            (2026, "deux mille vingt-six"),
            (80_000, "quatre-vingt mille"),
            (200_000, "deux cent mille"),
            (12_345, "douze mille trois cent quarante-cinq"),
            (1_000_000, "un million"),
            (2_000_000, "deux millions"),
            (200_000_000, "deux cents millions"),
            (3_200_000, "trois millions deux cent mille"),
            (1_000_000_000, "un milliard"),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(n), expected, "n = {n}");
        }
    }

    #[test]
    fn ordinals() {
        assert_eq!(ordinal(1, Gender::Masculine), "premier");
        assert_eq!(ordinal(1, Gender::Feminine), "première");
        assert_eq!(ordinal(2, Gender::Masculine), "deuxième");
        assert_eq!(ordinal(4, Gender::Masculine), "quatrième");
        assert_eq!(ordinal(5, Gender::Masculine), "cinquième");
        assert_eq!(ordinal(9, Gender::Masculine), "neuvième");
        assert_eq!(ordinal(21, Gender::Masculine), "vingt et unième");
        assert_eq!(ordinal(80, Gender::Masculine), "quatre-vingtième");
        assert_eq!(ordinal(100, Gender::Masculine), "centième");
        assert_eq!(ordinal(1000, Gender::Masculine), "millième");
    }

    #[test]
    fn digit_strings() {
        assert_eq!(integer_str("06"), "zéro six");
        assert_eq!(integer_str("007"), "zéro zéro sept");
        assert_eq!(integer_str("0"), "zéro");
        assert_eq!(integer_str("2026"), "deux mille vingt-six");
        assert_eq!(integer_str("1234567890123"), digits("1234567890123"));
    }
}
