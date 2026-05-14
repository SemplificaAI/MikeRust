//! `[PLACEHOLDER]` substitution — the convention adopted by every
//! shipped template per
//! [`docs/TEMPLATE_PRONTUARIO.md`] §3.1.4 (`placeholder_syntax =
//! "square_brackets"`).
//!
//! The renderer applies substitution AFTER the Markdown → XML pass,
//! against the body text and the header/footer partials. This keeps
//! placeholder syntax outside the Markdown parser's vocabulary — a
//! square-bracket pair inside a Markdown link `[label](url)` is
//! consumed by the parser before we ever see it, so the only `[NAME]`
//! tokens reaching this module are literal text.
//!
//! Rules:
//!
//! * Token is `[ALL_CAPS_UNDERSCORES_DOTS]`. Names hold A-Z, 0-9, `_`
//!   and `.` (the dot allows nested keys like `[PARTE_ASSISTITA.CF]`).
//! * Unknown tokens are left in place — the renderer surfaces them to
//!   the user so missing metadata is visible at proofread time
//!   instead of getting silently swallowed.
//! * Substitution is single-pass (no recursive expansion). A value
//!   that itself contains `[X]` syntax is treated as final.

use std::collections::HashMap;

/// Replace every `[NAME]` occurrence in `input` whose `NAME` appears
/// as a key in `bag`. Returns the rewritten string and the count of
/// substitutions performed.
///
/// XML-escaping is the renderer's job at a different layer — this
/// function operates on plain text. Values are inserted verbatim.
pub fn substitute(input: &str, bag: &HashMap<String, String>) -> (String, usize) {
    if input.is_empty() {
        return (String::new(), 0);
    }
    let bytes = input.as_bytes();
    // Accumulate raw bytes — every chunk we push (input slices, bag
    // values) is itself valid UTF-8, so the concatenation is too.
    // Pushing `bytes[i] as char` would have cast each byte of a
    // multi-byte sequence to a separate codepoint and corrupted the
    // text — see the regression test `handles_utf8_around_tokens`.
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0;
    let mut hits = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // Scan forward for matching ']' within a reasonable token
            // window. A `[` not followed by a valid token glyph is
            // emitted as-is.
            if let Some(end) = find_token_close(bytes, i + 1) {
                let token = &input[i + 1..end];
                if is_valid_token(token) {
                    if let Some(value) = bag.get(token) {
                        out.extend_from_slice(value.as_bytes());
                        i = end + 1;
                        hits += 1;
                        continue;
                    }
                    // Unknown token — preserve verbatim so the user
                    // sees the gap.
                    out.extend_from_slice(&bytes[i..=end]);
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Safe: every chunk above is either ASCII (single byte) or a
    // valid-UTF-8 string slice from `input` / `bag.get(...)`.
    (String::from_utf8(out).expect("valid utf8 by construction"), hits)
}

/// Locate the `]` that closes a token starting at `start`. Returns
/// `None` if no close is found within MAX_TOKEN_LEN bytes or if any
/// non-token char is encountered along the way.
fn find_token_close(bytes: &[u8], start: usize) -> Option<usize> {
    const MAX_TOKEN_LEN: usize = 64;
    let end_cap = (start + MAX_TOKEN_LEN).min(bytes.len());
    for i in start..end_cap {
        match bytes[i] {
            b']' => {
                if i == start {
                    return None; // empty token
                }
                return Some(i);
            }
            b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' => continue,
            _ => return None, // non-token char → not a placeholder
        }
    }
    None
}

fn is_valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_' || c == b'.')
}

/// Collect every token still present after a substitution pass. Used
/// by the renderer to warn the user about unfilled fields BEFORE
/// emitting the final `.docx` (so missing data is loud, not silent).
pub fn find_remaining_tokens(input: &str) -> Vec<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(end) = find_token_close(bytes, i + 1) {
                let token = &input[i + 1..end];
                if is_valid_token(token) {
                    out.push(token.to_string());
                    i = end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bag(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn substitutes_single_token() {
        let (out, n) = substitute("Caro [NOME], ti scrivo.", &bag(&[("NOME", "Mario")]));
        assert_eq!(out, "Caro Mario, ti scrivo.");
        assert_eq!(n, 1);
    }

    #[test]
    fn substitutes_multiple_distinct_tokens() {
        let (out, n) = substitute(
            "[LUOGO], [DATA] — Pratica [RIF_PRATICA]",
            &bag(&[
                ("LUOGO", "Cremona"),
                ("DATA", "14 maggio 2026"),
                ("RIF_PRATICA", "2026/042"),
            ]),
        );
        assert_eq!(out, "Cremona, 14 maggio 2026 — Pratica 2026/042");
        assert_eq!(n, 3);
    }

    #[test]
    fn substitutes_same_token_multiple_times() {
        let (out, n) = substitute(
            "[NOME] e di nuovo [NOME].",
            &bag(&[("NOME", "Tizio")]),
        );
        assert_eq!(out, "Tizio e di nuovo Tizio.");
        assert_eq!(n, 2);
    }

    #[test]
    fn unknown_tokens_left_in_place() {
        let (out, n) = substitute(
            "Caro [NOTSET], hai [NOME].",
            &bag(&[("NOME", "Mario")]),
        );
        assert_eq!(out, "Caro [NOTSET], hai Mario.");
        assert_eq!(n, 1);
    }

    #[test]
    fn supports_dotted_keys() {
        let (out, _) = substitute(
            "[PARTE_ASSISTITA.NOME] residente in [PARTE_ASSISTITA.INDIRIZZO]",
            &bag(&[
                ("PARTE_ASSISTITA.NOME", "Caio S.r.l."),
                ("PARTE_ASSISTITA.INDIRIZZO", "Via Roma 1"),
            ]),
        );
        assert_eq!(out, "Caio S.r.l. residente in Via Roma 1");
    }

    #[test]
    fn lowercase_tokens_are_not_placeholders() {
        // `[label]` from Markdown link syntax should never match — it
        // contains lowercase letters which our grammar rejects.
        let (out, n) = substitute("vedi [link](http://x)", &bag(&[("LINK", "wrong")]));
        assert_eq!(out, "vedi [link](http://x)");
        assert_eq!(n, 0);
    }

    #[test]
    fn empty_brackets_not_matched() {
        let (out, n) = substitute("vuoto []", &bag(&[]));
        assert_eq!(out, "vuoto []");
        assert_eq!(n, 0);
    }

    #[test]
    fn unmatched_open_bracket_passes_through() {
        let (out, n) = substitute("inizio [SENZA_FINE", &bag(&[("SENZA_FINE", "X")]));
        // No `]` ever closes, no substitution, original text preserved.
        assert_eq!(out, "inizio [SENZA_FINE");
        assert_eq!(n, 0);
    }

    #[test]
    fn handles_utf8_around_tokens() {
        // Italian accented chars must not break the byte walk.
        let (out, n) = substitute(
            "L'avvocato è [NOME], procuratore di Caio.",
            &bag(&[("NOME", "Verdi")]),
        );
        assert_eq!(out, "L'avvocato è Verdi, procuratore di Caio.");
        assert_eq!(n, 1);
    }

    #[test]
    fn token_too_long_passes_through() {
        // 65-char token (>MAX_TOKEN_LEN) must not match.
        let huge = "A".repeat(65);
        let input = format!("hello [{huge}]");
        let mut b = HashMap::new();
        b.insert(huge.clone(), "X".to_string());
        let (out, n) = substitute(&input, &b);
        assert_eq!(out, input);
        assert_eq!(n, 0);
    }

    #[test]
    fn empty_input_returns_empty() {
        let (out, n) = substitute("", &bag(&[]));
        assert!(out.is_empty());
        assert_eq!(n, 0);
    }

    // ── find_remaining_tokens ───────────────────────────────────────

    #[test]
    fn find_remaining_lists_unfilled_only() {
        // Two-stage flow: substitute against partial bag, then call
        // find_remaining_tokens on the result to surface what's left.
        let (out, _) = substitute(
            "[NOME] da [CITTA], P.IVA [CF_PIVA].",
            &bag(&[("NOME", "Mario")]),
        );
        let remaining = find_remaining_tokens(&out);
        assert_eq!(remaining, vec!["CF_PIVA".to_string(), "CITTA".to_string()]);
    }

    #[test]
    fn find_remaining_dedupes() {
        let tokens = find_remaining_tokens("[A] [A] [B]");
        assert_eq!(tokens, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn find_remaining_skips_markdown_brackets() {
        // `[label]` is rejected by the grammar — must not appear.
        let tokens = find_remaining_tokens("see [link](url) and [NOME]");
        assert_eq!(tokens, vec!["NOME".to_string()]);
    }
}
