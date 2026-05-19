//! Canonical professional-domain enum.
//!
//! Added in migration 0018 to let users categorise workflows, tabular
//! reviews, projects, and documents by professional vertical (legal,
//! medical, finance, …) and filter list views accordingly. Everything
//! inherited from upstream `willchen96/mike` defaults to `legal` —
//! the original tool was law-firm focused — and the schema default
//! ensures pre-migration rows land there automatically.
//!
//! Validation lives at the API boundary, not as a SQL CHECK
//! constraint, so adding a future domain (e.g. `architecture`,
//! `journalism`) is a one-line change to `DOMAINS` plus a new i18n
//! label set — no migration required.

/// The shipped domain set. Frontend `Domain` type in
/// `frontend/src/app/components/shared/types.ts` mirrors this list.
pub const DOMAINS: &[&str] = &[
    "legal",
    "medical",
    "finance",
    "real_estate",
    "hr",
    "insurance",
    "ip",
    "compliance",
    "pa",
    "others",
];

/// Default for new rows when the client doesn't specify and for the
/// schema column default. Matches the upstream Mike origin.
pub const DEFAULT_DOMAIN: &str = "legal";

/// Returns true when `s` is one of the canonical domain identifiers.
/// Used by route handlers to validate the `domain` field on
/// create/update payloads before persisting.
pub fn is_valid(s: &str) -> bool {
    DOMAINS.iter().any(|d| *d == s)
}

/// Normalize an optional user-provided domain into a guaranteed valid
/// one for INSERT — falls back to `DEFAULT_DOMAIN` for None / empty
/// / non-canonical values. Used at create time only; for update we
/// want to reject invalid input instead of silently coercing.
pub fn normalise_or_default(s: Option<&str>) -> &str {
    match s {
        Some(v) if is_valid(v) => v,
        _ => DEFAULT_DOMAIN,
    }
}
