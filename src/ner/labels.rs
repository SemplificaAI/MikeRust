//! Default PII label set the `gliner2-privacy-filter-PII-multi`
//! model is asked to detect when the caller doesn't supply its own.
//!
//! The exact labels each model emits depend on its training schema —
//! these constants match the GDPR + Italian-fiscal taxonomy that
//! MikeRust's other verticals already lean on. If the model card
//! later confirms different label strings, fix the constants here
//! and nothing else — the engine takes the slice as-is.
//!
//! Callers that need a tighter subset (e.g. only banking
//! identifiers) pass a custom `&[&str]` to `extract_entities`.

/// Canonical PII label list — used when the caller passes `None`.
/// Order is preservation-stable so a UI badge list reads predictably
/// rather than reshuffling between runs.
pub fn default_pii_labels() -> &'static [&'static str] {
    &[
        "person_name",
        "email",
        "phone",
        "address",
        "fiscal_code",
        "vat_number",
        "iban",
        "credit_card",
        "date_of_birth",
        "ip_address",
        "license_plate",
    ]
}
