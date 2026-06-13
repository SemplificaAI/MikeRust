# MikeRust v0.7.2 — Settore Fiscale: analisi del bilancio ai fini fiscali

Colma un buco emerso in test: il settore **Fiscale** non aveva
workflow sul bilancio, costringendo a usare la «Riclassificazione
bilanci» del settore Finanza. L'analisi del bilancio esiste
legittimamente in due settori con tagli complementari — **Finanza**
fa la lettura gestionale/valutativa (riclassificazione, indici,
valutazione, crisi), **Fiscale** mancava della lettura **tributaria**
(dal risultato civilistico al reddito imponibile, base IRAP,
fiscalità differita). v0.7.2 aggiunge quest'ultima.

## Tre nuovi workflow Fiscale

- **Analisi fiscale del bilancio** (assistant) — legge il bilancio ai
  fini delle imposte secondo il principio di derivazione (art. 83
  TUIR): risultato civilistico, poste a rilevanza fiscale
  (ammortamenti, svalutazione crediti, spese di rappresentanza,
  interessi passivi/ROL, compensi amministratori, auto, plus/
  minusvalenze, IMU/IRAP), stima IRES, cenno base IRAP, fiscalità
  differita/anticipata (OIC 25), disclaimer professionale.
- **Riconciliazione civilistico-fiscale (variazioni IRES — Quadro RF)**
  (tabular) — una riga per variazione in aumento/diminuzione dal
  risultato civilistico al reddito imponibile, con articolo TUIR,
  importo, segno e rigo RF.
- **Determinazione base imponibile IRAP da bilancio** (tabular) —
  base IRAP dal Conto Economico ex art. 5 D.Lgs. 446/97: componenti
  del valore della produzione, voci escluse (personale, svalutazioni,
  accantonamenti), deduzioni spettanti (cuneo fiscale, deduzione
  integrale dipendenti a tempo indeterminato).

Il settore Fiscale ora include **11 workflow** (4 assistant + 7
tabular).

## Note

- Nessuna migrazione di schema; preset = file in `config/` letti
  all'avvio. 40/40 test preset-loader verdi.
- `docs/piano_settore_fiscale.md` §3 aggiornato con i nuovi workflow
  e la nota sull'angolazione fiscale vs finanziaria del bilancio.

## Download

- `MikeRust_0.7.2_x64.msi` — Windows x86_64
- `MikeRust_0.7.2_arm64.msi` — Windows ARM64, Snapdragon X Elite

Sostituzione drop-in per v0.7.1.

## Licenza

AGPL-3.0-only. Il marchio e il logo Semplifica sono marchi
registrati; vedi `NOTICE.md`.
