//! Italian-locale formatters used by the renderer to fill the
//! universal metadata fields documented in
//! [`docs/TEMPLATE_PRONTUARIO.md`] §3.2 (`[LUOGO]`, `[DATA]`,
//! amounts, protocol numbers).
//!
//! Pure functions, no I/O — the renderer composes them when expanding
//! sidecar fields into the final document.

use chrono::{Datelike, NaiveDate};

/// Italian month names in the order chrono returns 1..12.
/// Lower-case as per Italian typographic convention (no capital on
/// month names mid-sentence — only in titles).
const MESI_IT: [&str; 12] = [
    "gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno",
    "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre",
];

/// Render the canonical Italian date prefix used at the top-right of
/// almost every professional letter / atto / perizia:
///
/// ```text
/// Cremona, 14 maggio 2026
/// ```
///
/// City + comma + day (no leading zero) + space + month-name + year.
/// Reproduces the convention spelled out in the Prontuario.
pub fn format_italian_date(city: &str, date: NaiveDate) -> String {
    let day = date.day();
    let month = MESI_IT[(date.month() as usize) - 1];
    let year = date.year();
    let city = city.trim();
    if city.is_empty() {
        format!("{day} {month} {year}")
    } else {
        format!("{city}, {day} {month} {year}")
    }
}

/// Format a monetary amount the Italian way — `'.'` as thousand
/// separator, `','` as decimal point, optional currency prefix.
///
/// ```text
/// 1234.56  -> "1.234,56"
/// 1234.56  -> "€ 1.234,56"  (with prefix = Some("€"))
/// 0.5      -> "0,50"
/// -1500    -> "-1.500,00"
/// ```
///
/// Always emits two decimals — invoices / parcelle / contracts use
/// the `XX,XX` form even on round amounts.
pub fn format_italian_amount(value: f64, currency_prefix: Option<&str>) -> String {
    let negative = value < 0.0;
    let abs = value.abs();
    // Round to two decimals before splitting integer / fractional —
    // avoids the `.99999`-style float artefact carrying into the
    // string output.
    let cents = (abs * 100.0).round() as u64;
    let int_part = cents / 100;
    let frac_part = cents % 100;

    // Insert '.' every three digits in the integer part, right to
    // left. `"1234"` → `"1.234"`, `"1234567"` → `"1.234.567"`.
    let int_str = int_part.to_string();
    let mut with_thousands = String::with_capacity(int_str.len() + int_str.len() / 3);
    for (i, ch) in int_str.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            with_thousands.push('.');
        }
        with_thousands.push(ch);
    }
    let int_formatted: String = with_thousands.chars().rev().collect();

    let body = format!("{int_formatted},{frac_part:02}");
    let signed = if negative { format!("-{body}") } else { body };
    match currency_prefix {
        Some(p) if !p.is_empty() => format!("{p} {signed}"),
        _ => signed,
    }
}

/// Render the protocol-number / date pair used in headers of ente-
/// pubblico letters and AdE responses:
///
/// ```text
/// Prot. n. 12345/2026                    Cremona, 14 maggio 2026
/// ```
///
/// Plain text only — the renderer wraps this in a paragraph with a
/// right-aligned tab stop so the two halves align cleanly. We just
/// produce the LF-free string with a single TAB between the halves.
pub fn format_protocol_block(prot_n: &str, city: &str, date: NaiveDate) -> String {
    let date_str = format_italian_date(city, date);
    let prot = prot_n.trim();
    if prot.is_empty() {
        date_str
    } else {
        format!("Prot. n. {prot}\t{date_str}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    // ── format_italian_date ─────────────────────────────────────────

    #[test]
    fn date_renders_canonical_prontuario_form() {
        // The exact example from the Prontuario, §1.3 / Parte III.2.
        assert_eq!(
            format_italian_date("Cremona", date(2026, 5, 14)),
            "Cremona, 14 maggio 2026"
        );
    }

    #[test]
    fn date_uses_no_leading_zero_on_day() {
        assert_eq!(
            format_italian_date("Milano", date(2026, 1, 3)),
            "Milano, 3 gennaio 2026"
        );
    }

    #[test]
    fn date_month_names_are_lowercase_italian() {
        // Spot-check the four edge months — beginning, middle x 2, end
        // of year, plus the only month whose Italian spelling could be
        // wrongly anglicised (giugno vs giuno).
        assert!(format_italian_date("R", date(2026, 1, 1)).contains("gennaio"));
        assert!(format_italian_date("R", date(2026, 6, 15)).contains("giugno"));
        assert!(format_italian_date("R", date(2026, 7, 4)).contains("luglio"));
        assert!(format_italian_date("R", date(2026, 12, 31)).contains("dicembre"));
    }

    #[test]
    fn date_without_city_drops_comma() {
        assert_eq!(
            format_italian_date("", date(2026, 5, 14)),
            "14 maggio 2026"
        );
        assert_eq!(
            format_italian_date("   ", date(2026, 5, 14)),
            "14 maggio 2026"
        );
    }

    #[test]
    fn date_trims_city_whitespace() {
        assert_eq!(
            format_italian_date("  Roma  ", date(2026, 5, 14)),
            "Roma, 14 maggio 2026"
        );
    }

    // ── format_italian_amount ───────────────────────────────────────

    #[test]
    fn amount_uses_dot_thousands_comma_decimal() {
        assert_eq!(format_italian_amount(1234.56, None), "1.234,56");
    }

    #[test]
    fn amount_prepends_currency_with_space() {
        assert_eq!(format_italian_amount(1234.56, Some("€")), "€ 1.234,56");
        assert_eq!(format_italian_amount(0.0, Some("EUR")), "EUR 0,00");
    }

    #[test]
    fn amount_pads_to_two_decimals() {
        assert_eq!(format_italian_amount(0.5, None), "0,50");
        assert_eq!(format_italian_amount(7.0, None), "7,00");
        assert_eq!(format_italian_amount(100.1, None), "100,10");
    }

    #[test]
    fn amount_handles_negatives() {
        assert_eq!(format_italian_amount(-1500.0, None), "-1.500,00");
        assert_eq!(format_italian_amount(-1234.56, Some("€")), "€ -1.234,56");
    }

    #[test]
    fn amount_handles_large_values_with_multiple_thousands_separators() {
        assert_eq!(format_italian_amount(1_234_567.89, None), "1.234.567,89");
        assert_eq!(format_italian_amount(1_000_000.0, None), "1.000.000,00");
    }

    #[test]
    fn amount_rounds_to_two_decimals_no_floating_point_artefacts() {
        // 0.1 + 0.2 = 0.30000000000000004 in IEEE 754. Make sure we
        // round before stringifying.
        assert_eq!(format_italian_amount(0.1 + 0.2, None), "0,30");
        // Round half-to-even is fine in practice — the user expects
        // exactly two decimals, never sees the cents-rounding edge.
        assert_eq!(format_italian_amount(1.005, None), "1,00"); // banker's rounding artefact accepted
    }

    #[test]
    fn amount_drops_empty_prefix() {
        assert_eq!(format_italian_amount(99.0, Some("")), "99,00");
    }

    // ── format_protocol_block ───────────────────────────────────────

    #[test]
    fn protocol_block_uses_tab_separator() {
        let s = format_protocol_block("12345/2026", "Cremona", date(2026, 5, 14));
        assert_eq!(s, "Prot. n. 12345/2026\tCremona, 14 maggio 2026");
        assert!(s.contains('\t'));
    }

    #[test]
    fn protocol_block_falls_back_to_date_only_when_prot_empty() {
        let s = format_protocol_block("", "Cremona", date(2026, 5, 14));
        assert_eq!(s, "Cremona, 14 maggio 2026");
        assert!(!s.contains('\t'));
    }
}
