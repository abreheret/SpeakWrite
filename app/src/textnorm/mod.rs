//! Text preparation before speech synthesis (French).
//!
//! Some TTS models (Pocket TTS) garble digits and symbols: "F9", "2026" or "3,5 %" must be
//! spelled out. Acronyms are left as written: "SNCF" reads fine, while "S N C F" or "S.N.C.F."
//! do not (measured in spikes, see README).

mod abbrev;
mod numbers;
mod units;

use std::collections::BTreeMap;

use numbers::{Gender, cardinal, integer_str, ordinal};
use units::{greek_name, read_unit};

pub struct Normalizer {
    /// Whole-word replacements from the user config, matched case-sensitively.
    pronunciation: BTreeMap<String, String>,
}

impl Normalizer {
    pub fn new(pronunciation: BTreeMap<String, String>) -> Self {
        Self { pronunciation }
    }

    /// Prepares `text` for the model of `language`. Only French is spelled out: the other
    /// models read digits and symbols themselves (checked for English).
    pub fn normalize(&self, text: &str, language: &str) -> String {
        if crate::language::base(language) == "french" { self.french(text) } else { plain(text) }
    }

    fn french(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len() + text.len() / 4);
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_digit() || is_minus_sign(&chars, i) {
                let (words, consumed) = read_number(&chars, i);
                push_words(&mut out, &words, chars.get(i + consumed).copied());
                i += consumed;
            } else if let Some((spoken, end)) = abbrev::read_dotted(&chars, i) {
                push_words(&mut out, &spoken, chars.get(end).copied());
                i = end;
            } else if c.is_alphabetic() {
                let start = i;
                while i < chars.len() && chars[i].is_alphanumeric() {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                if let Some(spoken) = self.pronunciation.get(&token) {
                    push_words(&mut out, spoken, chars.get(i).copied());
                } else if let Some((spoken, end)) = abbrev::read_word(&token, &chars, i) {
                    push_words(&mut out, &spoken, chars.get(end).copied());
                    i = end;
                } else if let Some((unit, end)) =
                    read_unit(&chars, start).filter(|(unit, _)| unit.standalone())
                {
                    let plural = !after_singular_determiner(&out);
                    push_words(&mut out, &unit.spoken(plural), chars.get(end).copied());
                    i = end;
                } else {
                    let spoken = self.read_word(&token);
                    push_words(&mut out, &spoken, chars.get(i).copied());
                }
            } else {
                match c {
                    // Markdown decorations, as in "**gras**" or "`code`".
                    '*' | '`' | '#' => {}
                    '_' => out.push(' '),
                    '&' => push_words(&mut out, "et", chars.get(i + 1).copied()),
                    '%' => push_words(&mut out, "pour cent", chars.get(i + 1).copied()),
                    '€' => push_words(&mut out, "euros", chars.get(i + 1).copied()),
                    '=' if !next_to_operator(&chars, i) => {
                        push_words(&mut out, "égale", chars.get(i + 1).copied())
                    }
                    '≠' => push_words(&mut out, "différent de", chars.get(i + 1).copied()),
                    '≈' => push_words(&mut out, "environ égal à", chars.get(i + 1).copied()),
                    '≤' => push_words(&mut out, "inférieur ou égal à", chars.get(i + 1).copied()),
                    '≥' => push_words(&mut out, "supérieur ou égal à", chars.get(i + 1).copied()),
                    '±' => push_words(&mut out, "plus ou moins", chars.get(i + 1).copied()),
                    '×' => push_words(&mut out, "fois", chars.get(i + 1).copied()),
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
        // Greek letters: "τ" -> "tau", "Δt" -> "delta t".
        if token.chars().any(|c| greek_name(c).is_some()) {
            let mut parts: Vec<String> = Vec::new();
            let mut run = String::new();
            for c in token.chars() {
                if let Some(name) = greek_name(c) {
                    if !run.is_empty() {
                        parts.push(self.read_word(&std::mem::take(&mut run)));
                    }
                    parts.push(name.to_string());
                } else {
                    run.push(c);
                }
            }
            if !run.is_empty() {
                parts.push(self.read_word(&run));
            }
            return parts.join(" ");
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

/// Markdown decorations and odd spaces removed, lines tidied: for languages other than French.
fn plain(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter_map(|c| match c {
            '*' | '`' | '#' => None,
            '_' | '\u{a0}' | '\u{202f}' | '\t' => Some(' '),
            c => Some(c),
        })
        .collect();
    tidy_lines(&cleaned)
}

/// "=" alone, not part of "==", "<=", "=>", ":=", "!=".
fn next_to_operator(chars: &[char], i: usize) -> bool {
    let is_op = |c: &char| matches!(c, '=' | '<' | '>' | '!' | ':' | '+' | '-' | '*' | '/');
    (i > 0 && is_op(&chars[i - 1])) || chars.get(i + 1).is_some_and(is_op)
}

/// A unit without a number reads in the singular after "au", "le", "par"... ("prix au m²"),
/// in the plural otherwise ("mesuré en cm").
fn after_singular_determiner(out: &str) -> bool {
    let last = out
        .trim_end()
        .trim_end_matches(['\'', '’'])
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("")
        .to_lowercase();
    matches!(last.as_str(), "au" | "le" | "l" | "un" | "une" | "du" | "par" | "chaque" | "ce" | "cet")
}

/// "un" -> "une" at the end of a cardinal, before a feminine unit: "vingt et une secondes".
fn feminine(words: &str) -> String {
    match words.strip_suffix("un") {
        Some(head) if head.is_empty() || head.ends_with([' ', '-']) => format!("{head}une"),
        _ => words.to_string(),
    }
}

fn with_decimal(words: String, decimal: &Option<(char, String)>) -> String {
    match decimal {
        Some((sep, dec)) => {
            let sep_word = if *sep == ',' { "virgule" } else { "point" };
            format!("{words} {sep_word} {}", integer_str(dec))
        }
        None => words,
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

    // Unit symbols: "10 m", "3 s", "50 km/h", "20 m²", "-5 °C".
    if unit.is_none()
        && let Some((symbol, end)) = read_unit(chars, unit_at)
    {
        let plural = int_digits.parse::<u64>().is_ok_and(|n| n >= 2) || int_digits.len() > 18;
        let mut words = integer_str(&int_digits);
        if symbol.feminine() {
            words = feminine(&words);
        }
        let big = ["million", "millions", "milliard", "milliards"].iter().any(|w| words.ends_with(w));
        let words = with_decimal(words, &decimal);
        let link = match (big && decimal.is_none(), symbol.starts_with_vowel()) {
            (false, _) => "",
            (true, true) => "d'",
            (true, false) => "de ",
        };
        return (format!("{prefix}{words} {link}{}", symbol.spoken(plural)), end - start);
    }

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
            let words = with_decimal(int_words, &decimal);
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
        Normalizer::new(dict).normalize(text, "french")
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

    #[test]
    fn units_after_numbers() {
        assert_eq!(norm("τ = 3 s"), "tau égale trois secondes.");
        assert_eq!(norm("Il a couru 10 m."), "Il a couru dix mètres.");
        assert_eq!(norm("Il a couru 10m."), "Il a couru dix mètres.");
        assert_eq!(norm("Un trait de 1 cm."), "Un trait de un centimètre.");
        assert_eq!(norm("Attendre 1 s, puis 21 s."), "Attendre une seconde, puis vingt et une secondes.");
        assert_eq!(norm("Latence de 1,5 ms."), "Latence de une virgule cinq milliseconde.");
        assert_eq!(norm("Il roule à 50 km/h."), "Il roule à cinquante kilomètres par heure.");
        assert_eq!(norm("g = 9,81 m/s²"), "g égale neuf virgule quatre-vingt-un mètres par seconde carrée.");
        assert_eq!(norm("Un appartement de 45 m²."), "Un appartement de quarante-cinq mètres carrés.");
        assert_eq!(norm("Un appartement de 45 m2."), "Un appartement de quarante-cinq mètres carrés.");
        assert_eq!(norm("Il fait -5 °C."), "Il fait moins cinq degrés Celsius.");
        assert_eq!(norm("Un angle de 90°."), "Un angle de quatre-vingt-dix degrés.");
        assert_eq!(norm("Un fichier de 3 Go."), "Un fichier de trois gigaoctets.");
        assert_eq!(norm("Pendant 2 min et 3 h."), "Pendant deux minutes et trois heures.");
        assert_eq!(norm("Il mesure 1 m 80."), "Il mesure un mètre quatre-vingts.");
        assert_eq!(norm("Un signal à 440 Hz."), "Un signal à quatre cent quarante hertz.");
        assert_eq!(norm("Il y a 2 000 000 m."), "Il y a deux millions de mètres.");
    }

    #[test]
    fn units_are_not_words() {
        assert_eq!(norm("Le 3 mai."), "Le trois mai.");
        assert_eq!(norm("Les 2 m'ont vu."), "Les deux m'ont vu.");
        assert_eq!(norm("Les 3 sont là."), "Les trois sont là.");
        assert_eq!(norm("Le mot m seul."), "Le mot m seul.");
    }

    #[test]
    fn units_without_numbers() {
        assert_eq!(norm("Mesuré en cm."), "Mesuré en centimètres.");
        assert_eq!(norm("Le prix au m²."), "Le prix au mètre carré.");
        assert_eq!(norm("Vitesse en km/h."), "Vitesse en kilomètres par heure.");
        assert_eq!(norm("Temps en ms."), "Temps en millisecondes.");
    }

    #[test]
    fn greek_letters_and_symbols() {
        assert_eq!(norm("τ = 3 s"), "tau égale trois secondes.");
        assert_eq!(norm("Soit Δt petit et λ fixé."), "Soit delta t petit et lambda fixé.");
        assert_eq!(norm("Une erreur de ± 2 %."), "Une erreur de plus ou moins deux pour cent.");
        assert_eq!(norm("Si a ≤ b."), "Si a inférieur ou égal à b.");
        assert_eq!(norm("Teste a == b."), "Teste a == b.");
        assert_eq!(norm("Une résistance de 10 kΩ."), "Une résistance de dix kilo-ohms.");
    }

    #[test]
    fn texting_abbreviations() {
        assert_eq!(norm("Merci bcp, svp réponds vite."), "Merci beaucoup, s'il vous plaît réponds vite.");
        assert_eq!(norm("Bcp de monde. SVP, aide-moi."), "Beaucoup de monde. s'il vous plaît, aide-moi.");
        assert_eq!(norm("tkt, on se voit pdt les vacances"), "t'inquiète, on se voit pendant les vacances.");
        assert_eq!(norm("Il y a qqn qui a qqch pr toi."), "Il y a quelqu'un qui a quelque chose pour toi.");
        assert_eq!(norm("Des pommes, des poires, etc."), "Des pommes, des poires, et cetera.");
    }

    #[test]
    fn titles_and_dotted_abbreviations() {
        assert_eq!(norm("M. Dupont et Mme Durand."), "Monsieur Dupont et Madame Durand.");
        assert_eq!(norm("Le Dr. House et le Pr Martin."), "Le Docteur House et le Professeur Martin.");
        assert_eq!(norm("Un fruit, c.-à-d. une pomme."), "Un fruit, c'est-à-dire une pomme.");
        assert_eq!(norm("Un fruit, p. ex. une pomme."), "Un fruit, par exemple une pomme.");
        assert_eq!(norm("Voir p. 12, cf. annexe."), "Voir page douze, confer annexe.");
        assert_eq!(norm("Le n° 5 et le n°7."), "Le numéro cinq et le numéro sept.");
        assert_eq!(norm("En 52 av. J.-C. Puis rien."), "En cinquante-deux avant Jésus-Christ. Puis rien.");
        assert_eq!(norm("Env. 3 km."), "Environ trois kilomètres.");
    }

    #[test]
    fn abbreviations_left_alone() {
        assert_eq!(norm("Le fichier index.ts et la PR 12."), "Le fichier index.ts et la PR douze.");
        assert_eq!(norm("Le mot M seul."), "Le mot M seul.");
        assert_eq!(norm("C'était mon ex."), "C'était mon ex.");
        assert_eq!(norm("Il dort pr."), "Il dort pour.");
    }

    #[test]
    fn other_languages_keep_digits_and_words() {
        let mut dict = BTreeMap::new();
        dict.insert("Kyutai".to_string(), "kiou-taï".to_string());
        let normalizer = Normalizer::new(dict);
        assert_eq!(
            normalizer.normalize("Press **F9**: 3.5% faster\tby Kyutai", "english"),
            "Press F9: 3.5% faster by Kyutai."
        );
        assert_eq!(
            normalizer.normalize("Title\n\nA paragraph.", "english_2026-10"),
            "Title.\nA paragraph."
        );
        assert_eq!(normalizer.normalize("F9", "french_24l"), "F neuf.");
    }
}
