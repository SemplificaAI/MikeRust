//! CH/Fedlex — Swiss federal legislation via the Fedlex SPARQL
//! endpoint (JOLux ontology).
//!
//! Search is a single SPARQL hop (title CONTAINS filter). Document
//! fetch is two hops: a SPARQL query resolves the HTML manifestation
//! URL for the requested language, then the HTML is downloaded and
//! reduced to plain text. Ported from SuzieLaw's `ch-fedlex` provider
//! — the declarative `http-fetch-per-id` engine can do neither a
//! SPARQL POST nor a two-hop fetch, so Fedlex ships as a builtin
//! adapter alongside EUR-Lex and Italian-Legal.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;

use super::{CorpusDocument, CorpusHit, LegalCorpusAdapter};

const ENDPOINT: &str = "https://fedlex.data.admin.ch/sparqlendpoint";
const PREFIXES: &str = concat!(
    "PREFIX jolux: <http://data.legilux.public.lu/resource/ontology/jolux#>\n",
    "PREFIX ft: <http://publications.europa.eu/resource/authority/file-type/>"
);

/// ISO-639-1 codes Fedlex serves an HTML manifestation in.
const LANGUAGES: &[&str] = &["de", "fr", "it", "en"];

/// Map a short language code to the EU Publications Office language
/// IRI that JOLux `jolux:language` points at.
fn lang_iri(lang: &str) -> &'static str {
    match lang {
        "fr" => "http://publications.europa.eu/resource/authority/language/FRA",
        "it" => "http://publications.europa.eu/resource/authority/language/ITA",
        "en" => "http://publications.europa.eu/resource/authority/language/ENG",
        _ => "http://publications.europa.eu/resource/authority/language/DEU",
    }
}

/// Escape a user string for safe embedding in a SPARQL double-quoted
/// literal — backslash + quote, and newlines flattened so a pasted
/// multi-line query can't break out of the literal.
fn sparql_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

/// Reduce an HTML page to plain text — the `<body>` text, whitespace
/// collapsed. Matches what the declarative engine's CSS `body` selector
/// produces for the other corpora.
fn html_to_text(html: &str) -> String {
    let doc = scraper::Html::parse_document(html);
    let body = scraper::Selector::parse("body").expect("static selector");
    let raw: String = match doc.select(&body).next() {
        Some(el) => el.text().collect::<Vec<_>>().join(" "),
        None => doc.root_element().text().collect::<Vec<_>>().join(" "),
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

// SPARQL 1.1 Query Results JSON Format — we only need the values.
#[derive(Deserialize, Default)]
struct SparqlResponse {
    #[serde(default)]
    results: SparqlResults,
}
#[derive(Deserialize, Default)]
struct SparqlResults {
    #[serde(default)]
    bindings: Vec<HashMap<String, SparqlValue>>,
}
#[derive(Deserialize)]
struct SparqlValue {
    value: String,
}

type Binding = HashMap<String, SparqlValue>;

/// Builtin corpus adapter for Fedlex. Registered into the adapter
/// registry under `ch-fedlex`, so the generic `/corpora/:id/*` routes
/// serve it like any declarative corpus.
pub struct FedlexAdapter {
    client: reqwest::Client,
}

impl FedlexAdapter {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .expect("reqwest client init");
        Self { client }
    }

    /// POST a SPARQL SELECT and return its result bindings.
    async fn sparql(&self, query: &str) -> Result<Vec<Binding>> {
        let full = format!("{PREFIXES}\n{query}");
        let resp = self
            .client
            .post(ENDPOINT)
            .header(
                reqwest::header::ACCEPT,
                "application/sparql-results+json",
            )
            .form(&[("query", full.as_str())])
            .send()
            .await
            .context("Fedlex SPARQL request")?;
        let status = resp.status();
        let body = resp.text().await.context("Fedlex SPARQL body")?;
        if !status.is_success() {
            bail!(
                "Fedlex SPARQL HTTP {}: {}",
                status.as_u16(),
                body.chars().take(300).collect::<String>()
            );
        }
        let parsed: SparqlResponse =
            serde_json::from_str(&body).context("Fedlex SPARQL JSON decode")?;
        Ok(parsed.results.bindings)
    }
}

impl Default for FedlexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LegalCorpusAdapter for FedlexAdapter {
    fn id(&self) -> &'static str {
        "ch-fedlex"
    }

    fn languages(&self) -> &[&'static str] {
        LANGUAGES
    }

    async fn search_by_keyword(
        &self,
        query: &str,
        _language: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CorpusHit>> {
        let esc = sparql_escape(query);
        let q = format!(
            "SELECT DISTINCT ?act ?title ?date WHERE {{\n\
               ?act jolux:isRealizedBy ?expr .\n\
               ?expr jolux:title ?title .\n\
               OPTIONAL {{ ?act jolux:dateDocument ?date }}\n\
               FILTER(CONTAINS(LCASE(STR(?title)), LCASE(\"{esc}\")))\n\
             }}\nORDER BY DESC(?date)\nLIMIT {}",
            limit.clamp(1, 50),
        );
        let bindings = self.sparql(&q).await?;
        Ok(bindings
            .into_iter()
            .filter_map(|b| {
                let act = b.get("act")?.value.clone();
                let title = b
                    .get("title")
                    .map(|v| v.value.clone())
                    .unwrap_or_else(|| act.clone());
                let date = b.get("date").map(|v| v.value.clone());
                Some(CorpusHit {
                    identifier: act.clone(),
                    title,
                    date,
                    url: act,
                    languages_available: Vec::new(),
                })
            })
            .collect())
    }

    async fn search_by_id(
        &self,
        identifier: &str,
        _language: Option<&str>,
    ) -> Result<Vec<CorpusHit>> {
        let iri = identifier.trim();
        // Fedlex identifiers are ELI IRIs; anything else can't resolve.
        if !iri.starts_with("http") {
            return Ok(Vec::new());
        }
        let q = format!(
            "SELECT ?title ?date WHERE {{\n\
               <{iri}> jolux:isRealizedBy ?expr .\n\
               ?expr jolux:title ?title .\n\
               OPTIONAL {{ <{iri}> jolux:dateDocument ?date }}\n\
             }}\nLIMIT 1"
        );
        let bindings = self.sparql(&q).await?;
        Ok(bindings
            .into_iter()
            .take(1)
            .map(|b| {
                let title = b
                    .get("title")
                    .map(|v| v.value.clone())
                    .unwrap_or_else(|| iri.to_string());
                let date = b.get("date").map(|v| v.value.clone());
                CorpusHit {
                    identifier: iri.to_string(),
                    title,
                    date,
                    url: iri.to_string(),
                    languages_available: Vec::new(),
                }
            })
            .collect())
    }

    async fn fetch(
        &self,
        identifier: &str,
        language: Option<&str>,
        fallback_en: bool,
    ) -> Result<CorpusDocument> {
        let iri = identifier.trim();
        let requested = language.unwrap_or("de").to_ascii_lowercase();
        // Try the requested language; if the act has no manifestation
        // there, fall back to German (Fedlex's most complete language).
        let mut attempts = vec![requested.clone()];
        if fallback_en && requested != "de" {
            attempts.push("de".to_string());
        }

        let mut last_err =
            format!("no HTML manifestation found for {iri}");
        for (i, lang) in attempts.iter().enumerate() {
            let q = format!(
                "SELECT ?fileUrl ?title ?date WHERE {{\n\
                   <{iri}> jolux:isRealizedBy ?expr .\n\
                   ?expr jolux:language <{}> .\n\
                   ?expr jolux:title ?title .\n\
                   OPTIONAL {{ <{iri}> jolux:dateDocument ?date }}\n\
                   ?expr jolux:isEmbodiedBy ?manif .\n\
                   ?manif jolux:format ft:HTML ;\n\
                          jolux:isExemplifiedBy ?fileUrl .\n\
                 }}\nLIMIT 1",
                lang_iri(lang),
            );
            let bindings = self.sparql(&q).await?;
            let Some(b) = bindings.into_iter().next() else {
                last_err = format!(
                    "no HTML manifestation for {iri} in language '{lang}'"
                );
                continue;
            };
            let Some(file_url) = b.get("fileUrl").map(|v| v.value.clone())
            else {
                last_err =
                    format!("manifestation for {iri} '{lang}' has no file URL");
                continue;
            };
            let title = b
                .get("title")
                .map(|v| v.value.clone())
                .unwrap_or_else(|| iri.to_string());
            let date = b.get("date").map(|v| v.value.clone());

            // Hop 2 — download the HTML manifestation, reduce to text.
            let resp = self
                .client
                .get(&file_url)
                .send()
                .await
                .with_context(|| format!("Fedlex HTML GET {file_url}"))?;
            if !resp.status().is_success() {
                bail!(
                    "Fedlex HTML HTTP {} from {file_url}",
                    resp.status().as_u16()
                );
            }
            let html = resp
                .text()
                .await
                .with_context(|| format!("Fedlex HTML decode {file_url}"))?;

            return Ok(CorpusDocument {
                identifier: iri.to_string(),
                title,
                date,
                language: lang.clone(),
                fetched_with_fallback: i > 0,
                bytes: html_to_text(&html).into_bytes(),
                mime: "text/plain; charset=utf-8",
                source_url: file_url,
            });
        }
        bail!("Fedlex fetch failed: {last_err}")
    }
}
