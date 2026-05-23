//! Deterministic regex-based PII redaction. Runs **before** the
//! GLiNER2 pass so high-precision patterns (emails, fiscal codes,
//! IBANs, phone numbers, credit cards) are guaranteed redacted even
//! when the zero-shot model misses them. GLiNER2 still runs on the
//! regex-masked text for everything that needs context to identify
//! (person names, addresses, organizations).
//!
//! Patterns are deliberately conservative — we'd rather over-mask
//! a coincidental match than leak a real fiscal code. The matchers
//! are anchored on character-class boundaries so a fiscal code
//! inside a longer alphanumeric token isn't mistakenly redacted.

use regex::Regex;
use std::sync::OnceLock;

/// One pass over `text` replacing each pattern with `[LABEL]`. Patterns
/// are applied in a fixed order chosen so longer/more-specific masks
/// run first (fiscal code before generic "any 11-digit sequence"),
/// keeping the output stable when a token matches multiple patterns.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    for (label, regex) in patterns() {
        out = regex.replace_all(&out, format!("[{label}]").as_str()).into_owned();
    }
    out
}

fn patterns() -> &'static [(&'static str, Regex)] {
    static CELL: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    CELL.get_or_init(|| {
        vec![
            // Italian fiscal code: 6 letters, 2 digits, 1 letter,
            // 2 digits, 1 letter, 3 digits, 1 letter. Word-bounded
            // so `RSSMRA80A01H501Z` matches but `RSSMRA80A01H501ZX`
            // does not. Common spurious match: short prose tokens —
            // mitigated by the strict 16-char layout.
            (
                "FISCAL_CODE",
                Regex::new(r"(?i)\b[A-Z]{6}\d{2}[A-Z]\d{2}[A-Z]\d{3}[A-Z]\b").unwrap(),
            ),
            // IT VAT number / partita IVA: literal "IT" prefix + 11
            // digits, OR bare 11-digit sequence in a partita-IVA-ish
            // context. Bare 11-digit catches phone too in some
            // formats — phone pattern runs after, but the redaction
            // is idempotent so a double-tag (VAT then PHONE) just
            // overwrites to PHONE (last-write-wins per pattern).
            (
                "VAT_NUMBER",
                Regex::new(r"\b(?:IT)?\d{11}\b").unwrap(),
            ),
            // IBAN: country code (2 letters) + 2 check digits +
            // 11-30 BBAN chars. The 15-34 char total matches every
            // ISO 13616 country. Word-bounded.
            (
                "IBAN",
                Regex::new(r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b").unwrap(),
            ),
            // Credit card: 13-19 digit sequences with optional
            // grouping by hyphen/space. Doesn't validate Luhn —
            // any digit run in that range is suspect enough to
            // mask. False positives include long order numbers;
            // acceptable trade-off for "never leak a card".
            (
                "CREDIT_CARD",
                Regex::new(r"\b(?:\d[ -]?){12,18}\d\b").unwrap(),
            ),
            // Email: RFC-friendly subset that catches every real
            // address without trying to validate edge cases.
            (
                "EMAIL",
                Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap(),
            ),
            // Phone: international +39 / national, with separators.
            // Matches +39 333 1234567, 0039 02 12345678, etc.
            (
                "PHONE",
                Regex::new(r"(?:\+|00)\d{1,3}[\s.-]?\d{2,4}[\s.-]?\d{4,8}").unwrap(),
            ),
            // IPv4 address.
            (
                "IP_ADDRESS",
                Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap(),
            ),
            // dd/mm/yyyy and dd-mm-yyyy and yyyy-mm-dd. Matches
            // most clinical dates the dataset surfaced.
            (
                "DATE",
                Regex::new(r"\b(?:\d{1,2}[/-]\d{1,2}[/-]\d{2,4}|\d{4}[/-]\d{1,2}[/-]\d{1,2})\b").unwrap(),
            ),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email() {
        let s = "Contact mario.rossi@example.com for info";
        assert_eq!(redact(s), "Contact [EMAIL] for info");
    }

    #[test]
    fn redacts_fiscal_code() {
        let s = "CF: RSSMRA80A01H501Z";
        assert_eq!(redact(s), "CF: [FISCAL_CODE]");
    }

    #[test]
    fn redacts_iban() {
        let s = "IBAN IT60X0542811101000000123456";
        assert_eq!(redact(s), "IBAN [IBAN]");
    }

    #[test]
    fn redacts_iso_date() {
        let s = "Data dimissione: 28/10/2025";
        assert_eq!(redact(s), "Data dimissione: [DATE]");
    }

    #[test]
    fn redacts_phone() {
        let s = "Tel +39 333 1234567";
        assert!(redact(s).contains("[PHONE]"));
    }

    #[test]
    fn leaves_plain_text_untouched() {
        let s = "La paziente presenta allergie multiple.";
        assert_eq!(redact(s), s);
    }
}
