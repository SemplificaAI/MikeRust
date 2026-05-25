// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

//! Domain-aware system-prompt **prologue**. Read once at chat-turn
//! time from `config/system-prompts/<locale>/<domain>.md` and
//! prepended to `MRUST_SYSTEM_PROMPT` so the assistant boots with a
//! professional-vertical persona before the generic Mike tool-use /
//! citation rules kick in.
//!
//! Resolution fall-back chain (first hit wins):
//!
//!   1. requested locale + requested domain
//!   2. `"it"` + requested domain (`it` is the primary curated
//!      locale — MikeRust's first-class users are Italian)
//!   3. `"en"` + requested domain (every domain ships an English
//!      version, so this is the last-stand language fallback)
//!   4. `None` — the caller composes a prologue without a domain body
//!
//! The directory hosting the files is found the same way the other
//! `crate::presets::*` registries find theirs: env-var override
//! (`MRUST_SYSTEM_PROMPTS_DIR`), CWD ancestor walk, then exe-dir
//! ancestor walk — so dev (cwd = workspace root) and installed-MSI
//! (cwd = anywhere, exe = `<install>/`) both land on the bundled
//! files without configuration.
//!
//! Country / jurisdiction handling is **prompt-time text**: there is
//! no `country` column on `user_settings` or `projects`. The
//! composer hard-codes a locale → default-country mapping (Italian →
//! Italy, French → France, …) and instructs the model to ASK the
//! user whenever the conversation suggests a different jurisdiction.
//! Adding a database-backed override is cheap if the present approach
//! turns out to be too coarse for power users.

use std::path::{Path, PathBuf};

const FALLBACK_LOCALES: &[&str] = &["it", "en"];

/// Locate the `config/system-prompts/` root directory. Mirrors the
/// `presets_dir` / `config_subdir` pattern in `crate::presets` so the
/// installed-MSI layout (`<install>/config/system-prompts/`) and the
/// dev workspace layout (`<repo>/config/system-prompts/`) both work
/// without an env-var override.
fn root_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MRUST_SYSTEM_PROMPTS_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_for_root(&cwd) {
            return found;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(found) = walk_for_root(&exe) {
            return found;
        }
    }
    PathBuf::from("./config/system-prompts")
}

fn walk_for_root(start: &Path) -> Option<PathBuf> {
    for anc in start.ancestors() {
        let candidate = anc.join("config").join("system-prompts");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Load the `.md` body for `(locale, domain)` walking the fallback
/// chain. Returns `None` when neither the requested pair nor any
/// fallback (`it/<domain>`, `en/<domain>`) exists.
///
/// The returned string is the raw file content trimmed of trailing
/// whitespace — the caller composes it into a wrapper section so the
/// stored `.md` can stay readable as a stand-alone document.
pub fn resolve(locale: &str, domain: &str) -> Option<String> {
    let root = root_dir();
    let domain_safe = sanitize_segment(domain)?;
    // Locale that fails sanitize_segment carries path-traversal or
    // other shell-metacharacter intent — hard reject rather than
    // silently substituting a fallback locale, otherwise a request
    // for `resolve("../etc", "medical")` would happily serve
    // `it/medical.md` (the fallback chain finds it) which masks the
    // attack. Sanitize-passing-but-unknown locales (e.g. "ja") still
    // walk the fallback chain normally — that path is benign.
    let locale_safe = sanitize_segment(locale)?;
    // 1. Requested locale.
    if let Some(text) = try_read(&root, &locale_safe, &domain_safe) {
        return Some(text);
    }
    // 2-3. Fallback locales in order.
    for fb in FALLBACK_LOCALES {
        if *fb == locale_safe {
            continue;
        }
        if let Some(text) = try_read(&root, fb, &domain_safe) {
            return Some(text);
        }
    }
    None
}

/// Map a UI / chat locale to its conventional default country string.
/// Surfaced verbatim in the prologue (`Default country: Italy`) so the
/// model knows what jurisdiction to assume absent explicit signals.
/// The English mapping is intentionally vague ("ask the user") because
/// the en locale legitimately spans US / UK / IE / AU / CA / NZ / IN.
pub fn default_country_for_locale(locale: &str) -> &'static str {
    match locale {
        "it" => "Italy",
        "fr" => "France",
        "de" => "Germany",
        "es" => "Spain",
        "pt" => "Portugal",
        _ => "unspecified (ask the user)",
    }
}

/// Human-facing language name surfaced in the prologue. Mirrors the
/// frontend's locale dropdown.
pub fn language_name_for_locale(locale: &str) -> &'static str {
    match locale {
        "it" => "Italian",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        _ => "English",
    }
}

/// Assemble the full prologue section that gets prepended to
/// `MRUST_SYSTEM_PROMPT`. Wraps the per-domain `.md` body in a
/// metadata header (Domain / Working language / Default country) and
/// a country-disambiguation reminder. Returns an empty string when
/// nothing meaningful can be assembled (no `.md` found AND no domain
/// known) — the caller then skips the section entirely.
pub fn assemble_prologue(locale: &str, domain: &str) -> String {
    let body = resolve(locale, domain).unwrap_or_default();
    let lang = language_name_for_locale(locale);
    let country = default_country_for_locale(locale);
    let mut out = String::new();
    out.push_str(
        "=== Domain context (read this first, it sets your role for this chat) ===\n",
    );
    out.push_str(&format!("Domain: {domain}\n"));
    out.push_str(&format!("Working language: {lang}\n"));
    out.push_str(&format!("Default country / jurisdiction: {country}\n\n"));
    if body.is_empty() {
        out.push_str(
            "No domain-specific guidance is available; behave as a generic professional \
             assistant for this vertical. Cite sources when relevant, defer to the user on \
             jurisdiction-specific decisions.\n",
        );
    } else {
        out.push_str(&body);
        out.push('\n');
    }
    out.push_str(
        "\nCountry disambiguation: If the user's request involves a country, regulation, \
         or legal/medical/professional framework that does not match the default above, \
         ASK the user which country / jurisdiction applies BEFORE giving jurisdiction-\
         specific advice. Do not silently assume.\n",
    );
    out
}

fn try_read(root: &Path, locale: &str, domain: &str) -> Option<String> {
    let path = root.join(locale).join(format!("{domain}.md"));
    if !path.is_file() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        Err(e) => {
            tracing::warn!(
                "[system-prompts] failed to read {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Guard against `..` / absolute path injection through user-supplied
/// locale or domain. Accepts only `[a-zA-Z0-9_-]+`; anything else
/// returns `None` and the caller treats the file as missing.
fn sanitize_segment(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(t.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_tree(files: &[(&str, &str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (locale, domain, body) in files {
            let dir = tmp.path().join(locale);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("{domain}.md")), body).unwrap();
        }
        // SAFETY: tests run sequentially via `cargo test`'s default
        // thread model; the env-var nudge here is observed only by the
        // child resolver call inside this same test.
        unsafe {
            std::env::set_var("MRUST_SYSTEM_PROMPTS_DIR", tmp.path());
        }
        tmp
    }

    #[test]
    fn resolve_uses_requested_locale_when_present() {
        let _tmp = make_tree(&[
            ("it", "medical", "ITALIANO"),
            ("en", "medical", "ENGLISH"),
        ]);
        assert_eq!(resolve("it", "medical").as_deref(), Some("ITALIANO"));
    }

    #[test]
    fn resolve_falls_back_to_italian_when_locale_missing() {
        let _tmp = make_tree(&[("it", "medical", "ITALIANO")]);
        assert_eq!(resolve("fr", "medical").as_deref(), Some("ITALIANO"));
    }

    #[test]
    fn resolve_falls_back_to_english_when_italian_missing() {
        let _tmp = make_tree(&[("en", "medical", "ENGLISH")]);
        assert_eq!(resolve("de", "medical").as_deref(), Some("ENGLISH"));
    }

    #[test]
    fn resolve_returns_none_when_domain_missing_everywhere() {
        let _tmp = make_tree(&[("it", "medical", "ITALIANO")]);
        assert!(resolve("it", "finance").is_none());
    }

    #[test]
    fn resolve_rejects_path_traversal() {
        let _tmp = make_tree(&[("it", "medical", "ITALIANO")]);
        assert!(resolve("../etc", "medical").is_none());
        assert!(resolve("it", "../passwd").is_none());
    }

    #[test]
    fn assemble_prologue_wraps_body() {
        let _tmp = make_tree(&[("it", "medical", "BODY")]);
        let p = assemble_prologue("it", "medical");
        assert!(p.contains("Domain: medical"));
        assert!(p.contains("Working language: Italian"));
        assert!(p.contains("Default country / jurisdiction: Italy"));
        assert!(p.contains("BODY"));
        assert!(p.contains("Country disambiguation"));
    }

    #[test]
    fn assemble_prologue_falls_back_when_md_missing() {
        let _tmp = make_tree(&[("it", "medical", "BODY")]);
        let p = assemble_prologue("it", "ip");
        assert!(p.contains("Domain: ip"));
        assert!(p.contains("No domain-specific guidance"));
        assert!(p.contains("Country disambiguation"));
    }

    #[test]
    fn default_country_for_locale_known_locales() {
        assert_eq!(default_country_for_locale("it"), "Italy");
        assert_eq!(default_country_for_locale("fr"), "France");
        assert_eq!(default_country_for_locale("de"), "Germany");
        assert_eq!(default_country_for_locale("es"), "Spain");
        assert_eq!(default_country_for_locale("pt"), "Portugal");
        assert!(default_country_for_locale("en").contains("ask"));
    }
}
