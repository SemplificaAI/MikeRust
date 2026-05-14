# Piano strategico — Toolkit AI per la perizia medico-legale

> **Destinatari:** Medici legali, CTU/CTP, periti assicurativi, consulenti INAIL  
> **Versione:** 1.0 — Maggio 2026  
> **Scopo:** Descrivere in modo completo i workflow operativi e i processi di estrazione tabellare a supporto della perizia medico-legale assistita da intelligenza artificiale.

---

## Indice

1. [Visione generale e architettura del sistema](#1-visione-generale)
2. [Modulo 1 — Acquisizione e classificazione documentale](#2-modulo-1)
3. [Modulo 2 — Ricostruzione cronologica](#3-modulo-2)
4. [Modulo 3 — Estrazione diagnosi](#4-modulo-3)
5. [Modulo 4 — Postumi temporanei (ITT)](#5-modulo-4)
6. [Modulo 5 — Postumi permanenti (IP)](#6-modulo-5)
7. [Modulo 6 — Stesura della relazione medico-legale](#7-modulo-6)
8. [Modulo 7 — Quality check automatico](#8-modulo-7)
9. [Struttura delle tabelle — Riferimento rapido](#9-tabelle)
10. [Riferimenti normativi e tabellari](#10-riferimenti)
11. [Roadmap di implementazione](#11-roadmap)

---

## 1. Visione generale e architettura del sistema {#1-visione-generale}

### 1.1 Obiettivo

Costruire un sistema modulare assistito da AI che supporti il medico legale in **tutte le fasi della perizia**: dall'acquisizione documentale alla produzione della relazione finale, con estrazione automatica di dati clinici, cronologie, diagnosi strumentali e quantificazione del danno biologico secondo i riferimenti normativi vigenti.

Il sistema non sostituisce il giudizio clinico del perito: lo supporta eliminando il lavoro manuale ripetitivo (ordinamento, trascrizione, ricerca tabellare) e riducendo il rischio di omissioni documentali.

### 1.2 Architettura a moduli

```
┌─────────────────────────────────────────────────────────────┐
│                     INPUT DOCUMENTALE                        │
│  Cartelle cliniche · Referti · Certificati · Verbali PS     │
└──────────────────────────┬──────────────────────────────────┘
                           │
              ┌────────────▼────────────┐
              │  MODULO 1               │
              │  Classificazione docs   │
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │  MODULO 2               │
              │  Timeline cronologica   │
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │  MODULO 3               │
              │  Estrazione diagnosi    │
              │  (ingresso · strum. ·   │
              │   dimissione)           │
              └────────────┬────────────┘
                           │
               ┌───────────┴───────────┐
               │                       │
  ┌────────────▼──────┐   ┌────────────▼──────┐
  │  MODULO 4         │   │  MODULO 5         │
  │  Postumi temp.    │   │  Postumi perm.    │
  │  ITT              │   │  IP (RC/INAIL/    │
  └────────────┬──────┘   │  Inv. Civile)     │
               │          └────────────┬──────┘
               └───────────┬───────────┘
                           │
              ┌────────────▼────────────┐
              │  MODULO 6               │
              │  Stesura relazione ML   │
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │  MODULO 7               │
              │  Quality check          │
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │     OUTPUT FINALE       │
              │  Relazione ML · Tabelle │
              │  · Allegati             │
              └─────────────────────────┘
```

### 1.3 Principi operativi

| Principio | Descrizione |
|-----------|-------------|
| **Tracciabilità** | Ogni dato estratto è collegato al documento sorgente con numero di riferimento |
| **Revisione umana** | Il perito valida ogni modulo prima di procedere al successivo |
| **Neutralità valutativa** | L'AI presenta i dati, il medico legale esprime il giudizio |
| **Completezza documentale** | Il sistema segnala gap, incongruenze e documenti non valorizzati |
| **Aderenza normativa** | Ogni quantificazione fa riferimento esplicito alla tabella o norma applicata |

---

## 2. Modulo 1 — Acquisizione e classificazione documentale {#2-modulo-1}

### 2.1 Tipologie documentali gestite

#### Documenti ospedalieri
- **Cartella clinica di ricovero** (ordinario, day hospital, day surgery)
- **Verbale di pronto soccorso** con codice triage, accertamenti, terapia, diagnosi di uscita
- **Lettera di dimissione** con diagnosi principale, secondarie, terapia prescritta
- **Diario clinico** infermieristico e medico

#### Documenti strumentali
- Radiografie (RX) — colonna, torace, arti, cranio
- Tomografia computerizzata (TC) — cranio, colonna, torace, addome, arti
- Risonanza magnetica (RMN) — encefalo, colonna, articolazioni, tessuti molli
- Ecografia — addominale, muscolo-tendinea, vascolare
- Elettromiografia (EMG) e velocità di conduzione nervosa (VCN)
- Scintigrafia ossea, DEXA, spirometria, audiometria

#### Documenti ambulatoriali e specialistici
- Referti di visita specialistica (ortopedica, neurologica, fisiatrica, psichiatrica…)
- Certificati medici di malattia (INPS)
- Piani terapeutici e di riabilitazione
- Relazioni di fisioterapia con numero di sedute

#### Documentazione giuridico-assicurativa
- Denuncia di sinistro / denuncia infortunio (INAIL)
- Perizia assicurativa precedente
- Verbali di accertamento medico-legale INAIL, INPS, Commissione Invalidi Civili

### 2.2 Workflow di classificazione

```
DOCUMENTO IN INPUT
       │
       ▼
[1] Identificazione tipo
    (cartella / referto strum. / certificato / lettera dim. / verbale PS / altro)
       │
       ▼
[2] Estrazione metadati
    · Data documento
    · Data evento/esame
    · Struttura sanitaria
    · Medico responsabile / refertante
       │
       ▼
[3] Tagging distretto anatomico
    · Cranio-encefalo · Colonna cervicale · Colonna dorso-lombare
    · Torace · Addome · Bacino · AASS · AAII · Psiche · Altro
       │
       ▼
[4] Flag rilevanza medico-legale
    · DIRETTA — reperto compatibile con l'evento
    · INDIRETTA — compatibile con concausa preesistente
    · ESCLUSIVA — valore negativo/esclusivo del danno
    · DA VALUTARE — compatibilità incerta
       │
       ▼
[5] Inserimento in tabella inventario
```

### 2.3 Tabella inventario documenti (output Modulo 1)

| N° | Data doc. | Tipo | Struttura | Distretto | Sintesi reperto | Rilevanza ML | Ref. |
|----|-----------|------|-----------|-----------|-----------------|--------------|------|
| 1 | gg/mm/aaaa | Verbale PS | Osp. X — PS | Cranio / Colonna cerv. | Contusione cranio, distorsione cervicale | DIRETTA | DOC-01 |
| 2 | gg/mm/aaaa | RX colonna cerv. | Radiologia Osp. X | Colonna cervicale | Negativa per lesioni ossee acute | ESCLUSIVA | DOC-02 |
| 3 | gg/mm/aaaa | Certificato medico | MMG Dr. Rossi | Colonna cerv. | Cervicalgia post-traumatica, prescrizione riposo | DIRETTA | DOC-03 |
| … | … | … | … | … | … | … | … |

> **Nota operativa:** il campo `Ref.` assegna un codice univoco a ogni documento. Tutte le tabelle successive citano questo codice per garantire la tracciabilità.

---

## 3. Modulo 2 — Ricostruzione cronologica {#3-modulo-2}

### 3.1 Obiettivo

Costruire una **timeline clinica completa** dall'evento lesivo all'ultimo documento disponibile, evidenziando le tappe clinicamente e medico-legalmente rilevanti.

### 3.2 Workflow

```
[1] Identificazione evento lesivo
    · Data e ora (da verbale PS, denuncia sinistro, INAIL)
    · Dinamica dell'evento
    · Circostanze (stradale, lavoro, domestico, altro)
        │
        ▼
[2] Ordinamento cronologico documenti
    · Dalla tabella inventario Modulo 1
    · Raggruppamento per fase clinica
        │
        ▼
[3] Mappatura fasi cliniche
    · Fase acuta: PS → ricovero → dimissione
    · Fase sub-acuta: follow-up ambulatoriale, esami di controllo
    · Fase riabilitativa: fisioterapia, terapia occupazionale
    · Fase di stabilizzazione: ultimo referto evolutivo → MMI (Maximum Medical Improvement)
        │
        ▼
[4] Identificazione elementi critici
    · Preesistenze patologiche documentate
    · Concause (eventi intercorrenti, patologie degenerative)
    · Gap documentali (periodi senza copertura clinica)
    · Incongruenze temporali (referto precedente all'evento)
        │
        ▼
[5] Produzione tabella timeline
```

### 3.3 Tabella timeline cronologica (output Modulo 2)

| Data | Evento / Documento | Struttura | Diagnosi / Reperto | Terapia | Fase clinica | Note ML | Ref. |
|------|--------------------|-----------|-------------------|---------|--------------|---------|------|
| gg/mm/aaaa | **EVENTO LESIVO** | — | Dinamica: … | Soccorso 118 | — | Ora: hh:mm | — |
| gg/mm/aaaa | Accesso PS | Osp. X | Contusione / distorsione | Collare, FANS | **Acuta** | Codice triage: verde/giallo | DOC-01 |
| gg/mm/aaaa | RX colonna | Osp. X | Negativa | — | Acuta | Valore esclusivo | DOC-02 |
| gg/mm/aaaa | Ricovero | Osp. X | … | … | **Ricovero** | Dal … al … | DOC-xx |
| gg/mm/aaaa | Dimissione | Osp. X | … | … | Post-acuta | | DOC-xx |
| gg/mm/aaaa | Visita ortopedica | Amb. Y | … | Fisioterapia | **Riabilitativa** | | DOC-xx |
| gg/mm/aaaa | RMN colonna | Centro Z | … | … | Riabilitativa | | DOC-xx |
| gg/mm/aaaa | **STABILIZZAZIONE** | — | Postumi definiti | — | — | Data MMI | — |

---

## 4. Modulo 3 — Estrazione diagnosi {#4-modulo-3}

### 4.1 Diagnosi di ingresso

**Fonti:** verbale PS, scheda di accettazione, nota di ricovero.

**Dati estratti:**

| Campo | Descrizione |
|-------|-------------|
| Modalità di accesso | PS spontaneo / 118 / ricovero programmato / trasferimento |
| Codice triage | Bianco / Verde / Giallo / Rosso |
| Diagnosi di accettazione | Testo libero + codice ICD-10 se disponibile |
| Accertamenti eseguiti in PS | Elenco esami richiesti |
| Terapia immediata | Farmaci, immobilizzazione, suture, altro |
| Condizioni generali | GCS (se rilevante), parametri vitali se documentati |
| Diagnosi di uscita PS | Con eventuale raccomandazione di ricovero |

---

### 4.2 Diagnosi strumentali

Questa è la tabella di estrazione più critica per la costruzione del nesso causale.

#### Regole di classificazione del nesso

| Classificazione | Criteri |
|-----------------|---------|
| **DIRETTA** | Reperto positivo, distretto compatibile con l'evento, congruenza temporale (insorgenza entro finestra plausibile) |
| **INDIRETTA** | Reperto positivo con sovrapposizione a patologia preesistente documentata; richiede stima della quota di aggravamento |
| **ESCLUSIVA** | Reperto negativo o nella norma; valore di esclusione di lesioni strutturali |
| **DA VALUTARE** | Compatibilità temporale o anatomica dubbia; richiede approfondimento o consulenza specialistica |

#### Tabella diagnosi strumentali (output Modulo 3b)

| N° | Data | Tipo esame | Struttura | Refertante | Distretto | Reperto positivo | Reperto negativo | Nesso con evento | Rilevanza ML | Ref. |
|----|------|------------|-----------|------------|-----------|-----------------|-----------------|-----------------|--------------|------|
| 1 | gg/mm/aaaa | RX colonna cerv. | Rad. Osp. X | Dr. … | C. cervicale | — | Negativa per fratt. ossee | Esclusione lesioni ossee acute | ESCLUSIVA | DOC-02 |
| 2 | gg/mm/aaaa | RMN colonna cerv. | Centro Z | Dr. … | C. cervicale | Protrusione C5-C6, edema perilesionale | — | Compatibile con trauma distorsivo | DIRETTA | DOC-07 |
| 3 | gg/mm/aaaa | EMG AASS | Neurofisiol. | Dr. … | Nervo ulnare dx | Rallentamento VCN | — | Compatibile con sofferenza radicolare post-traumatica | DA VALUTARE | DOC-09 |

---

### 4.3 Diagnosi di dimissione

**Fonti:** lettera di dimissione ospedaliera, referto di fine ricovero.

| Campo | Descrizione |
|-------|-------------|
| Diagnosi principale ICD-10 | Codice e descrizione |
| Diagnosi secondarie | Elenco con codici ICD-10 |
| Interventi / procedure | Codice ICD-9-CM o ICD-10-PCS se disponibile |
| Terapia prescritta alla dimissione | Farmaci con posologia, presidi ortopedici |
| Follow-up raccomandato | Tipo visita, tempi indicati |
| Menomazione residua indicata | Se il clinico ha già segnalato esiti |
| Prognosi | Se indicata dal clinico dimissionante |

---

## 5. Modulo 4 — Valutazione dei postumi temporanei (ITT) {#5-modulo-4}

### 5.1 Definizioni operative

| Termine | Definizione medico-legale |
|---------|--------------------------|
| **ITT assoluta (100%)** | Periodo in cui il soggetto è totalmente inabile a qualsiasi attività, incluso il ricovero ospedaliero |
| **Inabilità parziale 75%** | Grave limitazione funzionale; il soggetto non può svolgere le attività ordinarie in modo autonomo |
| **Inabilità parziale 50%** | Limitazione moderata; può svolgere attività sedentarie o leggere con difficoltà |
| **Inabilità parziale 25%** | Limitazione lieve; svolgimento delle attività con disagio ma senza impedimento sostanziale |
| **Stabilizzazione / MMI** | Data in cui i postumi non sono più suscettibili di miglioramento apprezzabile con le cure |

### 5.2 Workflow valutazione ITT

```
[1] Identificazione periodo di ricovero
    Fonte: cartella clinica, lettera di dimissione
    → ITT assoluta per tutta la durata del ricovero
        │
        ▼
[2] Periodo post-ricovero con limitazione totale
    Fonte: certificati medici, indicazione del clinico dimissionante
    → ITT assoluta fino a deambulazione autonoma / ripresa parziale attività
        │
        ▼
[3] Fase di inabilità parziale decrescente
    Fonte: certificati del MMG, referti fisiatrici, relazioni fisioterapiche
    · Criterio: indicazione clinica documentata del grado di limitazione
    · In assenza di certificazione esplicita: stima medico-legale motivata
        │
        ▼
[4] Identificazione data di stabilizzazione
    · Ultimo referto evolutivo disponibile
    · Assenza di variazioni cliniche nei successivi 3–6 mesi
    · Eventuale dichiarazione esplicita dello specialista
        │
        ▼
[5] Calcolo complessivo giorni per fascia
    · Somma algebrica per ogni percentuale
    · Calcolo equivalente in giorni di ITT assoluta (se richiesto)
```

### 5.3 Tabella postumi temporanei (output Modulo 4)

| Fase | Dal | Al | N° giorni | % inabilità | Fonte documentale | Ref. |
|------|-----|----|-----------|-------------|-------------------|------|
| Ricovero ospedaliero | gg/mm/aaaa | gg/mm/aaaa | — | **100%** | Cartella clinica | DOC-xx |
| Post-ricovero — inabilità assoluta | gg/mm/aaaa | gg/mm/aaaa | — | **100%** | Certificati MMG | DOC-xx |
| Inabilità parziale | gg/mm/aaaa | gg/mm/aaaa | — | **75%** | Referto fisiatra | DOC-xx |
| Inabilità parziale | gg/mm/aaaa | gg/mm/aaaa | — | **50%** | Certificati MMG | DOC-xx |
| Inabilità parziale | gg/mm/aaaa | gg/mm/aaaa | — | **25%** | Referto fisiatra | DOC-xx |
| **TOTALE** | | | **—** | | | |
| *Equivalente ITT assoluta* | | | *—* | | | |

> **Formula equivalente ITT assoluta:**  
> `(gg×100% + gg×75% + gg×50% + gg×25%) / 100`

---

## 6. Modulo 5 — Valutazione dei postumi permanenti (IP) {#6-modulo-5}

### 6.1 Schema decisionale

```
POSTUMI PERMANENTI ACCERTATI
          │
          ▼
   Regime applicabile?
   ┌───────────────────────────────────┐
   │                                   │
   ▼                                   ▼
Responsabilità civile            INAIL / Invalidity civile
   │                                   │
   ▼                                   ▼
% IP stimata?             Tabelle specifiche INAIL / DM 1992
   │
   ├── ≤ 9% → Micropermanenti
   │          D.Lgs. 209/2005
   │
   └── > 10% → Macropermanenti
               SIMLA 2016 o SIMLA 2025
```

---

### 6.2 Percorso A — Responsabilità civile: micropermanenti (≤ 9%)

**Riferimento normativo:** Art. 139 D.Lgs. 209/2005 (Codice delle Assicurazioni Private) e successive tabelle ministeriali.

**Requisiti per l'applicazione:**
- Accertamento obiettivo del danno (obbligo introdotto dalla L. 27/2012)
- Presenza di riscontro strumentale o clinico-obiettivo documentato
- Esclusione di patologie preesistenti non aggravate dall'evento

**Workflow:**

```
[1] Verifica accertamento obiettivo
    · Esistono referti strumentali positivi? → SÌ: proseguire
    · Solo sintomatologia soggettiva? → documentare con cautela, motivare
        │
        ▼
[2] Identificazione voce tabellare
    · Distretto anatomico
    · Tipo di lesione (contusiva, distorsiva, fratturativa, neurologica)
    · Grado di limitazione funzionale residua
        │
        ▼
[3] Applicazione range tabellare
    · Percentuale minima / massima prevista dalla tabella
    · Scelta della % proposta con motivazione clinica
        │
        ▼
[4] Verifica preesistenze
    · Nessuna preesistenza: % tabellare piena
    · Preesistenza aggravata: stima quota di aggravamento
    · Preesistenza non aggravata: esclusione dal computo
```

---

### 6.3 Percorso B — Responsabilità civile: macropermanenti (> 10%)

**Riferimento:** Tabelle SIMLA 2016 e tabelle SIMLA 2025 (per le voci aggiornate).

> Le tabelle SIMLA 2025 hanno aggiornato alcune voci rispetto al 2016. In caso di difformità, il perito indica esplicitamente quale edizione applica e perché.

**Tabella postumi permanenti — RC (output Modulo 5a)**

| N° | Distretto / apparato | Descrizione postumo | Tabella applicata | % IP min | % IP max | % IP proposta | Motivazione clinica | Preesistenza | Ref. |
|----|---------------------|--------------------|--------------------|---------|---------|--------------|---------------------|--------------|------|
| 1 | Colonna cervicale | Protrusione C5-C6 con limitazione funzionale | SIMLA 2025 | 3% | 8% | 5% | Limitazione antiflessione 30°, dolore residuo cronico | No | DOC-07 |
| 2 | Arto superiore dx | Deficit neuromotorio nervo ulnare parziale | SIMLA 2016 | 5% | 12% | 7% | Riduzione forza prensione, EMG patologica | No | DOC-09 |
| — | — | — | — | — | — | — | — | — | — |
| | | **IP totale (formula combinata)** | | | | **—%** | | | |

> **Formula di Balthazard per postumi plurimi:**  
> `IP_tot = IP1 + IP2 × (1 - IP1) + IP3 × (1 - IP1 - IP2) + …`  
> Le tabelle SIMLA indicano criteri combinatori propri per alcune categorie.

---

### 6.4 Percorso C — Danno biologico INAIL

**Riferimento normativo:** D.Lgs. 38/2000 (art. 13), DM 12 luglio 2000 (Tabelle per la determinazione del grado di menomazione dell'integrità psicofisica).

**Specificità del sistema INAIL:**
- La menomazione dell'integrità psicofisica (MIP) è distinta dalla quota patrimoniale (capacità lavorativa specifica)
- Il danno biologico INAIL si quantifica in **punti percentuali di MIP** (0–100%)
- La rendita è liquidata solo per gradi ≥ 6% (o 5% per malattie professionali)
- Sotto la soglia: indennizzo in capitale (6–15%) o nulla (< 6%)

**Tabella danno biologico INAIL (output Modulo 5b)**

| N° | Distretto | Postumo | Voce tabellare INAIL | % MIP | Note |
|----|-----------|---------|----------------------|-------|------|
| 1 | — | — | Tab. A, voce n. … | —% | |
| 2 | — | — | Tab. B, voce n. … | —% | |
| | | **MIP totale** | | **—%** | |
| | | **Soglia applicabile** | | | Rendita / capitale / nulla |

---

### 6.5 Percorso D — Invalidità civile

**Riferimento normativo:** DM 5 febbraio 1992 — *Approvazione della nuova tabella indicativa delle percentuali d'invalidità per le minorazioni e malattie invalidanti.*

**Soglie previdenziali:**

| Soglia % | Beneficio |
|----------|-----------|
| ≥ 33% | Iscrizione al collocamento mirato (L. 68/1999) |
| ≥ 46% | Assegno mensile di assistenza (se inoccupato) |
| ≥ 67% | Pensione di invalidità civile |
| ≥ 74% | Indennità di accompagnamento (con requisiti) |
| 100% con non autosufficienza | Indennità di accompagnamento piena |

**Tabella invalidità civile (output Modulo 5c)**

| N° | Apparato | Postumo | Voce DM 1992 | % invalidità | Soglia raggiunta | Note |
|----|----------|---------|--------------|-------------|------------------|------|
| 1 | — | — | Sez. …, n. … | —% | — | |
| | | **Totale** | | **—%** | — | |

---

## 7. Modulo 6 — Stesura della relazione medico-legale {#7-modulo-6}

### 7.1 Struttura standard della relazione

#### Sezione 1 — Premessa
- Incarico ricevuto (CTU, CTP, consulenza stragiudiziale, INAIL)
- Quesiti peritali
- Elenco documentazione esaminata (con rimando alla tabella inventario Modulo 1)
- Data e luogo della visita (se eseguita)

#### Sezione 2 — Anamnesi (se visita diretta)
- Anamnesi fisiologica
- Anamnesi patologica remota (preesistenze rilevanti)
- Anamnesi patologica prossima: racconto dell'evento, evoluzione clinica, terapie eseguite
- Anamnesi lavorativa (rilevante per INAIL e invalidità civile)

#### Sezione 3 — Esame obiettivo (se visita diretta)
- Esame generale
- Esame del distretto/apparato leso
- Valutazione funzionale (ROMs, forza, sensibilità, trofismo)
- Strumentazione utilizzata (goniometro, dinamometro, ecc.)

#### Sezione 4 — Analisi documentale
- Sintesi narrativa della cronologia clinica (da Modulo 2)
- Rimando alla tabella timeline

#### Sezione 5 — Diagnosi strumentali
- Tabella estratta dal Modulo 3b
- Commento medico-legale sui referti di maggiore rilevanza

#### Sezione 6 — Nesso di causalità
Struttura argomentativa standard:

```
6.1  Criterio cronologico
     L'evento si è verificato il [data]; i primi accertamenti 
     clinici risalgono a [data], con intervallo compatibile.

6.2  Criterio topografico
     Le lesioni documentate interessano [distretto], anatomicamente
     compatibile con la dinamica dell'evento.

6.3  Criterio di continuità sintomatologica
     La documentazione clinica attesta una sintomatologia 
     continua/ininterrotta dal [data evento] al [data stabiliz.].

6.4  Criterio di esclusione di cause alternative
     Non sono documentate cause alternative o eventi intercorrenti
     idonei a determinare autonomamente le lesioni riscontrate.

6.5  Eventuale concausa preesistente
     È documentata una preesistenza di [patologia] che [non ha 
     contribuito / ha contribuito per una quota stimata di …%]
     alla determinazione del danno.

6.6  Conclusione sul nesso
     Sussiste / Non sussiste nesso di causalità (o concausalità)
     tra l'evento del [data] e le lesioni documentate.
```

#### Sezione 7 — Postumi temporanei
- Tabella ITT (da Modulo 4)
- Motivazione clinica per ogni fascia

#### Sezione 8 — Postumi permanenti
- Tabella IP con riferimento al regime applicato (RC / INAIL / Inv. Civile)
- Motivazione per ogni voce tabellare
- Gestione delle preesistenze

#### Sezione 9 — Conclusioni
Risposta puntuale a ogni quesito peritale, con:
- Linguaggio chiaro e non tecnico dove richiesto
- Rimandi ai paragrafi di dettaglio per le argomentazioni

#### Sezione 10 — Allegati
- Tabella inventario documenti (Modulo 1)
- Tabella timeline (Modulo 2)
- Tabella diagnosi strumentali (Modulo 3b)
- Tabella ITT (Modulo 4)
- Tabella IP (Modulo 5)

---

### 7.2 Workflow di stesura AI-assistita

```
PERITO: fornisce documenti e quesiti
          │
          ▼
AI: esegue Moduli 1 → 5
    produce tutte le tabelle
          │
          ▼
PERITO: valida tabelle e segnala correzioni
          │
          ▼
AI: genera bozza sezioni 1-10
          │
          ▼
PERITO: revisione sezione per sezione
        (modifica, integra, valida)
          │
          ▼
AI: quality check (Modulo 7)
    segnala eventuali incongruenze
          │
          ▼
PERITO: firma relazione finale
```

---

## 8. Modulo 7 — Quality check automatico {#8-modulo-7}

Prima della finalizzazione, il sistema esegue una verifica sistematica su tutti i moduli prodotti.

### 8.1 Checklist di verifica

| # | Check | Criterio | Esito atteso |
|---|-------|----------|--------------|
| 1 | **Copertura documentale** | Ogni documento della tabella inventario è citato almeno una volta nella relazione | Nessun documento orfano |
| 2 | **Coerenza temporale** | Nessun referto post-stabilizzazione è citato come indicatore di fase acuta | Zero incongruenze |
| 3 | **Supporto strumentale postumi** | Ogni postumo permanente inserito in tabella IP ha almeno un referto strumentale correlato | Ogni IP ha ≥ 1 DOC |
| 4 | **Range tabellare** | La % IP proposta rientra nel range min-max della tabella applicata | Nessuna % fuori range |
| 5 | **Preesistenze documentate** | Ogni preesistenza citata ha riferimento documentale (non solo anamnestica) | Riferimento presente |
| 6 | **Congruenza ITT/IP** | I giorni di ITT sono proporzionati alla gravità dei postumi permanenti riconosciuti | Nessuna disproporzione macroscopica |
| 7 | **Nesso causale esplicitato** | Ogni postumo (sia temp. che perm.) ha una sezione di nesso causale dedicata | Nessun postumo senza nesso |
| 8 | **Quesiti peritali** | Ogni quesito riceve risposta esplicita nelle conclusioni | Risposta 1:1 con i quesiti |
| 9 | **Formula combinatoria** | In caso di postumi plurimi, è applicata la formula di Balthazard o il metodo SIMLA | Calcolo verificabile |
| 10 | **Regime normativo univoco** | Non vengono applicati contemporaneamente criteri RC e INAIL allo stesso danno senza motivazione | Nessuna sovrapposizione |

### 8.2 Output del quality check

```
QUALITY CHECK — REPORT FINALE
================================
Data: gg/mm/aaaa
Perizia: [identificativo]

✓  Check 1 — Copertura documentale: SUPERATO (12/12 documenti citati)
✓  Check 2 — Coerenza temporale: SUPERATO
⚠  Check 3 — Supporto strumentale: ATTENZIONE
   → Il postumo "deficit sensitivo mano dx" (riga 3 tab. IP) non ha
     un EMG/VCN correlato nel set documentale. Verificare DOC-09.
✓  Check 4 — Range tabellare: SUPERATO
✓  Check 5 — Preesistenze: SUPERATO
✓  Check 6 — Congruenza ITT/IP: SUPERATO
✓  Check 7 — Nesso causale: SUPERATO
✓  Check 8 — Quesiti peritali: SUPERATO (3/3 quesiti con risposta)
✓  Check 9 — Formula combinatoria: SUPERATO
✓  Check 10 — Regime normativo: SUPERATO

TOTALE: 9/10 check superati — 1 avviso da risolvere prima della firma
```

---

## 9. Struttura delle tabelle — Riferimento rapido {#9-tabelle}

| Tabella | Modulo | Colonne chiave | Scopo |
|---------|--------|----------------|-------|
| **Inventario documenti** | 1 | N°, data, tipo, struttura, distretto, reperto, rilevanza ML, ref. | Classificazione e tracciabilità |
| **Timeline cronologica** | 2 | Data, evento, struttura, diagnosi, terapia, fase, note ML, ref. | Ricostruzione evolutiva |
| **Diagnosi di ingresso** | 3a | Modalità accesso, triage, diagnosi ICD-10, terapia immediata | Fase acuta |
| **Diagnosi strumentali** | 3b | Data, esame, struttura, distretto, reperto +/-, nesso, rilevanza ML | Supporto al nesso causale |
| **Diagnosi di dimissione** | 3c | ICD-10 principale/secondarie, terapia, follow-up, prognosi | Esiti del ricovero |
| **Postumi temporanei ITT** | 4 | Fase, dal, al, giorni, %, fonte, ref. | Quantificazione ITT |
| **Postumi permanenti RC** | 5a | Distretto, postumo, tabella, % min/max/proposta, motivazione | Danno biologico civile |
| **Danno biologico INAIL** | 5b | Distretto, postumo, voce tabellare INAIL, % MIP | Liquidazione INAIL |
| **Invalidità civile** | 5c | Apparato, postumo, voce DM 1992, %, soglia | Benefici previdenziali |

---

## 10. Riferimenti normativi e tabellari {#10-riferimenti}

### Responsabilità civile

| Riferimento | Ambito | Note |
|-------------|--------|-------|
| Art. 138 D.Lgs. 209/2005 | Macropermanenti RC — lesioni gravi | > 9% |
| Art. 139 D.Lgs. 209/2005 | Micropermanenti RC | ≤ 9% |
| L. 27/2012 (Decreto Liberalizzazioni) | Obbligo accertamento obiettivo | Per micropermanenti |
| **Tabelle SIMLA 2016** | Valutazione danni permanenti | Edizione precedente |
| **Tabelle SIMLA 2025** | Valutazione danni permanenti | Edizione aggiornata |
| Tabella micropermanenti ministeriale | Lesioni lievi da sinistro stradale | Aggiornata periodicamente |

### INAIL

| Riferimento | Ambito |
|-------------|--------|
| D.Lgs. 38/2000, art. 13 | Danno biologico da infortunio sul lavoro |
| DM 12 luglio 2000 | Tabelle MIP per infortuni sul lavoro |
| DM 9 aprile 2008 | Tabelle MIP per malattie professionali |

### Invalidità civile

| Riferimento | Ambito |
|-------------|--------|
| **DM 5 febbraio 1992** | Tabelle percentuali di invalidità civile |
| L. 118/1971 | Norme a favore dei mutilati e invalidi civili |
| L. 104/1992 | Assistenza, integrazione sociale e diritti persone handicappate |
| L. 68/1999 | Diritto al lavoro dei disabili |

---

## 11. Roadmap di implementazione {#11-roadmap}

### Fase 1 — Fondamenta (priorità alta)

| Task | Descrizione | Output |
|------|-------------|--------|
| 1.1 | Workflow acquisizione e classificazione documenti | Tabella inventario |
| 1.2 | Tabella diagnosi strumentali con flag ML | Tabella 3b |
| 1.3 | Tabella ITT con calcolo automatico giorni | Tabella Mod. 4 |
| 1.4 | Tabella IP — RC (micropermanenti e SIMLA) | Tabella 5a |

### Fase 2 — Espansione (priorità media)

| Task | Descrizione | Output |
|------|-------------|--------|
| 2.1 | Tabella IP — INAIL (MIP) | Tabella 5b |
| 2.2 | Tabella IP — Invalidità civile DM 1992 | Tabella 5c |
| 2.3 | Template relazione ML — sezione per sezione | Documento Word / PDF |
| 2.4 | Quality check automatico pre-firma | Report check |

### Fase 3 — Automazione avanzata (priorità alta — richiede artifact AI)

| Task | Descrizione | Output |
|------|-------------|--------|
| 3.1 | Artifact con upload PDF documenti → estrazione automatica metadati | Tabella inventario auto-popolata |
| 3.2 | Parsing automatico referti strumentali → tabella 3b | Tabella diagnosi auto-popolata |
| 3.3 | Calcolo automatico giorni ITT e formula Balthazard | Calcoli verificati |
| 3.4 | Ricerca tabellare SIMLA/INAIL/DM 1992 per parola chiave | Voce tabellare suggerita |

---

*Fine documento — versione 1.0*  
*Aggiornare al rilascio di nuove tabelle SIMLA o modifiche normative al D.Lgs. 209/2005 e DM 1992.*
