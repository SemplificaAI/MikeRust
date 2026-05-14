# Piano strategico — Toolkit AI per il Commercialista

> **Destinatari:** Dottori commercialisti, esperti contabili, revisori legali, CTU tributari, attestatori ex CCII  
> **Versione:** 1.0 — Maggio 2026  
> **Scopo:** Descrivere in modo completo i workflow operativi e i processi di estrazione tabellare a supporto dell'attività professionale del commercialista, assistita da intelligenza artificiale.

---

## Indice

1. [Visione generale e architettura del sistema](#1-visione-generale)
2. [Area 1 — Perizie e stime di valore](#2-area-1)
3. [Area 2 — CTU tributaria](#3-area-2)
4. [Area 3 — Crisi d'impresa e procedure concorsuali](#4-area-3)
5. [Area 4 — Due diligence fiscale e societaria](#5-area-4)
6. [Area 5 — Contenzioso tributario](#6-area-5)
7. [Area 6 — Dichiarazioni e adempimenti periodici](#7-area-6)
8. [Struttura delle tabelle — Riferimento rapido](#8-tabelle)
9. [Riferimenti normativi e tabellari](#9-riferimenti)
10. [Roadmap di implementazione](#10-roadmap)

---

## 1. Visione generale e architettura del sistema {#1-visione-generale}

### 1.1 Obiettivo

Costruire un sistema modulare assistito da AI che supporti il commercialista in tutte le fasi del lavoro professionale: dall'acquisizione documentale alla produzione di relazioni tecniche, con estrazione automatica di dati contabili e fiscali, costruzione di cronologie, tabelle comparative e quantificazioni con riferimento alle norme vigenti.

Il sistema non sostituisce il giudizio professionale: elimina il lavoro manuale ripetitivo (raccolta dati, trascrizione, ricerca normativa) e riduce il rischio di omissioni documentali o di calcolo.

### 1.2 Principi operativi

| Principio | Descrizione |
|-----------|-------------|
| **Tracciabilità** | Ogni dato estratto è collegato al documento sorgente con codice di riferimento |
| **Revisione professionale** | Il commercialista valida ogni modulo prima di procedere al successivo |
| **Neutralità valutativa** | L'AI presenta i dati, il professionista esprime il giudizio tecnico |
| **Completezza documentale** | Il sistema segnala gap, incongruenze e documenti non valorizzati |
| **Aderenza normativa** | Ogni quantificazione fa riferimento esplicito alla norma o alla prassi applicata |
| **Aggiornamento continuo** | Il sistema segnala quando una norma citata è stata modificata o abrogata |

### 1.3 Architettura generale

```
┌──────────────────────────────────────────────────────────────┐
│                      INPUT DOCUMENTALE                        │
│  Bilanci · Dichiarazioni · Atti notarili · Cartelle          │
│  Contratti · Estratti conto · Verbali · Perizie precedenti   │
└──────────────────────────┬───────────────────────────────────┘
                           │
          ┌────────────────▼──────────────────┐
          │     CLASSIFICAZIONE DOCUMENTALE    │
          │     (comune a tutte le aree)       │
          └────────────────┬──────────────────┘
                           │
     ┌─────────────────────┼─────────────────────┐
     │         │           │          │           │           │
     ▼         ▼           ▼          ▼           ▼           ▼
  AREA 1    AREA 2      AREA 3     AREA 4      AREA 5      AREA 6
  Perizie   CTU trib.   Crisi      Due dilig.  Contenz.    Ademp.
  & stime              impresa    fiscale     tributario  periodici
     │         │           │          │           │           │
     └─────────┴───────────┴──────────┴───────────┴───────────┘
                           │
          ┌────────────────▼──────────────────┐
          │         OUTPUT PROFESSIONALE       │
          │  Relazioni · Tabelle · Checklist   │
          │  Ricorsi · Piani · Scadenzari      │
          └───────────────────────────────────┘
```

### 1.4 Modulo trasversale — Classificazione documentale

Prima di entrare in qualsiasi area, tutti i documenti vengono classificati con la seguente procedura:

```
[1] Identificazione tipo documento
    · Bilancio (CEE, OIC, IFRS) / Nota integrativa / Relazione gestione
    · Dichiarazione fiscale (Redditi, IVA, IRAP, 770, CU)
    · Atto notarile (costituzione, cessione quote, fusione, scissione)
    · Estratto conto bancario / movimentazione
    · Contratto (compravendita, affitto, finanziamento, leasing)
    · Cartella esattoriale / avviso di accertamento / PVC
    · Verbale CdA / assemblea soci
    · Perizia precedente / relazione tecnica

[2] Estrazione metadati
    · Data documento / periodo di riferimento
    · Soggetto emittente e destinatario
    · Importi principali (se rilevabili in modo diretto)
    · Riferimento normativo eventualmente citato

[3] Tagging per area di lavoro
    · Assegnazione automatica all'area 1–6 pertinente
    · Flag priorità (urgente / ordinario / archivio)

[4] Inserimento tabella inventario
```

**Tabella inventario documenti (comune a tutte le aree)**

| N° | Data | Tipo | Emittente | Periodo rif. | Importo principale | Area | Priorità | Ref. |
|----|------|------|-----------|-------------|-------------------|------|----------|------|
| 1 | gg/mm/aaaa | Bilancio d'esercizio | Società X Srl | 2024 | PN: €… | Area 1/3/4 | Ordinario | DOC-01 |
| 2 | gg/mm/aaaa | Avviso accertamento | ADE — Uff. Y | 2021 | €… | Area 2/5 | Urgente | DOC-02 |
| … | … | … | … | … | … | … | … | … |

---

## 2. Area 1 — Perizie e stime di valore {#2-area-1}

### 2.1 Ambito

Relazioni di stima per: cessione di azienda o ramo, conferimento in società, fusione/scissione, donazione o successione, affrancamento fiscale, determinazione del prezzo in sede giudiziale, perizie asseverate per garanzie bancarie.

### 2.2 Workflow principale

```
[1] Acquisizione documentale
    · Bilanci ultimi 3–5 esercizi (CEE, OIC, IFRS se applicabile)
    · Nota integrativa e relazione sulla gestione
    · Visura camerale aggiornata
    · Statuto e patti parasociali
    · Contratti significativi (affitti, licenze, finanziamenti)
    · Piano industriale / business plan se disponibile
        │
        ▼
[2] Analisi e riclassificazione bilanci
    · Riclassificazione SP (criterio finanziario o funzionale)
    · Riclassificazione CE (valore aggiunto o ricavi-costi)
    · Normalizzazione dei valori (eliminazione componenti straordinari,
      proventi/oneri non ricorrenti, rettifiche di competenza)
        │
        ▼
[3] Calcolo indicatori economico-finanziari
    · Indici di redditività: ROE, ROI, ROA, ROS, EBITDA margin
    · Indici di liquidità: current ratio, quick ratio, CCN
    · Indici di struttura: PFN/EBITDA, debt/equity, indice autonomia finanziaria
    · Indici di efficienza: rotazione magazzino, DSO, DPO
        │
        ▼
[4] Selezione e applicazione metodi di valutazione
    · Metodo patrimoniale semplice / complesso
    · Metodo reddituale (rendita perpetua / a periodo limitato)
    · Metodo misto patrimoniale-reddituale (UEC, EVA)
    · Metodo DCF (Discounted Cash Flow)
    · Metodo dei multipli di mercato (EV/EBITDA, P/E, EV/Sales)
        │
        ▼
[5] Riconciliazione e range di valore
    · Confronto risultati tra metodi applicati
    · Determinazione range min-max
    · Proposta di valore con motivazione
        │
        ▼
[6] Stesura relazione di stima
```

### 2.3 Tabella riclassificazione bilanci pluriennale

| Voce | 2020 | 2021 | 2022 | 2023 | 2024 | Var. % 24/20 | Note |
|------|------|------|------|------|------|-------------|------|
| Ricavi netti | — | — | — | — | — | — | |
| Costo del venduto | — | — | — | — | — | — | |
| **Valore aggiunto** | — | — | — | — | — | — | |
| Costo del lavoro | — | — | — | — | — | — | |
| **EBITDA** | — | — | — | — | — | — | |
| Ammortamenti | — | — | — | — | — | — | |
| **EBIT** | — | — | — | — | — | — | |
| Oneri finanziari netti | — | — | — | — | — | — | |
| **EBT** | — | — | — | — | — | — | |
| Imposte | — | — | — | — | — | — | |
| **Utile netto** | — | — | — | — | — | — | |

### 2.4 Tabella indicatori economico-finanziari

| Indicatore | Formula | 2022 | 2023 | 2024 | Media | Benchmark settore | Giudizio |
|------------|---------|------|------|------|-------|-------------------|---------|
| ROE | Utile netto / PN | —% | —% | —% | —% | —% | |
| ROI | EBIT / Capitale investito | —% | —% | —% | —% | —% | |
| EBITDA margin | EBITDA / Ricavi | —% | —% | —% | —% | —% | |
| PFN / EBITDA | PFN / EBITDA | — | — | — | — | < 3x | |
| Current ratio | Att. corr. / Pass. corr. | — | — | — | — | > 1,2 | |
| DSO | Crediti / (Ricavi/365) | — | — | — | — | — gg | |

### 2.5 Tabella applicazione metodi valutativi

| Metodo | Base di calcolo | Parametro chiave | Valore ottenuto | Peso assegnato | Valore ponderato |
|--------|----------------|-----------------|----------------|---------------|-----------------|
| Patrimoniale semplice | PN rettificato | — | €… | —% | €… |
| Reddituale | Reddito medio norm. | Tasso cap. r = —% | €… | —% | €… |
| DCF | Free Cash Flow prev. | WACC = —% | €… | —% | €… |
| Multipli EV/EBITDA | EBITDA norm. × multiplo | Multiplo: —x | €… | —% | €… |
| **Valore finale proposto** | | | | | **€…** |

### 2.6 Struttura della relazione di stima

1. Incarico e quesiti
2. Documentazione esaminata
3. Descrizione dell'azienda e del settore
4. Analisi dei bilanci e normalizzazione
5. Indicatori economico-finanziari
6. Metodi di valutazione applicati (uno per uno)
7. Riconciliazione e range di valore
8. Conclusioni e valore proposto
9. Allegati (tabelle, bilanci riclassificati)

---

## 3. Area 2 — CTU tributaria {#3-area-2}

### 3.1 Ambito

Consulenza tecnica d'ufficio (e di parte) in procedimenti tributari: analisi di avvisi di accertamento, ricostruzione induttiva del reddito, verifica di accertamenti bancari, analisi di PVC della Guardia di Finanza, valutazione di cartelle esattoriali, perizie su transfer pricing.

### 3.2 Workflow principale

```
[1] Acquisizione atti
    · Avviso di accertamento / cartella esattoriale / PVC
    · Dichiarazioni fiscali degli anni accertati
    · Bilanci e scritture contabili
    · Estratti conto bancari (tutti i conti intestati e cointestati)
    · Documentazione a supporto delle deduzioni contestate
        │
        ▼
[2] Analisi dell'atto impositivo
    · Identificazione delle contestazioni per tipologia
    · Estrazione degli importi recuperati a tassazione
    · Identificazione delle norme applicate dall'ufficio
    · Verifica della corretta applicazione di aliquote e sanzioni
        │
        ▼
[3] Ricostruzione del reddito accertato
    · Metodo analitico: rettifica voce per voce
    · Metodo sintetico (redditometro / spesometro): verifica parametri
    · Metodo induttivo: analisi bancaria, studi di settore / ISA
        │
        ▼
[4] Analisi bancaria (se presente)
    · Mappatura tutti i movimenti contestati
    · Classificazione: prelevamenti / versamenti
    · Verifica presunzione art. 32 DPR 600/73
    · Identificazione giustificativi disponibili
        │
        ▼
[5] Costruzione delle controdeduzioni
    · Contestazione per contestazione
    · Riferimenti normativi e giurisprudenziali
    · Calcolo del reddito rideterminato
        │
        ▼
[6] Stesura relazione CTU / perizia di parte
```

### 3.3 Tabella contestazioni dell'ufficio

| N° | Rilievo | Norma applicata | Importo recuperato | Sanzione applicata | Tipo accertamento | Ref. atto |
|----|---------|-----------------|-------------------|--------------------|------------------|-----------|
| 1 | Ricavi non contabilizzati | Art. 39 c.1 DPR 600/73 | €… | 90%–180% | Analitico-induttivo | DOC-02 |
| 2 | Costo non inerente | Art. 109 TUIR | €… | 90%–180% | Analitico | DOC-02 |
| 3 | Prelevamenti c/c non giustificati | Art. 32 c.1 n.2 DPR 600/73 | €… | 90%–180% | Bancario | DOC-02 |
| … | … | … | €… | … | … | … |
| | **TOTALE RECUPERATO** | | **€…** | | | |

### 3.4 Tabella analisi bancaria

| Data | Banca / conto | Tipo movimento | Importo | Giustificativo disponibile | Classificazione | Importo contestabile | Ref. |
|------|--------------|----------------|---------|--------------------------|-----------------|---------------------|------|
| gg/mm/aaaa | Banca X — c/c …. | Versamento | €… | Sì — bonifico cliente Y | Ricavo contabilizzato | €0 | DOC-xx |
| gg/mm/aaaa | Banca X — c/c …. | Prelevamento | €… | No | Presunto ricavo (art. 32) | €… | DOC-xx |
| … | … | … | … | … | … | … | … |
| | | **Totale versamenti** | **€…** | | | | |
| | | **Totale prelevamenti** | **€…** | | | | |
| | | **Importo contestabile netto** | | | | **€…** | |

### 3.5 Tabella rideterminazione del reddito

| Rilievo | Importo ufficio | Controdeduzioni | Importo rideterminato | Norma / giurispr. a supporto |
|---------|----------------|-----------------|----------------------|------------------------------|
| Rilievo 1 | €… | Documentazione disponibile, inerenza dimostrata | €0 | Cass. n. …/…; art. … TUIR |
| Rilievo 2 | €… | Parzialmente fondato | €… | … |
| Rilievo 3 | €… | Infondato — presunzione vinta | €0 | Cass. SS.UU. n. 24823/2015 |
| **TOTALE** | **€…** | | **€…** | |
| **Risparmio fiscale ottenuto** | | | **€…** | |

### 3.6 Struttura della relazione CTU

1. Nomina e quesiti del giudice
2. Documentazione esaminata
3. Ricostruzione della vicenda fiscale
4. Analisi contestazione per contestazione
5. Analisi bancaria (se presente)
6. Rideterminazione del reddito imponibile
7. Calcolo delle imposte rideterminate
8. Calcolo delle sanzioni rideterminate
9. Risposta ai quesiti
10. Allegati

---

## 4. Area 3 — Crisi d'impresa e procedure concorsuali {#4-area-3}

### 4.1 Ambito

Relazioni ex art. 33 CCII (attestazione piani di risanamento), composizione negoziata della crisi (CNC), piani attestati di risanamento (PAR), concordato preventivo, liquidazione giudiziale, relazioni del commissario giudiziale e del curatore fallimentare.

### 4.2 Workflow principale

```
[1] Diagnosi della crisi
    · Analisi indicatori di allerta (DSCR, PFN/EBITDA, PN negativo)
    · Ricostruzione cause della crisi (esogene / endogene)
    · Verifica continuità aziendale (going concern)
    · Analisi dei flussi di cassa storici e prospettici
        │
        ▼
[2] Ricognizione dello stato passivo
    · Classificazione creditori per rango
    · Verifica titoli esecutivi e ipoteche
    · Stima del passivo latente (contenziosi, garanzie prestate)
        │
        ▼
[3] Analisi dell'attivo
    · Verifica valori di bilancio vs. valori di realizzo
    · Perizia beni immobili e impianti (se presenti)
    · Valutazione avviamento e intangibili
        │
        ▼
[4] Costruzione del piano
    · Piano industriale (ricavi, costi, margini previsionali)
    · Piano finanziario (cash flow previsionale a 3–5 anni)
    · Analisi sensitivity (scenari base / pessimista / ottimista)
        │
        ▼
[5] Attestazione (se incarico da attestatore)
    · Verifica veridicità dei dati aziendali
    · Verifica fattibilità del piano
    · Confronto con alternativa liquidatoria
        │
        ▼
[6] Produzione relazione ex art. 33 CCII / relazione commissariale
```

### 4.3 Tabella indicatori di crisi

| Indicatore | Formula | Anno-2 | Anno-1 | Anno 0 | Soglia allerta | Stato |
|------------|---------|--------|--------|--------|---------------|-------|
| DSCR (Debt Service Coverage Ratio) | FCO / Servizio del debito | — | — | — | > 1,0 | ⚠ / ✓ |
| PFN / EBITDA | PFN / EBITDA | — | — | — | < 4,0x | ⚠ / ✓ |
| Patrimonio netto | Da SP | €… | €… | €… | > 0 | ⚠ / ✓ |
| Indice di liquidità | Att. corr. / Pass. corr. | — | — | — | > 1,0 | ⚠ / ✓ |
| Capitale circolante netto | Att. corr. – Pass. corr. | €… | €… | €… | > 0 | ⚠ / ✓ |
| Posizione finanziaria netta | Debiti fin. – Liquidità | €… | €… | €… | — | — |

### 4.4 Tabella stato passivo — classificazione creditori

| N° | Creditore | Tipo credito | Importo capitale | Interessi / sanzioni | Rango | Garanzie | Titolo esecutivo | Ref. |
|----|-----------|-------------|-----------------|----------------------|-------|---------|-----------------|------|
| 1 | Banca X | Mutuo ipotecario | €… | €… | **Ipotecario** | Ipoteca I grado | Sì | DOC-xx |
| 2 | Erario — IRES | Tributario | €… | €… | **Privilegiato** | — | Cartella | DOC-xx |
| 3 | INPS | Previdenziale | €… | €… | **Privilegiato** | — | Cartella | DOC-xx |
| 4 | Fornitore Y Srl | Commerciale | €… | €… | **Chirografario** | — | No | DOC-xx |
| … | … | … | … | … | … | … | … | … |
| | **TOTALE PASSIVO** | | **€…** | **€…** | | | | |

### 4.5 Tabella confronto piano vs. alternativa liquidatoria

| Classe creditori | Passivo ammesso | Soddisfazione piano % | Soddisfazione liquidatoria % | Delta |
|-----------------|----------------|----------------------|------------------------------|-------|
| Creditori ipotecari | €… | —% | —% | — |
| Creditori privilegiati (fisco/INPS) | €… | —% | —% | — |
| Creditori chirografari | €… | —% | —% | — |
| **Totale** | **€…** | **—%** | **—%** | **—** |

### 4.6 Tabella cash flow previsionale (piano industriale)

| Voce | Anno 1 | Anno 2 | Anno 3 | Anno 4 | Anno 5 | Note |
|------|--------|--------|--------|--------|--------|------|
| Ricavi | €… | €… | €… | €… | €… | |
| Costi operativi | €… | €… | €… | €… | €… | |
| **EBITDA** | €… | €… | €… | €… | €… | |
| Variazione CCN | €… | €… | €… | €… | €… | |
| Capex | €… | €… | €… | €… | €… | |
| **Free Cash Flow operativo** | €… | €… | €… | €… | €… | |
| Rimborso debiti | €… | €… | €… | €… | €… | |
| **Flusso netto disponibile** | €… | €… | €… | €… | €… | |
| **DSCR** | — | — | — | — | — | > 1,0 |

### 4.7 Struttura della relazione ex art. 33 CCII

1. Nomina e incarico dell'attestatore
2. Documentazione esaminata
3. Descrizione dell'azienda e della crisi
4. Veridicità dei dati aziendali
5. Analisi dello stato passivo
6. Analisi dell'attivo
7. Il piano industriale e finanziario
8. Analisi della fattibilità
9. Confronto con l'alternativa liquidatoria
10. Attestazione
11. Allegati

---

## 5. Area 4 — Due diligence fiscale e societaria {#5-area-4}

### 5.1 Ambito

Analisi pre-acquisizione o pre-investimento di una società target: verifica della posizione fiscale, previdenziale, giuslavoristica e societaria. Identificazione di rischi latenti e passivi potenziali.

### 5.2 Workflow principale

```
[1] Acquisizione documentale
    · Ultime 5 dichiarazioni fiscali (Redditi, IVA, IRAP, 770)
    · Bilanci ultimi 5 esercizi con note integrative
    · Visura camerale, statuto, libro soci, verbali CdA
    · Contratti di lavoro, buste paga, situazione previdenziale
    · Contenziosi in corso (tributari, civili, giuslavoristici)
    · Contratti significativi (affitti, licenze, finanziamenti)
        │
        ▼
[2] Analisi per aree di rischio
    · Fiscale: verifica coerenza dichiarazioni/bilanci, IVA,
      ritenute, operazioni infragruppo, transfer pricing
    · Previdenziale: verifica versamenti INPS/INAIL, posizione
      ispettiva, collaboratori e parasubordinati
    · Giuslavoristico: regolarità contratti, TFR, straordinari,
      dirigenti, patti di non concorrenza
    · Societario: validità delibere, patti parasociali, opzioni,
      diritti di prelazione, clausole drag/tag along
        │
        ▼
[3] Identificazione e quantificazione rischi
    · Classificazione: certo / probabile / possibile / remoto
    · Stima dell'esposizione massima (imposte + sanzioni + interessi)
    · Verifica esistenza di fondi rischi in bilancio
        │
        ▼
[4] Produzione report con semaforo di rischio
```

### 5.3 Tabella checklist documenti richiesti

| Area | Documento | Anni richiesti | Ricevuto | Data ricezione | Mancante | Note |
|------|-----------|---------------|---------|---------------|---------|------|
| Fiscale | Dichiarazione Redditi (SC/SP/PF) | 2020–2024 | ✓/✗ | — | — | |
| Fiscale | Dichiarazione IVA annuale | 2020–2024 | ✓/✗ | — | — | |
| Fiscale | Modello 770 | 2020–2024 | ✓/✗ | — | — | |
| Fiscale | F24 versamenti (campione) | 2022–2024 | ✓/✗ | — | — | |
| Societario | Visura camerale aggiornata | Attuale | ✓/✗ | — | — | |
| Societario | Statuto vigente | Attuale | ✓/✗ | — | — | |
| Societario | Verbali CdA ultimi 3 anni | 2022–2024 | ✓/✗ | — | — | |
| Lavoristico | Prospetti paga (campione) | 2023–2024 | ✓/✗ | — | — | |
| Lavoristico | DURC | Attuale | ✓/✗ | — | — | |
| Contenzioso | Elenco liti pendenti | Attuale | ✓/✗ | — | — | |

### 5.4 Tabella rischi identificati — semaforo

| N° | Area | Descrizione rischio | Norma / base | Anni esposti | Esposizione max | Probabilità | Semaforo | Fondo in bilancio | Note |
|----|------|--------------------|-----------|-----------|----|-----------|---------|------------------|----|
| 1 | Fiscale | Deduzione costi intercompany non documentati | Art. 110 c.7 TUIR | 2022–2024 | €… | Probabile | 🟡 | No | Richiedere transfer pricing study |
| 2 | Previdenziale | Collaboratori riqualificabili come dipendenti | Art. 2 D.Lgs. 81/2015 | 2021–2024 | €… | Possibile | 🟡 | No | |
| 3 | Fiscale | IVA detratta su operazioni con soggetto a rischio frode | Art. 19 DPR 633/72 | 2023 | €… | Remoto | 🟢 | — | |
| 4 | Fiscale | Omessa presentazione dichiarazione IVA 2021 | Art. 5 D.Lgs. 471/97 | 2021 | €… | Certo | 🔴 | No | Ravvedimento consigliato |
| … | … | … | … | … | … | … | … | … | … |
| | | **Esposizione totale** | | | **€…** | | | | |

**Legenda semaforo:**

| Colore | Probabilità | Azione consigliata |
|--------|-------------|-------------------|
| 🔴 Rosso | Certo / Molto probabile | Accantonamento pieno, clausola indennizzo nel SPA |
| 🟡 Giallo | Probabile / Possibile | Accantonamento parziale o clausola escrow |
| 🟢 Verde | Possibile / Remoto | Monitoraggio, dichiarazione venditore |

### 5.5 Tabella esposizione fiscale per anno e tributo

| Anno | IRES | IRAP | IVA | Ritenute | INPS/INAIL | Totale imposte | Sanzioni min | Sanzioni max | Interessi | Esposizione totale |
|------|------|------|-----|---------|-----------|---------------|------------|------------|---------|-------------------|
| 2020 | €… | €… | €… | €… | €… | €… | €… | €… | €… | €… |
| 2021 | €… | €… | €… | €… | €… | €… | €… | €… | €… | €… |
| 2022 | €… | €… | €… | €… | €… | €… | €… | €… | €… | €… |
| 2023 | €… | €… | €… | €… | €… | €… | €… | €… | €… | €… |
| 2024 | €… | €… | €… | €… | €… | €… | €… | €… | €… | €… |
| **Totale** | | | | | | **€…** | **€…** | **€…** | **€…** | **€…** |

### 5.6 Struttura del report di due diligence

1. Incarico e perimetro dell'analisi
2. Documentazione esaminata e gap documentali
3. Sintesi esecutiva (executive summary con semaforo)
4. Analisi fiscale
5. Analisi previdenziale e giuslavoristica
6. Analisi societaria
7. Contenziosi in corso
8. Riepilogo rischi e quantificazione
9. Raccomandazioni (clausole SPA, escrow, indennizzi)
10. Allegati

---

## 6. Area 5 — Contenzioso tributario {#6-area-5}

### 6.1 Ambito

Assistenza nel contenzioso tributario: redazione di ricorsi, memorie illustrative, repliche, reclami/mediazione. Analisi di PVC, avvisi di accertamento, cartelle esattoriali, dinieghi di rimborso.

### 6.2 Workflow principale

```
[1] Analisi dell'atto impugnato
    · Tipo atto (accertamento, cartella, diniego rimborso, irrogazione sanzioni)
    · Ufficio emittente e competenza territoriale
    · Anno/periodo d'imposta
    · Termini di decadenza dell'accertamento
    · Termini per ricorso / reclamo-mediazione
        │
        ▼
[2] Estrazione dei rilievi contestati
    · Identificazione di ogni singola contestazione
    · Norma applicata dall'ufficio per ogni rilievo
    · Importo di ogni singola ripresa a tassazione
    · Tipo e misura delle sanzioni irrogate
        │
        ▼
[3] Verifica preliminare di ammissibilità
    · Rispetto dei termini (60 gg per ricorso; 30 gg per reclamo)
    · Competenza della Corte di Giustizia Tributaria
    · Valore della controversia (< €50.000: reclamo/mediazione obbligatorio)
    · Eventuale autotutela possibile
        │
        ▼
[4] Costruzione delle controdeduzioni
    · Per ogni rilievo: tesi difensiva, norme a supporto, giurisprudenza
    · Documenti a prova di ogni controdeduzione
    · Calcolo imposta/sanzione rideterminata
        │
        ▼
[5] Redazione atto (ricorso / memoria / replica)
    · Struttura conforme al D.Lgs. 546/92
    · Istanza di sospensione cautelare (se urgente)
        │
        ▼
[6] Gestione del fascicolo fino alla decisione
```

### 6.3 Tabella rilievi e controdeduzioni

| N° | Rilievo ufficio | Norma ufficio | Importo ripreso | Sanzione | Tesi difensiva | Norma difesa | Giurisprudenza | Doc. prova | Importo rideterminato | Ref. |
|----|----------------|-------------|----------------|---------|---------------|-------------|---------------|-----------|----------------------|------|
| 1 | Indeducibilità costo | Art. 109 TUIR | €… | 90% | Costo inerente e documentato | Art. 109 c.1 TUIR | Cass. n. …/… | Contratto, fattura | €0 | DOC-xx |
| 2 | Ricavi non dichiarati | Art. 39 DPR 600/73 | €… | 90%–180% | Presunzione superata da documenti | Art. 2697 c.c. | CTR … n. …/… | Estratti conto | €… | DOC-xx |
| … | … | … | … | … | … | … | … | … | … | … |
| | **TOTALE UFFICIO** | | **€…** | | | | | | **€… (rideterminato)** | |

### 6.4 Tabella scadenze processuali

| Atto | Data notifica / deposito | Termine scadenza | Giorni residui | Azione richiesta | Stato |
|------|-------------------------|-----------------|---------------|-----------------|-------|
| Avviso di accertamento | gg/mm/aaaa | gg/mm/aaaa | — | Ricorso / reclamo | In lavorazione |
| Reclamo-mediazione | gg/mm/aaaa | gg/mm/aaaa (90 gg) | — | Attesa risposta ufficio | — |
| Ricorso CGT I grado | gg/mm/aaaa | gg/mm/aaaa | — | Deposito + notifica | — |
| Memoria illustrativa | gg/mm/aaaa | gg/mm/aaaa (20 gg liberi udienza) | — | Redazione | — |
| Udienza CGT I grado | gg/mm/aaaa | — | — | Discussione | — |

### 6.5 Tabella calcolo risparmio atteso

| Scenario | Imponibile ripreso | Imposta | Sanzioni | Interessi | Totale pretesa | Risparmio vs. ufficio |
|----------|--------------------|---------|---------|---------|---------------|----------------------|
| Ufficio (base) | €… | €… | €… | €… | **€…** | — |
| Accoglimento totale | €0 | €0 | €0 | €0 | **€0** | **€…** |
| Accoglimento parziale | €… | €… | €… | €… | **€…** | **€…** |
| Definizione agevolata | €… | €… | Ridotte 1/3 | €… | **€…** | **€…** |

### 6.6 Struttura del ricorso tributario (D.Lgs. 546/92)

1. Intestazione: CGT competente, ricorrente, resistente
2. Esposizione del fatto
3. Motivi di ricorso (uno per ogni rilievo contestato)
4. Richieste al Collegio
5. Valore della controversia
6. Eventuale istanza cautelare (art. 47 D.Lgs. 546/92)
7. Sottoscrizione e procura alle liti
8. Allegati (atto impugnato + documenti probatori)

---

## 7. Area 6 — Dichiarazioni e adempimenti periodici {#7-area-6}

### 7.1 Ambito

Gestione strutturata del portafoglio clienti per adempimenti ricorrenti: dichiarazioni fiscali, versamenti, comunicazioni obbligatorie, adempimenti previdenziali. Scadenzario, checklist documenti, controllo qualità pre-invio.

### 7.2 Workflow principale

```
[1] Mappatura portafoglio clienti
    · Classificazione per tipo soggetto (persone fisiche, società di capitali,
      società di persone, enti non commerciali, professionisti)
    · Classificazione per regime fiscale (ordinario, semplificato,
      forfettario, startup, ETS)
    · Adempimenti applicabili per ogni cliente
        │
        ▼
[2] Generazione scadenzario annuale
    · Calendario adempimenti per ogni cliente
    · Priorità e assegnazione al collaboratore responsabile
    · Alert automatici a 30/15/7/1 giorni dalla scadenza
        │
        ▼
[3] Acquisizione documenti per ogni adempimento
    · Checklist personalizzata per tipo adempimento e cliente
    · Tracciamento documenti ricevuti / attesi / sollecitati
        │
        ▼
[4] Elaborazione e controllo
    · Preparazione bozza dichiarazione / versamento
    · Quadratura con dichiarazioni precedenti e contabilità
    · Controllo coerenza con ISA / indici di anomalia
        │
        ▼
[5] Quality check pre-invio
    · Verifica formale e sostanziale
    · Confronto anno precedente (variazioni > soglia da investigare)
        │
        ▼
[6] Invio / versamento e archiviazione
    · Invio telematico (Entratel / Fisconline)
    · Ricevuta di presentazione
    · Archiviazione fascicolo cliente
```

### 7.3 Scadenzario adempimenti principali

| Scadenza | Adempimento | Tributo / ente | Soggetti | Note |
|----------|-------------|---------------|---------|------|
| 16 gennaio | Versamento IVA (mensile dic.) | IVA | Mensili | — |
| 16 febbraio | Versamento IVA (mensile gen.) | IVA | Mensili | — |
| Febbraio (fine) | Certificazioni Uniche (CU) | IRPEF / INPS | Sostituti d'imposta | Invio a dipendenti/collaboratori |
| 31 marzo | Trasmissione CU | Ag. Entrate | Sostituti d'imposta | |
| 30 aprile | Dichiarazione IVA annuale | IVA | Tutti i soggetti IVA | |
| 31 maggio | 730 precompilato | IRPEF | Persone fisiche | |
| 30 giugno | Saldo IRPEF/IRES/IRAP anno prec. | IRPEF/IRES/IRAP | Tutti | Primo acconto se non rateizzato |
| 30 settembre | Mod. Redditi / IRAP (prorogato) | IRPEF/IRES/IRAP | Tutti | Verifica proroga annuale |
| 30 novembre | Secondo/unico acconto | IRPEF/IRES/IRAP | Tutti | Metodo storico o previsionale |
| 16 ogni mese | Versamento ritenute / contributi | IRPEF / INPS/INAIL | Sostituti | F24 |
| Trimestrale | Liquidazione IVA trimestrale | IVA | Trimestrali | 16/05, 16/08, 16/11, 16/03 |

### 7.4 Tabella portafoglio clienti — stato adempimenti

| Cliente | Tipo soggetto | Regime | Responsabile | Dichiaz. Redditi | IVA annuale | 770 | Acconti | DURC | Stato generale |
|---------|-------------|--------|-------------|-----------------|------------|-----|--------|------|---------------|
| Alfa Srl | Soc. capitali | Ordinario | Dott. X | ✓ Inviata | ✓ Inviata | ✓ | ✓ Versato | ✓ | 🟢 OK |
| Beta Snc | Soc. persone | Semplificato | Sig.ra Y | ⏳ In lavoraz. | ✓ Inviata | — | ⚠ Atteso | — | 🟡 Parziale |
| Gamma PF | Pers. fisica | Forfettario | Dott. X | ✗ Docs mancanti | — | — | ⏳ | — | 🔴 Blocco |
| … | … | … | … | … | … | … | … | … | … |

### 7.5 Checklist documenti per dichiarazione dei redditi (persone fisiche)

| Categoria | Documento | Obbligatorio | Ricevuto | Note |
|-----------|-----------|-------------|---------|------|
| Redditi lavoro | CU dipendente / pensione | Sì | ✓/✗ | |
| Redditi lavoro | CU collaborazione / autonomo | Se presente | ✓/✗ | |
| Redditi fondiari | Visura catastale / contratti affitto | Se presente | ✓/✗ | |
| Redditi capitale | Estratto conto titoli / dividendi | Se presente | ✓/✗ | |
| Oneri detraibili | Spese sanitarie (scontrini/ricevute) | Se presenti | ✓/✗ | Soglia €129,11 |
| Oneri detraibili | Interessi mutuo prima casa | Se presente | ✓/✗ | Max €4.000 |
| Oneri detraibili | Spese istruzione | Se presenti | ✓/✗ | |
| Oneri detraibili | Ristrutturazione (bonifico parlante) | Se presente | ✓/✗ | 50% su max €96.000 |
| Oneri detraibili | Ecobonus / Superbonus | Se presente | ✓/✗ | Verificare cessione credito |
| Oneri deducibili | Contributi previdenziali (artigiani/comm.) | Se presente | ✓/✗ | |
| Oneri deducibili | Contributi fondi pensione | Se presente | ✓/✗ | Max €5.164,57 |
| Immobili | Atti di acquisto/vendita nell'anno | Se presenti | ✓/✗ | Plusvalenza tassabile |

### 7.6 Tabella quadratura e controllo qualità pre-invio

| Check | Descrizione | Esito | Note |
|-------|-------------|-------|------|
| Coerenza reddito dichiarato vs. anno precedente | Variazione > 20%: motivare | ✓/⚠ | |
| Quadratura IVA: dichiarazione vs. liquidazioni periodiche | Differenza ammissibile < €5 | ✓/⚠ | |
| Coerenza ritenute subite vs. CU ricevute | Totale CU = totale in dichiarazione | ✓/⚠ | |
| Verifica ISA / voto di affidabilità | ISA ≥ 8: no accertamenti presuntivi | ✓/⚠ | |
| F24 acconti versati vs. debito da dichiarazione | Differenza giustificata | ✓/⚠ | |
| Presenza deleghe firmate dal cliente | Obbligatoria per invio | ✓/✗ | |
| Codice fiscale e dati anagrafici | Corrispondenza con CF ufficiale | ✓/⚠ | |

---

## 8. Struttura delle tabelle — Riferimento rapido {#8-tabelle}

| Tabella | Area | Colonne chiave | Scopo |
|---------|------|----------------|-------|
| Inventario documenti | Trasversale | N°, data, tipo, emittente, periodo, area, priorità, ref. | Classificazione e tracciabilità |
| Riclassificazione bilanci pluriennale | 1 | Voci CE/SP, anni, var. % | Analisi economico-finanziaria |
| Indicatori economico-finanziari | 1 | Indicatore, formula, anni, media, benchmark | Diagnosi aziendale |
| Metodi valutativi | 1 | Metodo, parametro, valore, peso, ponderato | Stima del valore |
| Contestazioni ufficio | 2 / 5 | Rilievo, norma, importo, sanzione, tipo | Analisi atto impositivo |
| Analisi bancaria | 2 | Data, conto, tipo, importo, giustificativo, contestabile | Art. 32 DPR 600/73 |
| Rideterminazione reddito | 2 / 5 | Rilievo, importo ufficio, controdeduzioni, rideterminato | Quantificazione risparmio |
| Indicatori di crisi | 3 | Indicatore, formula, anni, soglia, stato | Diagnosi crisi |
| Stato passivo creditori | 3 | Creditore, tipo, importo, rango, garanzie | Piano di risanamento |
| Cash flow previsionale | 3 | Voci FCF, anni 1–5, DSCR | Fattibilità piano |
| Checklist documenti DD | 4 | Area, documento, anni, ricevuto, mancante | Acquisizione documentale |
| Rischi — semaforo | 4 | Area, rischio, norma, esposizione, probabilità, semaforo | Risk assessment |
| Esposizione per anno e tributo | 4 | Anno, tributo per tipo, sanzioni, interessi, totale | Quantificazione rischio |
| Rilievi e controdeduzioni | 5 | Rilievo, norma, importo, tesi, giurispr., rideterminato | Strategia difensiva |
| Scadenze processuali | 5 | Atto, data, termine, giorni residui, azione | Gestione fascicolo |
| Portafoglio clienti — adempimenti | 6 | Cliente, regime, responsabile, stato per adempimento | Controllo avanzamento |
| Checklist documenti dichiarazione | 6 | Categoria, documento, obbligatorio, ricevuto | Raccolta dati |
| Quadratura pre-invio | 6 | Check, descrizione, esito | Quality control |

---

## 9. Riferimenti normativi e tabellari {#9-riferimenti}

### Valutazione d'azienda

| Riferimento | Ambito |
|-------------|--------|
| OIC 9, OIC 16, OIC 24 | Valutazione asset in bilancio italiano |
| IFRS 3, IAS 36 | Business combination e impairment test |
| Principi italiani di valutazione (PIV) — OIV 2015 e agg. | Standard valutazione professionale italiana |
| D.Lgs. 139/2015 | Bilancio OIC per società non quotate |

### Fiscalità delle imprese

| Riferimento | Ambito |
|-------------|--------|
| DPR 917/1986 (TUIR) | Testo Unico Imposte sui Redditi |
| DPR 633/1972 | IVA |
| D.Lgs. 446/1997 | IRAP |
| DPR 600/1973 | Accertamento imposte dirette |
| DPR 602/1973 | Riscossione |
| D.Lgs. 471/1997 | Sanzioni amministrative tributarie |
| D.Lgs. 472/1997 | Principi generali sanzioni tributarie |
| L. 212/2000 (Statuto del Contribuente) | Diritti del contribuente |

### Accertamento e contenzioso

| Riferimento | Ambito |
|-------------|--------|
| Art. 36-bis / 36-ter DPR 600/73 | Controllo formale e liquidazione automatica |
| Art. 37-bis DPR 600/73 (abrogato) / Art. 10-bis L. 212/2000 | Abuso del diritto |
| D.Lgs. 546/1992 | Processo tributario |
| D.Lgs. 218/1997 | Accertamento con adesione |
| D.Lgs. 19/2024 | Riforma del processo tributario |

### Crisi d'impresa

| Riferimento | Ambito |
|-------------|--------|
| D.Lgs. 14/2019 (CCII) e succ. modifiche | Codice della Crisi d'Impresa e dell'Insolvenza |
| Art. 33 CCII | Attestazione del professionista indipendente |
| Art. 56 CCII | Piano attestato di risanamento |
| Artt. 84–120 CCII | Concordato preventivo |
| D.Lgs. 136/2024 | Recepimento Direttiva Insolvency II |

### Adempimenti e dichiarazioni

| Riferimento | Ambito |
|-------------|--------|
| DPR 322/1998 | Termini e modalità dichiarazioni fiscali |
| D.Lgs. 241/1997 | Versamenti F24 e compensazione |
| L. 190/2014 e succ. | Regime forfettario |
| D.Lgs. 175/2014 | Semplificazioni fiscali |

---

## 10. Roadmap di implementazione {#10-roadmap}

### Fase 1 — Fondamenta trasversali (priorità alta)

| Task | Descrizione | Output |
|------|-------------|--------|
| T1.1 | Modulo classificazione documentale universale | Tabella inventario auto-popolata |
| T1.2 | Scadenzario adempimenti con alert | Calendario interattivo per area 6 |
| T1.3 | Tabella portafoglio clienti — stato adempimenti | Dashboard avanzamento |
| T1.4 | Quality check pre-invio dichiarazioni | Report controllo area 6 |

### Fase 2 — Aree a maggiore impatto (priorità alta)

| Task | Descrizione | Output |
|------|-------------|--------|
| T2.1 | Workflow perizia di stima con tabelle bilanci pluriennali | Relazione area 1 |
| T2.2 | Tabella rilievi/controdeduzioni con ricerca giurisprudenziale | Ricorso area 5 |
| T2.3 | Semaforo di rischio due diligence | Report area 4 |
| T2.4 | Stato passivo e confronto piano/liquidatoria | Relazione area 3 |

### Fase 3 — Automazione avanzata (priorità media — richiede artifact AI)

| Task | Descrizione | Output |
|------|-------------|--------|
| T3.1 | Upload bilanci PDF → estrazione automatica voci CE/SP | Tabelle riclassificate |
| T3.2 | Upload avviso accertamento → estrazione rilievi e importi | Tabella contestazioni |
| T3.3 | Calcolo automatico indicatori economico-finanziari | Tabella indicatori con benchmark |
| T3.4 | Ricerca automatica giurisprudenza tributaria per parola chiave | Suggerimenti citazioni |
| T3.5 | Generazione scadenzario personalizzato da profilo cliente | Scadenzario annuale cliente |

### Fase 4 — Integrazione avanzata (priorità bassa)

| Task | Descrizione | Output |
|------|-------------|--------|
| T4.1 | Connessione a banche dati giurisprudenziali (DeJure, Fiscoetasse) | Ricerca integrata |
| T4.2 | Integrazione con software gestionali (Zucchetti, TeamSystem, Wolters) | Import/export automatico |
| T4.3 | Generazione automatica F24 da piano versamenti | Bozza F24 precompilata |
| T4.4 | Dashboard KPI studio professionale | Produttività per area e collaboratore |

---

*Fine documento — versione 1.0*  
*Aggiornare a ogni modifica normativa rilevante: riforma CCII, aggiornamenti TUIR, nuovi termini processuali D.Lgs. 546/92, variazioni aliquote e scadenze dichiarative.*
