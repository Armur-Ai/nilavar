//! Local passphrase-strength guidance and passphrase suggestions.
//!
//! The estimate is a rough, conservative heuristic computed entirely on this
//! device. It is guidance, not proof of strength: a passphrase that looks
//! random to this estimator may still be guessable if it is reused, appears
//! in a breach, or is derived from personal information. No complexity rule
//! is enforced; the user decides.

use crate::ids::random_bytes;
use crate::secret::Passphrase;

const WORDLIST: &str = include_str!("../data/eff_large_wordlist.words");

/// A handful of very common passwords/patterns; matching any of them (case
/// insensitive, ignoring trailing digits/symbols) caps the estimate.
const COMMON: &[&str] = &[
    "password", "passw0rd", "123456", "12345678", "123456789", "qwerty", "qwertyuiop", "abc123", "letmein",
    "welcome", "monkey", "dragon", "iloveyou", "admin", "login", "master", "sunshine", "princess", "football",
    "baseball", "shadow", "superman", "trustno1", "hello", "freedom", "whatever", "secret", "changeme", "default",
    "asdfgh", "zxcvbn", "111111", "000000", "starwars", "matrix", "hunter2", "correcthorsebatterystaple",
];

/// Coarse rating bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rating {
    /// < 35 bits.
    VeryWeak,
    /// < 50 bits.
    Weak,
    /// < 65 bits.
    Fair,
    /// < 80 bits.
    Strong,
    /// ≥ 80 bits.
    Excellent,
}

impl Rating {
    /// Label for display.
    pub fn label(self) -> &'static str {
        match self {
            Rating::VeryWeak => "very weak",
            Rating::Weak => "weak",
            Rating::Fair => "fair",
            Rating::Strong => "strong",
            Rating::Excellent => "excellent",
        }
    }
}

/// Specific, non-sensitive observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    /// Fewer than 12 characters.
    Short,
    /// Matches a very common password or pattern.
    Common,
    /// Contains keyboard or alphabetical sequences.
    Sequence,
    /// Contains long runs of a repeated character.
    Repeats,
    /// Uses only one character class.
    SingleClass,
    /// Starts or ends with whitespace (kept exactly as typed).
    EdgeWhitespace,
    /// Looks like a multi-word passphrase (estimated per word).
    WordBased,
}

impl Hint {
    /// Fixed explanatory text.
    pub fn text(self) -> &'static str {
        match self {
            Hint::Short => "Shorter than 12 characters. Longer passphrases are much harder to guess.",
            Hint::Common => "Resembles a very common password. Attackers try these first.",
            Hint::Sequence => "Contains a predictable sequence (like abc, 123 or qwerty).",
            Hint::Repeats => "Contains repeated characters, which add little strength.",
            Hint::SingleClass => "Uses only one kind of character.",
            Hint::EdgeWhitespace => "Leading or trailing spaces are part of the passphrase and are kept exactly.",
            Hint::WordBased => "Estimated per word, assuming the words might be guessable choices.",
        }
    }
}

/// A strength estimate.
#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    /// Rough entropy estimate in bits.
    pub bits: f64,
    /// Rating band.
    pub rating: Rating,
    /// Observations.
    pub hints: Vec<Hint>,
}

impl Estimate {
    /// Rough time for an offline attacker making `guesses_per_sec` guesses
    /// to find it, on average, in seconds.
    pub fn offline_seconds(&self, guesses_per_sec: f64) -> f64 {
        2f64.powf(self.bits - 1.0) / guesses_per_sec
    }

    /// Human-readable rough crack-time for an attacker with
    /// [`ASSUMED_GUESSES_PER_SEC`] guesses per second.
    pub fn offline_time_label(&self) -> String {
        humanize_seconds(self.offline_seconds(ASSUMED_GUESSES_PER_SEC))
    }
}

/// Assumed offline guessing rate against age's scrypt (log N = 18) for a
/// well-resourced attacker with many GPUs. Deliberately pessimistic.
pub const ASSUMED_GUESSES_PER_SEC: f64 = 1.0e6;

/// Estimates passphrase strength locally.
pub fn estimate(p: &Passphrase) -> Estimate {
    estimate_str(p.expose_for_strength())
}

fn estimate_str(s: &str) -> Estimate {
    let mut hints = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    if n == 0 {
        return Estimate {
            bits: 0.0,
            rating: Rating::VeryWeak,
            hints,
        };
    }
    if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
        hints.push(Hint::EdgeWhitespace);
    }

    let (mut lower, mut upper, mut digit, mut symbol, mut space, mut other) = (false, false, false, false, false, false);
    for &c in &chars {
        if c.is_ascii_lowercase() {
            lower = true
        } else if c.is_ascii_uppercase() {
            upper = true
        } else if c.is_ascii_digit() {
            digit = true
        } else if c == ' ' {
            space = true
        } else if c.is_ascii() {
            symbol = true
        } else {
            other = true
        }
    }
    let pool = [(lower, 26.0), (upper, 26.0), (digit, 10.0), (symbol, 33.0), (space, 1.0), (other, 100.0)]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, v)| v)
        .sum::<f64>();
    let classes = [lower, upper, digit, symbol, other].iter().filter(|b| **b).count();
    if classes <= 1 && n < 20 {
        hints.push(Hint::SingleClass);
    }

    // Effective length: discount repeated runs and sequences.
    let mut effective = 0.0;
    let mut saw_repeat = false;
    let mut saw_seq = false;
    for i in 0..n {
        if i >= 1 && chars[i] == chars[i - 1] {
            effective += 0.2;
            if i >= 2 && chars[i] == chars[i - 2] {
                saw_repeat = true;
            }
            continue;
        }
        if i >= 1 {
            let d = chars[i] as i64 - chars[i - 1] as i64;
            if d.abs() == 1 && chars[i].is_ascii_alphanumeric() {
                effective += 0.3;
                if i >= 2 && chars[i - 1] as i64 - chars[i - 2] as i64 == d {
                    saw_seq = true;
                }
                continue;
            }
        }
        effective += 1.0;
    }
    let lowered = s.to_lowercase();
    for row in ["qwertyuiop", "asdfghjkl", "zxcvbnm", "1234567890"] {
        for w in 4..=row.len() {
            for start in 0..=(row.len() - w) {
                if lowered.contains(&row[start..start + w]) {
                    saw_seq = true;
                }
            }
        }
    }
    if saw_repeat {
        hints.push(Hint::Repeats);
    }
    if saw_seq {
        hints.push(Hint::Sequence);
        effective *= 0.8;
    }
    let mut bits = effective * pool.log2();

    // Multi-word passphrases: estimate per word (~11 bits for human-chosen
    // words, ~12.9 for words drawn from the 7,776-word list).
    let words: Vec<&str> = s
        .split(|c: char| c == ' ' || c == '-' || c == '_' || c == '.')
        .filter(|w| !w.is_empty())
        .collect();
    let wordy = words.len() >= 3 && words.iter().all(|w| w.chars().count() >= 2);
    if wordy {
        let per_word: f64 = words
            .iter()
            .map(|w| if in_wordlist(&w.to_lowercase()) { 12.9 } else { 11.0 })
            .sum();
        bits = bits.min(per_word + 2.0);
        hints.push(Hint::WordBased);
    }

    let stripped: String = lowered
        .trim_end_matches(|c: char| c.is_ascii_digit() || c.is_ascii_punctuation())
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if COMMON.iter().any(|c| stripped == *c || lowered == *c) {
        hints.push(Hint::Common);
        bits = bits.min(10.0);
    }
    if n < 12 {
        hints.push(Hint::Short);
    }
    let rating = match bits {
        b if b < 35.0 => Rating::VeryWeak,
        b if b < 50.0 => Rating::Weak,
        b if b < 65.0 => Rating::Fair,
        b if b < 80.0 => Rating::Strong,
        _ => Rating::Excellent,
    };
    Estimate { bits, rating, hints }
}

fn in_wordlist(w: &str) -> bool {
    WORDLIST.lines().any(|l| l == w)
}

/// Number of words in the bundled list.
pub fn wordlist_len() -> usize {
    WORDLIST.lines().count()
}

/// Generates a random passphrase of `words` words from the EFF large
/// wordlist using the OS CSPRNG (≈12.9 bits per word), joined by `-`.
pub fn suggest_passphrase(words: usize) -> Passphrase {
    let list: Vec<&str> = WORDLIST.lines().collect();
    let len = list.len() as u32;
    let limit = u32::MAX - (u32::MAX % len);
    let mut out = String::with_capacity(words * 10);
    for i in 0..words {
        let idx = loop {
            let v = u32::from_le_bytes(random_bytes::<4>());
            if v < limit {
                break (v % len) as usize;
            }
        };
        if i > 0 {
            out.push('-');
        }
        out.push_str(list[idx]);
    }
    Passphrase::new(out)
}

/// Formats seconds as a coarse human duration.
pub fn humanize_seconds(s: f64) -> String {
    const MIN: f64 = 60.0;
    const HOUR: f64 = 3600.0;
    const DAY: f64 = 86_400.0;
    const YEAR: f64 = 31_557_600.0;
    if !s.is_finite() || s > YEAR * 1e9 {
        "longer than a billion years".to_owned()
    } else if s < 1.0 {
        "less than a second".to_owned()
    } else if s < MIN {
        format!("{:.0} seconds", s)
    } else if s < HOUR {
        format!("{:.0} minutes", s / MIN)
    } else if s < DAY {
        format!("{:.0} hours", s / HOUR)
    } else if s < YEAR {
        format!("{:.0} days", s / DAY)
    } else if s < YEAR * 1e3 {
        format!("{:.0} years", s / YEAR)
    } else if s < YEAR * 1e6 {
        format!("{:.0} thousand years", s / YEAR / 1e3)
    } else {
        format!("{:.0} million years", s / YEAR / 1e6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordlist_is_complete() {
        assert_eq!(wordlist_len(), 7776);
    }

    #[test]
    fn weak_inputs_rate_weak() {
        for p in ["password", "Password1!", "123456789", "aaaaaaaaaaaa", "qwerty123", "abcdefgh"] {
            let e = estimate_str(p);
            assert!(e.rating <= Rating::Weak, "{p} rated {:?} ({} bits)", e.rating, e.bits);
        }
        assert!(estimate_str("password").hints.contains(&Hint::Common));
    }

    #[test]
    fn strong_inputs_rate_strong() {
        let s = suggest_passphrase(6);
        let e = estimate(&s);
        assert!(e.rating >= Rating::Strong, "{:?} {}", e.rating, e.bits);
        assert!(estimate_str("vT9#qL2!mZ8&wR4@xK").rating >= Rating::Strong);
    }

    #[test]
    fn suggestions_are_random_and_well_formed() {
        let a = suggest_passphrase(6);
        let b = suggest_passphrase(6);
        assert!(!a.matches(&b));
        assert_eq!(a.expose_for_strength().split('-').count(), 6);
    }

    #[test]
    fn humanize() {
        assert_eq!(humanize_seconds(0.5), "less than a second");
        assert_eq!(humanize_seconds(120.0), "2 minutes");
    }
}
