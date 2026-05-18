# PLAN_FONTI_INTERNAZIONALI

## Scopo
Questo documento definisce, per ogni fonte internazionale attualmente registrata in SuzieLaw (`apps/suzielaw/src/tools/legal-research/providers/*`), i requisiti minimi per implementare:
- download del documento completo
- ricerca documentale
- UI di scelta fonte

Per ogni fonte sono riportati:
- Modalità di download
- Autenticazione
- Formato dati (search/fetch)
- Specifiche tecniche operative (doc_id, vincoli, fallback)
- Indicazioni UI (quando mostrarla, come guidare l’utente)

---

## Regole UI trasversali (tutte le fonti)
- Filtri base: `jurisdiction`, `type` (`legislation`/`case_law`), `source_id` opzionale.
- Mostrare badge per fonte:
  - `API Key` / `OAuth2` / `Public`
  - `Free-text` / `Citation-only` / `Date-window`
  - `HTML` / `XML` / `JSON` / `SPARQL` / `ZIP(EPUB)`
- Mostrare in UI la sintassi `doc_id` richiesta dalla fonte (tooltip o help inline).
- Fonti auth-gated devono essere disabilitate se credenziali assenti (con CTA “Configura credenziali”).
- Search può essere multi-source per giurisdizione; `getDocument` deve sempre passare da `source_id + doc_id` restituiti dalla search.

---

## Schede per fonte

### AR/InfoLEG (AR, legislation)
- Download: GET HTML su `verNorma.do?id=...` e testo su `/anexos/<range>/<id>/(texact|norma).htm`.
- Auth: nessuna API key, ma serve cookie sessione `JSESSIONID` per POST search.
- Formato: search HTML (`windows-1252`), fetch HTML (`windows-1252`).
- Specifiche tecniche: POST `buscarNormas.do` richiede almeno 2 criteri (`norm_type/number/year/text`); fallback `consolidated -> original`; `doc_id = infoleg_id`.
- UI scelta: mostra hint “inserire numero norma e/o anno”; warning se query troppo generica.

### AT/RIS (AT, legislation+case_law)
- Download: search JSON API RIS (`/Bundesrecht`, `/Judikatur`), fetch HTML su URL documento RIS.
- Auth: pubblica (no key).
- Formato: search JSON, fetch HTML.
- Specifiche tecniche: `doc_id` prefissato `leg:<ID>` o `case:<ID>`; doppio endpoint con `Applikation=BrKons|Justiz`.
- UI scelta: toggle tipo `legislation/case_law`; nessun prerequisito auth.

### AU/FederalRegister (AU, legislation)
- Download: OData search titoli -> risoluzione versione documento -> download binario (EPUB preferito, Word fallback).
- Auth: pubblica (no key).
- Formato: JSON OData + ZIP EPUB (XHTML interno) / Word binary.
- Specifiche tecniche: endpoint `/v1/titles`, `/v1/Documents`, `/v1/documents(...)`; conversione EPUB tramite unzip; Word legacy non decodificabile inline.
- UI scelta: badge “EPUB preferred”; in caso Word mostra link viewer come fallback.

### BE/Justel (BE, legislation)
- Download: year listing ELI + fetch pagina Justel.
- Auth: pubblica.
- Formato: HTML (`iso-8859-1`).
- Specifiche tecniche: no full-text API; ricerca tramite scansione annuale e filtro keyword; `doc_id=<type>/<YYYY>/<MM>/<DD>/<numac>`.
- UI scelta: richiedere anno in query (consigliato); default scan ultimi 5 anni.

### BR/Planalto (BR, legislation)
- Download: search su LexML HTML (URN), fetch testo via URL pattern Planalto.
- Auth: pubblica.
- Formato: search HTML, fetch HTML (`iso-8859-1`).
- Specifiche tecniche: `doc_id=URN`; mapping URN->URL Planalto con più template; fallback a reference LexML se testo non risolto.
- UI scelta: mostrare che alcune URN possono non risolversi in full-text diretto.

### CA/Justice (CA, legislation)
- Download: carica indice XML completo (`Legis.xml`) e fetch XML per `UniqueId`.
- Auth: pubblica.
- Formato: XML.
- Specifiche tecniche: cache TOC in memoria (~5MB); filtro title client-side; `doc_id=UniqueId` (es. `P-21`).
- UI scelta: badge “index cached”; ricerca title-based.

### CH/Fedlex (CH, legislation)
- Download: SPARQL search + SPARQL resolve `HTML manifestation` + fetch HTML.
- Auth: pubblica.
- Formato: SPARQL JSON + HTML.
- Specifiche tecniche: endpoint `sparqlendpoint`; lingua selezionabile via `version` (`de/fr/it/en`); `doc_id=IRI ELI`.
- UI scelta: selettore lingua documento (default `de`).

### CoE/HUDOC (CoE, case_law)
- Download: RSS search (`transform/rss`) + fetch full text (`conversion/docx/html/body`).
- Auth: pubblica.
- Formato: XML RSS + HTML.
- Specifiche tecniche: query Lucene-like; filtro doctype per evitare press release; `doc_id=itemid`.
- UI scelta: campo avanzato query (frasi, operatori), badge “ECHR only”.

### DE/GesetzeImInternet (DE, legislation)
- Download: lookup diretto se citazione (`§ N CODE`) o cache XML.zip codice e grep locale sezioni.
- Auth: pubblica.
- Formato: XML.zip (DEFLATE) + HTML sezione.
- Specifiche tecniche: `doc_id=<CODE>:<num>`; unzip singolo file XML; modalità citation e keyword; cache per codice.
- UI scelta: modalità guidata “citazione” vs “keyword”; suggerire abbreviazione codice (`BGB`, `StGB`, ...).

### DE/OpenLegalData (DE, case_law)
- Download: search REST `/cases/search/` e fetch `/cases/{id}/`.
- Auth: pubblica.
- Formato: JSON.
- Specifiche tecniche: backend search può dare `503 search_backend_unavailable`; fetch dettaglio spesso disponibile anche quando search è giù; `doc_id=case:<id>`.
- UI scelta: mostrare stato degradato quando search backend non disponibile.

### ES/BOE (ES, legislation)
- Download: search HTML frontend BOE, fetch HTML (`act.php`) con fallback XML.
- Auth: pubblica.
- Formato: HTML + XML fallback.
- Specifiche tecniche: estrazione ID `BOE-A-YYYY-NNNNN`; `doc_id=BOE-A-...`; supporta `findInDocument` per articoli.
- UI scelta: ottimo per keyword su titoli; mostrare ID BOE esplicito nei risultati.

### EU/CURIA (EU, case_law)
- Download: SPARQL per casi CJEU + fetch CELLAR/CELEX via content negotiation.
- Auth: pubblica.
- Formato: SPARQL JSON + HTML.
- Specifiche tecniche: filtro resource types (`JUDG/ORDER/OPIN_AG`), CELEX settore 6; header `Accept-Language` obbligatorio su fetch.
- UI scelta: filtro date disponibile; tipo case-law predefinito.

### EU/EUR-Lex (EU, legislation+case_law)
- Download: SPARQL search + fetch CELEX via CELLAR.
- Auth: pubblica.
- Formato: SPARQL JSON + HTML.
- Specifiche tecniche: mapping tipo su CELEX sectors (`3/1` legislation, `6` case law); `Accept-Language` obbligatorio.
- UI scelta: fonte primaria EU; permette entrambi i tipi.

### FR/Judilibre (FR, case_law)
- Download: API search `/search`, fetch `/decision?id=...`.
- Auth: API key (`KeyId` header).
- Formato: JSON.
- Specifiche tecniche: provider disponibile solo con `judilibreApiKey`; `page` zero-based in API; `doc_id=id decisione`.
- UI scelta: mostrare badge “API Key required”; disabilitare se key assente.

### FR/Legifrance (FR, legislation)
- Download: OAuth2 token (`client_credentials`) -> API `/search` e `/consult/getArticle`.
- Auth: OAuth2 (`client_id/client_secret`).
- Formato: JSON.
- Specifiche tecniche: token cache con expiry; corpus `CODE_DATE_VERSION`; `doc_id=LEGIARTI...`.
- UI scelta: badge “OAuth2 required”; helper per onboarding credenziali PISTE.

### IE/IrishStatuteBook (IE, legislation)
- Download: citation-style search e fetch ELI HTML (`revised` con fallback `enacted`).
- Auth: pubblica.
- Formato: HTML.
- Specifiche tecniche: non ha free-text API; `doc_id=<year>/act/<n>` o `/si/`; resolver regex citazioni.
- UI scelta: evidenziare modalità `Citation-only`.

### IN/IndianKanoon (IN, legislation+case_law)
- Download: POST form search `/search/`, POST fetch `/doc/{tid}/`.
- Auth: API key (`Authorization: Token ...`).
- Formato: JSON + HTML body in campo `doc`.
- Specifiche tecniche: provider env-gated; doctypes (`judgments`, `centralacts`); `doc_id=tid`.
- UI scelta: badge “API Key required”; filtro tipo mappato su doctypes.

### IT/Normattiva (IT, legislation)
- Download: search HTML (`ricerca/semplice`), fetch ELI (`/CONSOLIDATED` fallback base path).
- Auth: pubblica.
- Formato: HTML.
- Specifiche tecniche: no API ufficiale; parsing ELI path; `doc_id=/eli/id/YYYY/MM/DD/<codice>/sg`; supporta `findInDocument`.
- UI scelta: mostrare codice redazionale/ELI nei risultati.

### JP/eGov (JP, legislation)
- Download: cache lawlists category 1..4, fetch lawdata per `LawId`.
- Auth: pubblica.
- Formato: XML.
- Specifiche tecniche: ricerca su nome legge client-side; testo solo giapponese; `doc_id=LawId`.
- UI scelta: indicare chiaramente lingua solo JP.

### MX/DOF (MX, legislation)
- Download: summary giornaliero JSON + fetch pagina dettaglio HTML.
- Auth: pubblica.
- Formato: JSON + HTML.
- Specifiche tecniche: no free-text globale; scansione finestra date (default 14 giorni); `doc_id=<DD-MM-YYYY>:<codNota>`.
- UI scelta: input data consigliato; badge `Date-window search`.

### NL/Rechtspraak (NL, case_law)
- Download: feed Atom (`zoeken`) + fetch XML contenuto per ECLI.
- Auth: pubblica.
- Formato: Atom XML + XML contenuto.
- Specifiche tecniche: API non supporta full-text query; keyword filter client-side; `doc_id=ECLI`.
- UI scelta: esporre filtri data e avviso “keyword filter lato client”.

### NL/Wetten (NL, legislation)
- Download: SRU search (`x-connection=BWB`) + fetch HTML su wetten.overheid.nl.
- Auth: pubblica.
- Formato: XML SRU + HTML.
- Specifiche tecniche: query CQL title-based con AND termini; `doc_id=BWBR...`.
- UI scelta: mostrare `BWBR` come identificatore primario.

### UK/FindCaseLaw (UK, case_law)
- Download: search Atom (`atom.xml`) + fetch HTML decisione.
- Auth: pubblica.
- Formato: Atom XML + HTML.
- Specifiche tecniche: `doc_id=uri` (es. `ewhc/ch/2026/694`); filtri data supportati.
- UI scelta: ideale per case law UK; mostrare neutral citation quando disponibile.

### UK/Legislation (UK, legislation)
- Download: search feed Atom (`all/data.feed`) + fetch HTML pagina legge.
- Auth: pubblica.
- Formato: Atom XML + HTML.
- Specifiche tecniche: `doc_id=path` (es. `ukpga/1998/29`); parser da feed id.
- UI scelta: suggerire path categories (`ukpga`, `uksi`, ecc.) per utenti avanzati.

### US/CourtListener (US, case_law)
- Download: REST search `/search/?type=o`, fetch `/opinions/{id}/`.
- Auth: opzionale token (`Authorization: Token`), altrimenti tier pubblico.
- Formato: JSON.
- Specifiche tecniche: `doc_id=id opinione`; metadati ricchi (court, citation, judge).
- UI scelta: badge “Token opzionale (rate limit migliore)”.

### US/CFR (US, legislation)
- Download: eCFR search API + fetch XML full per title/issue date/hierarchy.
- Auth: pubblica.
- Formato: JSON search + XML fetch.
- Specifiche tecniche: cache titoli per `up_to_date_as_of`; `doc_id` gerarchico (`title=..|part=..|section=..`); date issue obbligatoria per evitare 404.
- UI scelta: mostrare citation CFR generata e livello gerarchico del risultato.

---

## Fonti condizionate da credenziali (gating)
- FR/Legifrance: richiede `pisteClientId` + `pisteClientSecret`.
- FR/Judilibre: richiede `judilibreApiKey`.
- IN/IndianKanoon: richiede `indianKanoonApiKey`.
- US/CourtListener: token opzionale (non obbligatorio).

---

## Requisiti minimi UI di scelta fonte (implementativi)
- Selettori:
  - `Jurisdiction` (lista da provider registrati)
  - `Type` (`legislation` / `case_law`)
  - `Source` (opzionale; filtrata da jurisdiction+type)
- Badge per fonte in dropdown:
  - `AUTH` (`Public`, `API Key`, `OAuth2`, `Optional Token`)
  - `SEARCH MODE` (`Free-text`, `Citation-only`, `Date-window`, `SPARQL`)
  - `FETCH FORMAT` (`HTML`, `XML`, `JSON`, `ZIP/EPUB`)
- Hint dinamici query:
  - IE: esempi citazione (`Act 7 of 2018`, `2018/act/7`)
  - MX/DOF: data raccomandata (`DD/MM/YYYY`)
  - AR/InfoLEG: numero norma/anno consigliati
  - DE/GesetzeImInternet: formato citazione (`§ 535 BGB`)
- Gestione errori specifici:
  - DE/OpenLegalData: backend search indisponibile
  - AU/FederalRegister: documento solo `.doc` legacy
  - CH/EU fetch: language/content negotiation

---

## Nota di manutenzione
Quando si aggiunge una nuova fonte, aggiornare in parallelo:
- registrazione provider in `apps/suzielaw/src/tools/legal-research/index.ts`
- questa matrice (`PLAN_FONTI_INTERNAZIONALI.md`)
- eventuali hint/query examples nella UI di selezione fonte
