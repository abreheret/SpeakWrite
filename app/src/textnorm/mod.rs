//! Text preparation before speech synthesis (French).
//!
//! Some TTS models (Pocket TTS) garble digits and symbols: "F9", "2026" or "3,5 %" must be
//! spelled out. Acronyms are left as written: "SNCF" reads fine, while "S N C F" or "S.N.C.F."
//! do not (measured in spikes, see README).

mod numbers;

use std::collections::BTreeMap;

use numbers::{Gender, cardinal, integer_str, ordinal};

pub struct Normalizer {
    /// Whole-word replacements from the user config, matched case-sensitively.
    pronunciation: BTreeMap<String, String>,
}

impl Normalizer {
    pub fn new(pronunciation: BTreeMap<String, String>) -> Self {
        Self { pronunciation }
    }

    pub fn normalize(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len() + text.len() / 4);
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_digit() || is_minus_sign(&chars, i) {
                let (words, consumed) = read_number(&chars, i);
                push_words(&mut out, &words, chars.get(i + consumed).copied());
                i += consumed;
            } else if c.is_alphabetic() {
                let start = i;
                while i < chars.len() && chars[i].is_alphanumeric() {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                let spoken = self.read_word(&token);
                push_words(&mut out, &spoken, chars.get(i).copied());
            } else {
                match c {
                    // Markdown decorations, as in "**gras**" or "`code`".
                    '*' | '`' | '#' => {}
                    '_' => out.push(' '),
                    '&' => push_words(&mut out, "et", chars.get(i + 1).copied()),
                    '%' => push_words(&mut out, "pour cent", chars.get(i + 1).copied()),
                    '€' => push_words(&mut out, "euros", chars.get(i + 1).copied()),
                    '\u{a0}' | '\u{202f}' | '\t' => out.push(' '),
                    _ => out.push(c),
                }
                i += 1;
            }
        }
        tidy_lines(&out)
    }

    fn read_word(&self, token: &str) -> String {
        if let Some(spoken) = self.pronunciation.get(token) {
            return spoken.clone();
        }
        if !token.chars().any(|c| c.is_ascii_digit()) {
            return token.to_string();
        }
        // Function keys: "F9" -> "F neuf".
        if let Some(n) = token.strip_prefix('F').and_then(|d| d.parse::<u64>().ok())
            && (1..=24).contains(&n)
            && token[1..].chars().all(|c| c.is_ascii_digit())
        {
            return format!("F {}", cardinal(n));
        }
        // Letters mixed with digits: "MP3" -> "MP trois".
        split_runs(token)
            .into_iter()
            .map(|run| {
                if run.chars().all(|c| c.is_ascii_digit()) {
                    integer_str(&run)
                } else {
                    run
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Appends spoken words, with spaces so they never glue to neighbouring letters or digits.
fn push_words(out: &mut String, words: &str, next: Option<char>) {
    if out.chars().last().is_some_and(char::is_alphanumeric) {
        out.push(' ');
    }
    out.push_str(words);
    if next.is_some_and(char::is_alphanumeric) {
        out.push(' ');
    }
}

/// Collapses blank runs and ends every line with punctuation, so that titles and list items
/// get a pause instead of running into the next sentence.
fn tidy_lines(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .map(|mut line| {
            if !line.ends_with(['.', '!', '?', ':', ';', ',', '…']) {
                line.push('.');
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_runs(token: &str) -> Vec<String> {
    let mut runs: Vec<String> = Vec::new();
    let mut last_is_digit: Option<bool> = None;
    for c in token.chars() {
        let is_digit = c.is_ascii_digit();
        if last_is_digit != Some(is_digit) {
            runs.push(String::new());
            last_is_digit = Some(is_digit);
        }
        runs.last_mut().expect("pushed above").push(c);
    }
    runs
}

fn is_minus_sign(chars: &[char], i: usize) -> bool {
    matches!(chars[i], '-' | '−')
        && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        && (i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == '(')
}

fn is_group_separator(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}')
}

fn take_digits(chars: &[char], mut j: usize) -> (String, usize) {
    let start = j;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    (chars[start..j].iter().collect(), j)
}

fn boundary_at(chars: &[char], j: usize) -> bool {
    chars.get(j).is_none_or(|c| !c.is_alphanumeric())
}

/// Reads a number starting at `start` (a digit or a minus sign). Returns the spoken words and
/// the number of chars consumed, including a trailing unit or suffix ("%", "€", "h", "e").
fn read_number(chars: &[char], start: usize) -> (String, usize) {
    let mut j = start;
    let negative = !chars[j].is_ascii_digit();
    if negative {
        j += 1;
    }
    let (mut int_digits, next) = take_digits(chars, j);
    j = next;

    // Thousands groups: "12 345". Not for "06 12 34" (phone numbers) or "2026 150".
    if int_digits.len() <= 3 && !int_digits.starts_with('0') {
        while chars.get(j).copied().is_some_and(is_group_separator)
            && (1..=3).all(|k| chars.get(j + k).is_some_and(char::is_ascii_digit))
            && !chars.get(j + 4).is_some_and(char::is_ascii_digit)
        {
            int_digits.extend(&chars[j + 1..j + 4]);
            j += 4;
        }
    }

    let prefix = if negative { "moins " } else { "" };

    // Times: "14h30", "8h".
    if !negative && int_digits.len() <= 2 && chars.get(j) == Some(&'h') {
        let (minutes, after) = take_digits(chars, j + 1);
        let hours: u64 = int_digits.parse().unwrap_or(99);
        let minutes_ok =
            minutes.is_empty() || (minutes.len() == 2 && minutes.parse::<u64>().is_ok_and(|m| m < 60));
        if hours <= 24 && minutes_ok && boundary_at(chars, after) {
            let hour_words = match hours {
                1 => "une heure".to_string(),
                0 => "zéro heure".to_string(),
                h => format!("{} heures", cardinal(h)),
            };
            let words = match minutes.parse::<u64>() {
                Ok(m) if m > 0 => format!("{hour_words} {}", cardinal(m)),
                _ => hour_words,
            };
            return (words, after - start);
        }
    }

    // Decimals: "3,5" -> "trois virgule cinq", "3.5" -> "trois point cinq".
    let mut decimal: Option<(char, String)> = None;
    if let Some(&sep) = chars.get(j)
        && (sep == ',' || sep == '.')
        && chars.get(j + 1).is_some_and(char::is_ascii_digit)
    {
        let (dec, after) = take_digits(chars, j + 1);
        decimal = Some((sep, dec));
        j = after;
    }

    // Ordinals: "1er", "1re", "2e", "3ème".
    if decimal.is_none() && !negative {
        let suffix_end = (j..chars.len())
            .find(|&k| !chars[k].is_alphabetic())
            .unwrap_or(chars.len());
        let suffix: String = chars[j..suffix_end].iter().collect();
        let n: u64 = int_digits.parse().unwrap_or(0);
        let gender = match suffix.as_str() {
            "er" if n == 1 => Some(Gender::Masculine),
            "re" | "ère" if n == 1 => Some(Gender::Feminine),
            "e" | "ème" | "eme" | "è" if n > 1 => Some(Gender::Masculine),
            _ => None,
        };
        if let Some(gender) = gender
            && int_digits.len() <= 12
        {
            return (ordinal(n, gender), suffix_end - start);
        }
    }

    // Units right after the number, optionally after one space: "3,5 %", "12 €".
    let unit_at = if chars.get(j).copied().is_some_and(is_group_separator) {
        j + 1
    } else {
        j
    };
    let unit = chars.get(unit_at).copied().filter(|c| matches!(c, '%' | '€' | '$'));
    let int_words = integer_str(&int_digits);
    let words = match (unit, &decimal) {
        (Some(currency @ ('€' | '$')), Some((',', cents))) if cents.len() == 2 => {
            let name = currency_name(currency, &int_digits);
            match cents.as_str() {
                "00" => format!("{int_words} {name}"),
                _ => format!("{int_words} {name} {}", integer_str(cents)),
            }
        }
        _ => {
            let mut words = int_words;
            if let Some((sep, dec)) = &decimal {
                let sep_word = if *sep == ',' { "virgule" } else { "point" };
                words = format!("{words} {sep_word} {}", integer_str(dec));
            }
            match unit {
                Some('%') => format!("{words} pour cent"),
                Some(c) => format!("{words} {}", currency_name(c, &int_digits)),
                None => words,
            }
        }
    };
    let consumed = if unit.is_some() { unit_at + 1 } else { j };
    (format!("{prefix}{words}"), consumed - start)
}

fn currency_name(currency: char, int_digits: &str) -> &'static str {
    let singular = matches!(int_digits.trim_start_matches('0'), "" | "1");
    match (currency, singular) {
        ('€', true) => "euro",
        ('€', false) => "euros",
        (_, true) => "dollar",
        (_, false) => "dollars",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(text: &str) -> String {
        let mut dict = BTreeMap::new();
        dict.insert("Kyutai".to_string(), "kiou-taï".to_string());
        Normalizer::new(dict).normalize(text)
    }

    #[test]
    fn function_keys_and_years() {
        assert_eq!(norm("Appuie sur F9."), "Appuie sur F neuf.");
        assert_eq!(norm("En 2026, tout change."), "En deux mille vingt-six, tout change.");
    }

    #[test]
    fn percents_decimals_currencies() {
        assert_eq!(norm("Hausse de 3,5 %."), "Hausse de trois virgule cinq pour cent.");
        assert_eq!(norm("Hausse de 3,5%."), "Hausse de trois virgule cinq pour cent.");
        assert_eq!(norm("Version 2.5 publiée."), "Version deux point cinq publiée.");
        assert_eq!(norm("Il coûte 3,50 €."), "Il coûte trois euros cinquante.");
        assert_eq!(norm("Il coûte 1 €."), "Il coûte un euro.");
        assert_eq!(norm("Il fait -5 degrés."), "Il fait moins cinq degrés.");
    }

    #[test]
    fn groups_times_ordinals() {
        assert_eq!(
            norm("Il y a 12 345 lignes."),
            "Il y a douze mille trois cent quarante-cinq lignes."
        );
        assert_eq!(
            norm("Appelle le 06 12 34 56 78."),
            "Appelle le zéro six douze trente-quatre cinquante-six soixante-dix-huit."
        );
        assert_eq!(norm("Rendez-vous à 14h30."), "Rendez-vous à quatorze heures trente.");
        assert_eq!(norm("Départ à 8h."), "Départ à huit heures.");
        assert_eq!(norm("Le 1er mai."), "Le premier mai.");
        assert_eq!(norm("La 1re fois."), "La première fois.");
        assert_eq!(norm("La 2e fois."), "La deuxième fois.");
    }

    #[test]
    fn words_acronyms_markdown() {
        assert_eq!(norm("Le train de la SNCF."), "Le train de la SNCF.");
        assert_eq!(norm("Les modèles Kyutai."), "Les modèles kiou-taï.");
        assert_eq!(norm("Un fichier MP3."), "Un fichier MP trois.");
        assert_eq!(norm("Du **gras** et du `code`."), "Du gras et du code.");
        assert_eq!(norm("Titre\n\nUn paragraphe."), "Titre.\nUn paragraphe.");
        assert_eq!(norm("Toi & moi."), "Toi et moi.");
    }
}
