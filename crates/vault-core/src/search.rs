//! In-memory search. Nothing here persists an index: results are computed
//! on demand from the unlocked vault and dropped with the caller's state.

use std::ops::Range;

use zeroize::Zeroizing;

use crate::ids::DocId;
use crate::model::Vault;

/// Simple case folding for matching (single-char lowercase mappings only,
/// so byte offsets in the original text stay exact).
fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// Finds all non-overlapping occurrences of `needle` (literal) and returns
/// their byte ranges in `haystack`.
pub fn find_all(haystack: &str, needle: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    find_all_limited(haystack, needle, case_sensitive, usize::MAX)
}

/// Like [`find_all`] but stops after `limit` matches.
pub fn find_all_limited(
    haystack: &str,
    needle: &str,
    case_sensitive: bool,
    limit: usize,
) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if needle.is_empty() || limit == 0 {
        return out;
    }
    if case_sensitive {
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(needle) {
            let s = start + pos;
            out.push(s..s + needle.len());
            if out.len() >= limit {
                break;
            }
            start = s + needle.len();
        }
        return out;
    }
    let pat: Vec<char> = needle.chars().map(fold).collect();
    let first = pat[0];
    let mut skip_until = 0usize;
    for (i, c) in haystack.char_indices() {
        if i < skip_until || fold(c) != first {
            continue;
        }
        if let Some(end) = match_at(haystack, i, &pat) {
            out.push(i..end);
            if out.len() >= limit {
                break;
            }
            skip_until = end;
        }
    }
    out
}

fn match_at(haystack: &str, start: usize, pat: &[char]) -> Option<usize> {
    let mut it = haystack[start..].char_indices();
    for &p in pat {
        let (_, c) = it.next()?;
        if fold(c) != p {
            return None;
        }
    }
    Some(match it.next() {
        Some((off, _)) => start + off,
        None => haystack.len(),
    })
}

/// Replaces every match; returns the new text and the number of
/// replacements. The result is zeroizing.
pub fn replace_all(
    text: &str,
    needle: &str,
    replacement: &str,
    case_sensitive: bool,
) -> (Zeroizing<String>, usize) {
    let ranges = find_all(text, needle, case_sensitive);
    let mut out = Zeroizing::new(String::with_capacity(
        text.len() + ranges.len() * replacement.len().saturating_sub(needle.len()),
    ));
    let mut last = 0;
    for r in &ranges {
        out.push_str(&text[last..r.start]);
        out.push_str(replacement);
        last = r.end;
    }
    out.push_str(&text[last..]);
    (out, ranges.len())
}

/// Where a search hit was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitField {
    /// Document title.
    Title,
    /// A tag.
    Tag,
    /// Document body.
    Body,
}

/// One search result.
pub struct SearchHit {
    /// Document.
    pub doc: DocId,
    /// Best field.
    pub field: HitField,
    /// Number of body matches (capped).
    pub body_matches: usize,
    /// One-line excerpt around the first body match (plaintext; transient).
    pub snippet: Zeroizing<String>,
    /// Byte range of the match within `snippet`.
    pub snippet_match: Range<usize>,
    /// Byte offset of the first body match within the document.
    pub first_offset: Option<usize>,
    /// Ranking score (higher first).
    pub score: i64,
}

impl std::fmt::Debug for SearchHit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SearchHit({:?}, {:?}, <redacted>)", self.doc, self.field)
    }
}

/// Parsed global query: free text plus optional `tag:` filters.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Query {
    /// Free text (literal, case-insensitive).
    pub text: String,
    /// Required tags (exact, case-insensitive).
    pub tags: Vec<String>,
}

impl Query {
    /// Parses `tag:foo some words`.
    pub fn parse(q: &str) -> Self {
        let mut text = Vec::new();
        let mut tags = Vec::new();
        for part in q.split_whitespace() {
            match part.strip_prefix("tag:").or_else(|| part.strip_prefix('#')) {
                Some(t) if !t.is_empty() => tags.push(t.to_lowercase()),
                _ => text.push(part),
            }
        }
        Self {
            text: text.join(" "),
            tags,
        }
    }

    /// Whether nothing was entered.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tags.is_empty()
    }
}

const MAX_BODY_MATCHES: usize = 999;

/// Searches titles, tags and text of live documents.
pub fn search_vault(vault: &Vault, query: &Query, limit: usize) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    if query.is_empty() {
        return hits;
    }
    for d in &vault.documents {
        if !query
            .tags
            .iter()
            .all(|t| d.tags.iter().any(|dt| dt.expose().to_lowercase() == *t))
        {
            continue;
        }
        let (title_hit, tag_hit, body) = if query.text.is_empty() {
            (false, false, Vec::new())
        } else {
            (
                !find_all_limited(d.title.expose(), &query.text, false, 1).is_empty(),
                d.tags
                    .iter()
                    .any(|t| !find_all_limited(t.expose(), &query.text, false, 1).is_empty()),
                find_all_limited(d.text.expose(), &query.text, false, MAX_BODY_MATCHES),
            )
        };
        if !query.text.is_empty() && !title_hit && !tag_hit && body.is_empty() {
            continue;
        }
        let field = if title_hit {
            HitField::Title
        } else if tag_hit {
            HitField::Tag
        } else {
            HitField::Body
        };
        let score = i64::from(title_hit) * 1000
            + i64::from(tag_hit) * 300
            + body.len().min(100) as i64
            + i64::from(d.pinned) * 50
            + d.updated_at / 86_400 % 100_000 / 1000;
        let (snippet, snippet_match) = match body.first() {
            Some(r) => make_snippet(d.text.expose(), r.clone(), 48),
            None => (Zeroizing::new(String::new()), 0..0),
        };
        hits.push(SearchHit {
            doc: d.id,
            field,
            body_matches: body.len(),
            snippet,
            snippet_match,
            first_offset: body.first().map(|r| r.start),
            score,
        });
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.score));
    hits.truncate(limit);
    hits
}

/// Builds a one-line excerpt with about `context` characters on each side.
pub fn make_snippet(
    text: &str,
    m: Range<usize>,
    context: usize,
) -> (Zeroizing<String>, Range<usize>) {
    let line_start = text[..m.start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[m.end..]
        .find('\n')
        .map(|i| m.end + i)
        .unwrap_or(text.len());
    let mut start = m.start;
    for (n, (i, _)) in text[line_start..m.start].char_indices().rev().enumerate() {
        start = line_start + i;
        if n + 1 >= context {
            break;
        }
    }
    let mut end = m.end;
    for (n, (i, c)) in text[m.end..line_end].char_indices().enumerate() {
        end = m.end + i + c.len_utf8();
        if n + 1 >= context {
            break;
        }
    }
    let mut s = Zeroizing::new(String::with_capacity(end - start + 6));
    let prefix = start > line_start;
    if prefix {
        s.push('…');
    }
    let offset = s.len();
    s.push_str(&text[start..end]);
    if end < line_end {
        s.push('…');
    }
    let ms = offset + (m.start - start);
    (s, ms..ms + (m.end - m.start))
}

/// Fuzzy subsequence score for the palette/switcher. `None` if `pattern`
/// is not a subsequence of `candidate` (case-insensitive).
pub fn fuzzy_score(pattern: &str, candidate: &str) -> Option<i64> {
    if pattern.is_empty() {
        return Some(0);
    }
    let cand: Vec<char> = candidate.chars().collect();
    let mut score = 0i64;
    let mut ci = 0usize;
    let mut prev_match: Option<usize> = None;
    for p in pattern.chars().filter(|c| !c.is_whitespace()).map(fold) {
        let mut found = None;
        while ci < cand.len() {
            if fold(cand[ci]) == p {
                found = Some(ci);
                ci += 1;
                break;
            }
            ci += 1;
        }
        let i = found?;
        score += 10;
        if prev_match.is_some_and(|pm| pm + 1 == i) {
            score += 15;
        }
        let word_start = i == 0
            || !cand[i - 1].is_alphanumeric()
            || (cand[i].is_uppercase() && cand[i - 1].is_lowercase());
        if word_start {
            score += 20;
        }
        if i == 0 {
            score += 10;
        }
        prev_match = Some(i);
    }
    score -= (cand.len() as i64) / 4;
    Some(score)
}

/// Positions (char indices) matched by [`fuzzy_score`], for highlighting.
pub fn fuzzy_positions(pattern: &str, candidate: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut pat = pattern
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(fold)
        .peekable();
    for (i, c) in candidate.chars().enumerate() {
        match pat.peek() {
            Some(&p) if fold(c) == p => {
                out.push(i);
                pat.next();
            }
            Some(_) => {}
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_find_unicode_and_case() {
        let t = "Straße STRASSE straße ÄÖÜ äöü";
        assert_eq!(find_all(t, "straße", false).len(), 2);
        assert_eq!(find_all(t, "straße", true).len(), 1);
        assert_eq!(find_all(t, "äöü", false).len(), 2);
        for r in find_all(t, "äöü", false) {
            assert!(t.is_char_boundary(r.start) && t.is_char_boundary(r.end));
        }
        assert_eq!(find_all("aaaa", "aa", true), vec![0..2, 2..4]);
        assert_eq!(find_all("aaaa", "AA", false), vec![0..2, 2..4]);
        assert!(find_all("abc", "", false).is_empty());
        // Regex metacharacters are literal.
        assert_eq!(find_all("a.*b a.*b", ".*", false).len(), 2);
    }

    #[test]
    fn replace_counts() {
        let (s, n) = replace_all("Key key KEY", "key", "🔑", false);
        assert_eq!(n, 3);
        assert_eq!(s.as_str(), "🔑 🔑 🔑");
    }

    #[test]
    fn vault_search_ranks_titles_and_filters_tags() {
        let mut v = Vault::new();
        let a = v.create_document("AWS credentials", None).unwrap();
        let b = v.create_document("Notes", None).unwrap();
        v.document_mut(b)
            .unwrap()
            .text
            .set("the aws account id is synthetic");
        v.set_tags(b, &["cloud"]).unwrap();
        let hits = search_vault(&v, &Query::parse("aws"), 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].doc, a);
        assert!(hits[1].snippet.contains("aws"));
        let only = search_vault(&v, &Query::parse("tag:cloud aws"), 10);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].doc, b);
    }

    #[test]
    fn snippet_bounds() {
        let text = format!("{}NEEDLE{}\nnext", "x".repeat(100), "y".repeat(100));
        let start = text.find("NEEDLE").unwrap();
        let (s, r) = make_snippet(&text, start..start + 6, 10);
        assert_eq!(&s[r], "NEEDLE");
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(!s.contains("next"));
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_score("nd", "New document").is_some());
        assert!(fuzzy_score("xyz", "New document").is_none());
        let a = fuzzy_score("lock", "Lock vault").unwrap();
        let b = fuzzy_score("lock", "Toggle clock overlay").unwrap();
        assert!(a > b);
        assert_eq!(fuzzy_positions("nd", "New doc"), vec![0, 4]);
    }
}
