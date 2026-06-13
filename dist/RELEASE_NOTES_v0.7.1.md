# MikeRust v0.7.1 — Localizzazione italiana del verticale legale

Traduce in italiano i workflow e i column-preset del **settore legale**,
ultimo contenuto rimasto in inglese nel catalogo (i preset legali
derivavano dai template internazionali del fork upstream; medico,
finanza, fiscale, assicurazioni, PA e compliance macchine erano già in
italiano).

Include tutto il lavoro di v0.7.0 (nuovo settore Fiscale) — vedi le
release notes di v0.7.0 — più la localizzazione legale qui sotto.

## Cosa cambia

### 27 file legali tradotti

**14 workflow preset** (`config/workflow-presets/legal/`):
revisione NDA, patto parasociale (SHA) + sintesi, compravendita
partecipazioni (SPA), contratto commerciale, locazione commerciale,
contratto di lavoro subordinato, contratto di fornitura, contratto di
finanziamento + sintesi, regolamento del fondo (LPA), checklist
condizioni sospensive, verifica clausole di cambio di controllo,
e-Discovery.

**13 column preset** (`config/column-presets/legal/`):
parti, riservatezza, legge applicabile, foro competente, durata,
risoluzione e recesso, manleva, dichiarazioni e garanzie, forza
maggiore, cessione, modifiche, data di efficacia, pagamenti e
corrispettivi, cambio di controllo.

### Tradotto vs invariato

Tradotti in registro legale italiano: `title`, `practice`, `prompt_md`,
ogni `name` + `prompt` di colonna, e i valori display dei `tags` (es.
NDA `["Mutual","Unilateral"]` → `["Reciproco","Unilaterale"]`).

Lasciati invariati (identificatori canonici): `id`, `type`, `domain`,
`index`, `format`, e il `match_pattern`/`match_flags` dei column-preset
(il regex è un matcher, non testo visualizzato — resta bilingue).

I concetti specifici di ordinamento sono stati adattati al sistema
italiano dove rilevante (es. locazione commerciale: tutela della
stabilità del rapporto ex L. 392/1978; indicizzazione ISTAT; segreto
professionale). I termini di mercato e da prassi PE (SOFR, EURIBOR,
make-whole, carried interest, hurdle, waterfall, locked box) sono
mantenuti come d'uso tra i professionisti italiani.

## TODO in standby

L'integrazione delle **banche dati pubbliche di norme e sentenze**
(def.finanze.it, Sentenze Web Cassazione, Giustizia Tributaria DGT)
come corpora consultabili in chat è **parcheggiata su richiesta**:
annotata in `docs/piano_settore_fiscale.md` §6 e roadmap Fase 3.
Richiede un `LegalCorpusAdapter` per fonte, non un semplice file di
config — sarà un workstream dedicato futuro.

## Note di migrazione

Nessuna migrazione di schema; nessun cambio di codice. I preset sono
file in `config/` letti all'avvio: installando il nuovo MSI il settore
legale appare in italiano e il settore Fiscale (v0.7.0) è disponibile.

## Download

MSI Windows precompilati:

- `MikeRust_0.7.1_x64.msi` — Windows x86_64
- `MikeRust_0.7.1_arm64.msi` — Windows ARM64, Snapdragon X Elite

Sostituzione drop-in per v0.6.7 (supera v0.7.0, non distribuito come MSI).

## Licenza

MikeRust è distribuito sotto **AGPL-3.0-only**. Il marchio e il logo
Semplifica sono marchi registrati; vedi `NOTICE.md`.
