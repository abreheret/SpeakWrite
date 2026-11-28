//! TTS -> STT round trip over a corpus of sentences: each line is prepared as for a reading,
//! spoken by the voice engine, heard back by the dictation model and compared.
//! Catches mispronunciations after a change to the normalizer, the voice or pocket-tts.
//!
//! The dictation model writes what it hears in its own way ("14 h 30", "3,5 %"): its text goes
//! through the same normalizer as the corpus line before the comparison, so "quatorze heures
//! trente" and "14 h 30" match. Accents and punctuation are ignored ("où" / "ou").
//!
//! A sentence passes on its character error rate, spaces left out: the dictation model often
//! splits words its own way ("Ville Âge" -> "village", "Alésia" -> "à Lésia") or picks another
//! spelling of the same sound ("égal" -> "égale"), which would fail a word-by-word check.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::config::{Config, Paths};
use crate::language::{LanguageDetector, Segment};
use crate::{resample, stt, textnorm, tts};

/// Built-in corpus, used without a file argument.
const DEFAULT_CORPUS: &str = include_str!("../corpus/roundtrip.txt");

/// A sentence fails above this character error rate: room for a homophone or a misheard
/// liaison, not for a garbled word.
const MAX_CER: f64 = 0.10;

/// Readings of a sentence before it fails.
const ATTEMPTS: usize = 3;

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    line: usize,
    text: String,
}

/// One sentence per line; blank lines and lines starting with `#` are skipped.
fn parse(corpus: &str) -> Vec<Entry> {
    corpus
        .lines()
        .enumerate()
        .map(|(i, text)| Entry { line: i + 1, text: text.trim().to_string() })
        .filter(|entry| !entry.text.is_empty() && !entry.text.starts_with('#'))
        .collect()
}

/// Comparable words: lowercase, accents removed, punctuation, apostrophes and hyphens as spaces.
fn words(text: &str) -> Vec<String> {
    let mut folded = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' | 'á' => folded.push('a'),
            'é' | 'è' | 'ê' | 'ë' => folded.push('e'),
            'î' | 'ï' | 'í' => folded.push('i'),
            'ô' | 'ö' | 'ó' => folded.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => folded.push('u'),
            'ç' => folded.push('c'),
            'ÿ' => folded.push('y'),
            'ñ' => folded.push('n'),
            'æ' => folded.push_str("ae"),
            'œ' => folded.push_str("oe"),
            c if c.is_alphanumeric() => folded.push(c),
            _ => folded.push(' '),
        }
    }
    folded.split_whitespace().map(str::to_string).collect()
}

/// Substitutions, insertions and deletions to turn `reference` into `heard` (Levenshtein).
fn edits<T: PartialEq>(reference: &[T], heard: &[T]) -> usize {
    let mut previous: Vec<usize> = (0..=heard.len()).collect();
    for (i, r) in reference.iter().enumerate() {
        let mut current = vec![i + 1; heard.len() + 1];
        for (j, h) in heard.iter().enumerate() {
            let substitution = previous[j] + usize::from(r != h);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[heard.len()]
}

/// Runs the corpus at `file` (the built-in one if `None`). Fails if a sentence is misheard.
/// `SPEAKWRITE_ROUNDTRIP_WAV=dir` saves the audio of each failed sentence, to listen to it.
pub fn run(paths: &Paths, config: &Config, file: Option<&Path>) -> Result<()> {
    let corpus = match file {
        Some(file) => std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?,
        None => DEFAULT_CORPUS.to_string(),
    };
    let entries = parse(&corpus);
    if entries.is_empty() {
        bail!("the corpus has no sentence");
    }
    let wav_dir = std::env::var_os("SPEAKWRITE_ROUNDTRIP_WAV").map(PathBuf::from);
    if let Some(dir) = &wav_dir {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }

    let started = Instant::now();
    let model = stt::SttModel::load(&config.dictation.model, &config.dictation.device)?;
    println!("dictation model ready on {} in {:.1} s", model.device_name(), started.elapsed().as_secs_f32());
    let detector = LanguageDetector::new(&config.pocket.auto_languages);
    let mut bench = Bench {
        model,
        engines: tts::Engines::new(config.clone(), paths.clone()),
        normalizer: textnorm::Normalizer::new(config.pronunciation.clone()),
    };

    let started = Instant::now();
    let (mut total_words, mut word_errors, mut total_chars, mut char_errors) = (0, 0, 0, 0);
    let mut audio_seconds = 0.0;
    let mut failures: Vec<usize> = Vec::new();
    let mut retried = 0;
    for entry in &entries {
        let segments = detector.split(&config.pocket.language, &entry.text);
        if segments.is_empty() {
            continue;
        }
        // Each reading samples anew: a sentence fails only if it is misheard every time.
        let mut best = bench.attempt(&segments)?;
        let mut tries = 1;
        while best.cer() > MAX_CER && tries < ATTEMPTS {
            let attempt = bench.attempt(&segments)?;
            tries += 1;
            if attempt.cer() < best.cer() {
                best = attempt;
            }
        }
        audio_seconds += best.audio.len() as f64 / best.rate as f64;
        total_words += best.words;
        word_errors += best.word_errors;
        total_chars += best.chars;
        char_errors += best.char_errors;
        let failed = best.cer() > MAX_CER;
        let note = if tries > 1 { format!("  ({tries} tries)") } else { String::new() };
        println!(
            "{} {:>3.0} %  l.{:<3} {}{note}",
            if failed { "FAIL" } else { "  ok" },
            best.cer() * 100.0,
            entry.line,
            entry.text
        );
        if failed {
            println!("               said:  {}", best.spoken.trim());
            println!("               heard: {}", best.heard.trim());
            failures.push(entry.line);
            if let Some(dir) = &wav_dir {
                crate::write_wav(&dir.join(format!("line-{:03}.wav", entry.line)), best.rate, 1, &best.audio)?;
            }
        } else if tries > 1 {
            retried += 1;
        }
    }

    println!(
        "\n{} sentences ({retried} passed on a retry), character error rate {:.1} %, word error \
         rate {:.1} % ({word_errors} / {total_words} words), {audio_seconds:.0} s of audio in {:.0} s",
        entries.len(),
        char_errors as f64 * 100.0 / total_chars.max(1) as f64,
        word_errors as f64 * 100.0 / total_words.max(1) as f64,
        started.elapsed().as_secs_f32()
    );
    if !failures.is_empty() {
        let lines: Vec<String> = failures.iter().map(usize::to_string).collect();
        bail!(
            "{} sentence(s) above {:.0} % of misheard characters, lines {}",
            failures.len(),
            MAX_CER * 100.0,
            lines.join(", ")
        );
    }
    Ok(())
}

struct Bench {
    model: stt::SttModel,
    engines: tts::Engines,
    normalizer: textnorm::Normalizer,
}

/// One reading of a sentence, heard back.
struct Attempt {
    spoken: String,
    heard: String,
    /// Mono samples at `rate`.
    audio: Vec<f32>,
    rate: u32,
    words: usize,
    word_errors: usize,
    chars: usize,
    char_errors: usize,
}

impl Attempt {
    fn cer(&self) -> f64 {
        self.char_errors as f64 / self.chars.max(1) as f64
    }
}

impl Bench {
    /// Reads `segments` (one sentence) with the voice engine, then transcribes the audio.
    fn attempt(&mut self, segments: &[Segment]) -> Result<Attempt> {
        let mut spoken = String::new();
        let mut audio: Vec<f32> = Vec::new();
        let mut rate = stt::SAMPLE_RATE;
        for segment in segments {
            if !self.engines.is_ready(&segment.language) {
                let ready = Instant::now();
                self.engines.start(&segment.language)?;
                println!("{} voice engine ready in {:.1} s", segment.language, ready.elapsed().as_secs_f32());
            }
            let text = self.normalizer.normalize(&segment.text, &segment.language);
            let engine = self.engines.get(&segment.language).expect("started above");
            engine.synthesize(&text, &|| false, &mut |chunk| {
                rate = chunk.sample_rate;
                let channels = chunk.channels.max(1) as usize;
                audio.extend(chunk.samples.chunks(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32));
            })?;
            spoken.push_str(&text);
            spoken.push(' ');
        }

        let mut heard = String::new();
        self.model.begin()?;
        let mut on_word = |word: String| {
            heard.push_str(&word);
            heard.push(' ');
        };
        self.model.feed(&resample::Resampler::new(rate, stt::SAMPLE_RATE).process(&audio), &mut on_word)?;
        self.model.finish(&mut on_word)?;

        // Both sides through the normalizer: the model writes "14 h 30" for "quatorze heures trente".
        let language = &segments[0].language;
        let reference = words(&self.normalizer.normalize(&spoken, language));
        let hypothesis = words(&self.normalizer.normalize(&heard, language));
        let (reference_chars, hypothesis_chars) = (letters(&reference), letters(&hypothesis));
        Ok(Attempt {
            words: reference.len(),
            word_errors: edits(&reference, &hypothesis),
            chars: reference_chars.len(),
            char_errors: edits(&reference_chars, &hypothesis_chars),
            spoken,
            heard,
            audio,
            rate,
        })
    }
}

/// The letters and digits of `words`, without the spaces between them.
fn letters(words: &[String]) -> Vec<char> {
    words.iter().flat_map(|word| word.chars()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str) -> Vec<String> {
        words(text)
    }

    #[test]
    fn corpus_skips_comments_and_blank_lines() {
        let entries = parse("# Titre\n\nUne phrase.\n  # retrait\n  Deux.  \n");
        assert_eq!(
            entries,
            vec![
                Entry { line: 3, text: "Une phrase.".to_string() },
                Entry { line: 5, text: "Deux.".to_string() },
            ]
        );
        assert!(!parse(DEFAULT_CORPUS).is_empty());
    }

    #[test]
    fn words_ignore_case_accents_and_punctuation() {
        assert_eq!(w("Où est-il ? C'est l'été."), w("ou est il c est l ete"));
        assert_eq!(w("Cœur, « déjà » !"), vec!["coeur", "deja"]);
    }

    #[test]
    fn edits_count_substitutions_insertions_deletions() {
        assert_eq!(edits(&w("le chat dort"), &w("le chat dort")), 0);
        assert_eq!(edits(&w("le chat dort"), &w("le chien dort")), 1);
        assert_eq!(edits(&w("le chat dort"), &w("le chat")), 1);
        assert_eq!(edits(&w("le chat dort"), &w("le petit chat dort bien")), 2);
        assert_eq!(edits(&w("le chat"), &[]), 2);
        assert_eq!(edits(&[], &w("bruit")), 1);
    }

    #[test]
    fn characters_ignore_how_words_are_split() {
        assert_eq!(edits(&letters(&w("Prénom Ville Âge")), &letters(&w("prénom village"))), 1);
        assert_eq!(edits(&letters(&w("César assiège Alésia")), &letters(&w("César assiège à Lésia"))), 0);
    }

    #[test]
    fn dictation_spelling_matches_the_spoken_form() {
        let normalizer = textnorm::Normalizer::new(Default::default());
        let n = |text: &str| words(&normalizer.normalize(text, "french"));
        assert_eq!(n("Rendez-vous à 14 h 30."), n("Rendez-vous à quatorze heures trente."));
        assert_eq!(n("Hausse de 3,5 %."), n("Hausse de trois virgule cinq pour cent."));
        assert_eq!(n("En 2026."), n("En deux mille vingt-six."));
        assert_eq!(n("Il coûte 12,50 euros."), n("Il coûte 12,50 €."));
    }
}
