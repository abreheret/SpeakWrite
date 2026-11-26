//! French abbreviations: texting ("bcp", "svp"), titles ("Mme", "M. Dupont") and dotted forms
//! ("c.-à-d.", "p. ex.", "n°"). Ambiguous ones are left out: "ms", "mm", "ns" are units, "ex"
//! is also a noun ("mon ex"), "vol." ends sentences ("un vol.").

/// Lowercase written form, spoken form, and whether the all-caps form counts too ("SVP").
/// "Svp" (capitalized) always counts and reads "S'il vous plaît".
const WORDS: &[(&str, &str, bool)] = &[
    ("bcp", "beaucoup", true),
    ("svp", "s'il vous plaît", true),
    ("stp", "s'il te plaît", true),
    ("rdv", "rendez-vous", true),
    ("mdr", "mort de rire", true),
    ("ptdr", "pété de rire", true),
    ("jsp", "je sais pas", true),
    ("tkt", "t'inquiète", true),
    ("dsl", "désolé", true),
    ("jtm", "je t'aime", true),
    ("jpp", "j'en peux plus", true),
    ("osef", "on s'en fout", true),
    ("cdt", "cordialement", true),
    ("cdlt", "cordialement", true),
    ("qqch", "quelque chose", true),
    ("qqn", "quelqu'un", true),
    ("qq", "quelques", false),
    ("qqes", "quelques", false),
    ("qqf", "quelquefois", false),
    ("slt", "salut", false),
    ("bjr", "bonjour", false),
    ("bsr", "bonsoir", false),
    ("pcq", "parce que", false),
    ("pck", "parce que", false),
    ("psk", "parce que", false),
    ("tjs", "toujours", false),
    ("tjrs", "toujours", false),
    ("tjr", "toujours", false),
    ("jms", "jamais", false),
    ("pr", "pour", false),
    ("ds", "dans", false),
    ("dc", "donc", false),
    ("tt", "tout", false),
    ("tte", "toute", false),
    ("ts", "tous", false),
    ("ttes", "toutes", false),
    ("pdt", "pendant", false),
    ("avc", "avec", false),
    ("qd", "quand", false),
    ("qdm", "quand même", false),
    ("mtn", "maintenant", false),
    ("mnt", "maintenant", false),
    ("auj", "aujourd'hui", false),
    ("ajd", "aujourd'hui", false),
    ("dmn", "demain", false),
    ("pb", "problème", false),
    ("pbs", "problèmes", false),
    ("pbm", "problème", false),
    ("msg", "message", false),
    ("msgs", "messages", false),
    ("pk", "pourquoi", false),
    ("pq", "pourquoi", false),
    ("tlm", "tout le monde", false),
    ("vrmt", "vraiment", false),
    ("dac", "d'accord", false),
    ("dacc", "d'accord", false),
    ("ptet", "peut-être", false),
    ("srx", "sérieux", false),
    ("oklm", "au calme", false),
    ("cad", "c'est-à-dire", false),
    ("càd", "c'est-à-dire", false),
    ("nbr", "nombre", false),
    ("nbre", "nombre", false),
    ("qté", "quantité", false),
    ("tps", "temps", false),
    ("vs", "versus", false),
    ("cf", "confer", false),
    ("etc", "et cetera", false),
];

/// Titles and other case-sensitive forms. A dot after them is dropped before a capitalized
/// name ("Dr. House"); `dot_required`: only "M." and "MM.", since "M" alone is a letter.
const TITLES: &[(&str, &str, bool)] = &[
    ("M", "Monsieur", true),
    ("MM", "Messieurs", true),
    ("Mme", "Madame", false),
    ("Mmes", "Mesdames", false),
    ("Mlle", "Mademoiselle", false),
    ("Mlles", "Mesdemoiselles", false),
    ("Dr", "Docteur", false),
    ("Dre", "Docteure", false),
    ("Pr", "Professeur", false),
    ("Mgr", "Monseigneur", false),
    ("St", "Saint", false),
    ("Ste", "Sainte", false),
    ("Sté", "Société", false),
    ("Cie", "Compagnie", false),
    ("NB", "nota bene", false),
    ("RAS", "rien à signaler", false),
];

/// Forms with dots, dashes or spaces, matched as written (first letter in either case).
const DOTTED: &[(&str, &str)] = &[
    ("av. J.-C.", "avant Jésus-Christ"),
    ("apr. J.-C.", "après Jésus-Christ"),
    ("J.-C.", "Jésus-Christ"),
    ("c.-à-d.", "c'est-à-dire"),
    ("c.-à-d", "c'est-à-dire"),
    ("c-à-d", "c'est-à-dire"),
    ("c.à.d.", "c'est-à-dire"),
    ("p. ex.", "par exemple"),
    ("p.ex.", "par exemple"),
    ("e.g.", "par exemple"),
    ("i.e.", "c'est-à-dire"),
    ("N.B.", "nota bene"),
    ("P.-S.", "post-scriptum"),
    ("P.S.", "post-scriptum"),
    ("cf.", "confer"),
    ("env.", "environ"),
    ("tél.", "téléphone"),
    ("réf.", "référence"),
    ("approx.", "approximativement"),
    ("hab.", "habitants"),
    ("chap.", "chapitre"),
    ("fig.", "figure"),
    ("n°", "numéro"),
    ("H/M", "homme-mois"),
];

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Part of a path, file name or identifier: "index.ts", "src/pr", "my_ds".
fn in_identifier(chars: &[char], start: usize, end: usize) -> bool {
    let before = start.checked_sub(1).and_then(|k| chars.get(k));
    let joined = |c: &char| matches!(c, '.' | '/' | '\\' | '_' | '@');
    before.is_some_and(joined)
        || chars.get(end).is_some_and(|c| matches!(c, '/' | '\\' | '_' | '@'))
        || (chars.get(end) == Some(&'.') && chars.get(end + 1).is_some_and(|c| c.is_alphanumeric()))
}

/// Whitespace then a capital letter: the dot after "M" or "Dr" is not a sentence end.
fn name_follows(chars: &[char], j: usize) -> bool {
    chars.get(j).is_some_and(|c| *c == ' ' || *c == '\u{a0}')
        && chars.get(j + 1).is_some_and(|c| c.is_uppercase())
}

/// Reads a dotted abbreviation at `start`, or "p. 12" / "pp. 12-15" (page numbers). Returns the
/// spoken words and the index past the abbreviation. A dot that ended the sentence is kept.
pub fn read_dotted(chars: &[char], start: usize) -> Option<(String, usize)> {
    if !chars[start].is_alphabetic() || start.checked_sub(1).is_some_and(|k| chars[k].is_alphanumeric()) {
        return None;
    }
    for (prefix, spoken) in [("pp. ", "pages"), ("p. ", "page")] {
        let len = prefix.chars().count();
        if prefix.chars().eq(chars.iter().skip(start).take(len).copied())
            && chars.get(start + len).is_some_and(char::is_ascii_digit)
        {
            return Some((spoken.to_string(), start + len - 1));
        }
    }
    let (written, spoken, end) = DOTTED
        .iter()
        .filter_map(|(written, spoken)| {
            let pattern: Vec<char> = written.chars().collect();
            let end = start + pattern.len();
            let source = chars.get(start..end)?;
            let same = source[0].to_lowercase().eq(pattern[0].to_lowercase()) && source[1..] == pattern[1..];
            let boundary = written.ends_with('°') || chars.get(end).is_none_or(|c| !c.is_alphanumeric());
            (same && boundary).then_some((*written, *spoken, end))
        })
        .max_by_key(|(_, _, end)| *end)?;
    let mut out = if chars[start].is_uppercase() && !written.starts_with(char::is_uppercase) {
        capitalize(spoken)
    } else {
        spoken.to_string()
    };
    let next = chars[end..].iter().find(|c| **c != ' ' && **c != '\u{a0}');
    if written.ends_with('.') && next.is_some_and(|c| c.is_uppercase()) {
        out.push('.');
    }
    Some((out, end))
}

/// Reads `token`, a whole word ending at `end`. Returns the spoken words and the index past
/// what was read (past the dot of "M." or "Dr.").
pub fn read_word(token: &str, chars: &[char], end: usize) -> Option<(String, usize)> {
    let start = end - token.chars().count();
    let identifier = in_identifier(chars, start, end);
    if let Some((_, spoken, dot_required)) = TITLES.iter().find(|(written, _, _)| *written == token) {
        if chars.get(end) == Some(&'.') && name_follows(chars, end + 1) {
            return Some((spoken.to_string(), end + 1));
        }
        return (!dot_required && !identifier).then(|| (spoken.to_string(), end));
    }
    if identifier {
        return None;
    }
    WORDS.iter().find_map(|(written, spoken, caps)| {
        if token == *written {
            Some(spoken.to_string())
        } else if token == capitalize(written) {
            Some(capitalize(spoken))
        } else if *caps && token == written.to_uppercase() {
            Some(spoken.to_string())
        } else {
            None
        }
        .map(|spoken| (spoken, end))
    })
}
