//! Language of the text to read. Pocket TTS has one model per language: in "auto" mode the
//! text is split into runs of lines of one language, each read by its own model.

use lingua::Language as Lang;
use lingua::LanguageDetectorBuilder;

/// Config value of the language setting that detects the language of each text.
pub const AUTO: &str = "auto";

/// Languages with a Pocket TTS model: config name, detector code, menu label.
const LANGUAGES: [(&str, Lang, &str); 7] = [
    ("french", Lang::French, "Français"),
    ("english", Lang::English, "Anglais"),
    ("german", Lang::German, "Allemand"),
    ("spanish", Lang::Spanish, "Espagnol"),
    ("italian", Lang::Italian, "Italien"),
    ("portuguese", Lang::Portuguese, "Portugais"),
    ("dutch", Lang::Dutch, "Néerlandais"),
];

/// Lines with fewer letters take the language of their neighbours: too short to tell.
const MIN_LETTERS: usize = 20;

/// Below this probability, the detector's guess is ignored.
const MIN_CONFIDENCE: f64 = 0.7;

/// The language of a model name: variants such as "french_24l" or "english_2026-10" share it.
pub fn base(model: &str) -> &str {
    model.split('_').next().unwrap_or(model)
}

pub fn is_known(model: &str) -> bool {
    LANGUAGES.iter().any(|(name, ..)| *name == base(model))
}

/// "Français" for "french" or "french_24l"; other names as written.
pub fn label(model: &str) -> String {
    match LANGUAGES.iter().find(|(name, ..)| *name == base(model)) {
        Some((_, _, label)) if base(model) == model => label.to_string(),
        Some((_, _, label)) => format!("{label} ({model})"),
        None => model.to_string(),
    }
}

/// A run of consecutive lines in one language.
#[derive(Debug, PartialEq, Eq)]
pub struct Segment {
    /// Model name, as in the config.
    pub language: String,
    pub text: String,
}

pub struct LanguageDetector {
    detector: lingua::LanguageDetector,
    /// Model name for each detectable language; the first one is the fallback.
    models: Vec<(Lang, String)>,
}

impl LanguageDetector {
    /// `models`: the model names to choose from, the fallback first. Unknown names are
    /// ignored; with none left, everything is French.
    pub fn new(models: &[String]) -> Self {
        let mut known: Vec<(Lang, String)> = Vec::new();
        for model in models {
            let Some((_, lang, _)) = LANGUAGES.iter().find(|(name, ..)| *name == base(model)) else {
                continue;
            };
            if !known.iter().any(|(l, _)| l == lang) {
                known.push((*lang, model.clone()));
            }
        }
        if known.is_empty() {
            known.push((Lang::French, "french".to_string()));
        }
        let languages: Vec<Lang> = known.iter().map(|(lang, _)| *lang).collect();
        // Models load on first use, a few MB each.
        let detector = LanguageDetectorBuilder::from_languages(&languages).build();
        Self { detector, models: known }
    }

    fn model_of(&self, text: &str) -> Option<usize> {
        if self.models.len() == 1 {
            return Some(0);
        }
        let (lang, confidence) = *self.detector.compute_language_confidence_values(text).first()?;
        if confidence < MIN_CONFIDENCE {
            return None;
        }
        self.models.iter().position(|(l, _)| *l == lang)
    }

    /// The text to read with each model, for a language setting (`AUTO` or a model name).
    pub fn split(&self, setting: &str, text: &str) -> Vec<Segment> {
        if setting == AUTO {
            self.segments(text)
        } else {
            vec![Segment { language: setting.to_string(), text: text.to_string() }]
        }
    }

    /// Splits `text` into runs of lines in one language. Short lines (titles, list items)
    /// join the language of the next long line, or else the previous one.
    pub fn segments(&self, text: &str) -> Vec<Segment> {
        let lines: Vec<&str> = text.lines().collect();
        let detected: Vec<Option<usize>> = lines
            .iter()
            .map(|line| {
                let letters = line.chars().filter(|c| c.is_alphabetic()).count();
                if letters >= MIN_LETTERS { self.model_of(line) } else { None }
            })
            .collect();
        // Nothing long enough: the whole text is the best clue.
        let whole = || self.model_of(text).unwrap_or(0);
        let mut resolved = vec![None; lines.len()];
        let mut next = None;
        for i in (0..lines.len()).rev() {
            next = detected[i].or(next);
            resolved[i] = next;
        }
        let mut previous = None;
        for slot in &mut resolved {
            *slot = slot.or(previous);
            previous = *slot;
        }

        let mut segments: Vec<Segment> = Vec::new();
        for (line, model) in lines.iter().zip(resolved) {
            let language = &self.models[model.unwrap_or_else(whole)].1;
            match segments.last_mut() {
                Some(last) if last.language == *language => {
                    last.text.push('\n');
                    last.text.push_str(line);
                }
                _ => segments.push(Segment { language: language.clone(), text: line.to_string() }),
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector() -> LanguageDetector {
        LanguageDetector::new(&["french".to_string(), "english".to_string()])
    }

    fn languages(text: &str) -> Vec<String> {
        detector().segments(text).into_iter().map(|s| s.language).collect()
    }

    #[test]
    fn single_language() {
        assert_eq!(languages("Le train de 8 h 30 partira avec un léger retard."), ["french"]);
        assert_eq!(languages("The quick brown fox jumps over the lazy dog."), ["english"]);
        assert_eq!(languages("Press the button to start the recording."), ["english"]);
        assert_eq!(languages("Appuie sur le bouton pour lancer l'enregistrement."), ["french"]);
    }

    #[test]
    fn short_texts() {
        assert_eq!(languages("Hello, how are you?"), ["english"]);
        assert_eq!(languages("Bonjour, comment ça va ?"), ["french"]);
        assert_eq!(languages("See you tomorrow"), ["english"]);
        assert_eq!(languages("Merci beaucoup"), ["french"]);
        // Undecidable: the fallback.
        assert_eq!(languages("Introduction"), ["french"]);
        assert_eq!(languages("42"), ["french"]);
        assert_eq!(languages(""), Vec::<String>::new());
    }

    #[test]
    fn mixed_lines() {
        let text = "Voici le message reçu ce matin de notre partenaire :\n\
                    Release notes\n\
                    The new version fixes the crash when the audio device changes.\n\
                    Merci de le relire avant la réunion de demain.";
        let segments = detector().segments(text);
        assert_eq!(
            segments,
            [
                Segment {
                    language: "french".into(),
                    text: "Voici le message reçu ce matin de notre partenaire :".into()
                },
                Segment {
                    language: "english".into(),
                    text: "Release notes\nThe new version fixes the crash when the audio device changes."
                        .into()
                },
                Segment {
                    language: "french".into(),
                    text: "Merci de le relire avant la réunion de demain.".into()
                },
            ]
        );
    }

    #[test]
    fn titles_follow_their_paragraph() {
        let text = "Introduction\n\nCe document décrit le fonctionnement du logiciel.";
        assert_eq!(languages(text), ["french"]);
    }

    #[test]
    fn one_language_configured() {
        let detector = LanguageDetector::new(&["english".to_string()]);
        let segments = detector.segments("Bonjour tout le monde, voici un texte en français.");
        assert_eq!(segments[0].language, "english");
    }

    #[test]
    fn model_names() {
        assert!(is_known("french_24l"));
        assert!(!is_known("klingon"));
        assert_eq!(label("english"), "Anglais");
        assert_eq!(label("french_24l"), "Français (french_24l)");
        let detector = LanguageDetector::new(&["english_2026-10".to_string(), "french".to_string()]);
        assert_eq!(detector.segments("Introduction")[0].language, "english_2026-10");
        assert_eq!(detector.split("german", "Hallo")[0].language, "german");
    }
}


