// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.
// One-shot helper: deep-merges new i18n keys into the locale files
// without clobbering existing translations. Safe to re-run.

import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const localesDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'locales')

const EN = {
  Common: { or: 'or' },
  Boot: {
    cannotReach: 'Cannot reach the backend',
    cannotReachHint:
      'Make sure the MikeRust backend is running. In dev, launch it from the repo root with',
    connecting: 'Connecting to MikeRust…',
  },
  Auth: {
    welcome: 'Welcome, {name}',
    welcomeBack: 'Welcome back, {name}',
    setupTitle: 'Welcome to MikeRust',
    setupSubtitle: 'Create your local profile. Everything stays on this machine.',
    setupPinNote:
      'Your PIN protects local access. There is no password recovery — keep it somewhere safe.',
    unlockTitle: 'Unlock MikeRust',
    unlockTitleNamed: 'Unlock, {name}',
    unlockSubtitle: 'Enter your PIN to continue.',
    username: 'Username',
    usernamePlaceholder: 'How the app addresses you',
    displayNameOptional: 'Display name (optional)',
    displayNamePlaceholder: 'Shown in the greeting',
    pin: 'PIN',
    pinPlaceholder: '{min}–{max} digits',
    pinEnter: 'Enter your PIN',
    confirmPin: 'Confirm PIN',
    confirmPinPlaceholder: 'Re-enter your PIN',
    pinFormat: 'PIN must be {min}–{max} digits',
    pinMismatch: 'PINs do not match',
    createProfile: 'Create profile',
    unlock: 'Unlock',
    useBiometric: 'Use biometric unlock',
    biometricReason: 'Unlock MikeRust with your biometric',
    lockout: 'Too many attempts — retry in {secs}s',
  },
  Ui: {
    comingSoonTitle: '{screen} — coming soon',
    comingSoonShort: 'Coming soon',
    comingSoonBody: 'This screen is built in a later phase of the UI rewrite.',
    preset: 'preset',
    hide: 'Hide',
    soon: 'soon',
    columnCount: '{n} col.',
    columnCountFull: '{n} columns',
    allLocales: 'All locales',
    clearFiltersHint: 'Try clearing the search or filters.',
    alsoDomain: 'also: {domain}',
    requiredFieldCount: '{n} required fields',
    createdOn: 'created {date}',
    deleteReview: 'Delete review',
  },
  Workflows: {
    restoredToast: '"{title}" restored',
    hiddenToast: '"{title}" hidden',
    updateError: 'Could not update workflow',
    columnsEmptyHint:
      'Add columns to define what this tabular review workflow extracts from each document.',
    columnPromptPlaceholder:
      'Write the analysis prompt — describe what should be extracted from each document for this column…',
  },
  ColumnFormats: {
    free_text: 'Free text',
    bulleted_list: 'Bulleted list',
    number: 'Number',
    percentage: 'Percentage',
    monetary_amount: 'Monetary amount',
    currency: 'Currency',
    yes_no: 'Yes / No',
    date: 'Date',
    tags: 'Tags',
  },
  Settings: {
    llmModels: 'LLM models',
    mcpServers: 'MCP servers',
    dataSources: 'Data sources',
    dangerZone: 'Danger zone',
  },
  TabularReviews: {
    subtitle: "Multi-document reviews driven by a tabular workflow's columns.",
    selectWorkflowOption: '— select a tabular workflow —',
    pickWorkflowError: 'Pick a tabular workflow.',
    createdToast: 'Review created',
    deletedToast: 'Review deleted',
    deleteError: 'Could not delete review',
    emptyHint: 'Create a review from a tabular workflow to get started.',
    inheritsColumns: 'Inherits {n} columns from this workflow.',
    scopedToDomain: 'Showing tabular workflows in the {domain} domain.',
    deleteConfirmTitle: 'Delete review?',
    deleteConfirmBody: '"{title}" will be permanently deleted.',
  },
}

const IT = {
  Common: { or: 'oppure' },
  Boot: {
    cannotReach: 'Impossibile raggiungere il backend',
    cannotReachHint:
      'Assicurati che il backend MikeRust sia in esecuzione. In sviluppo, avvialo dalla radice del repository con',
    connecting: 'Connessione a MikeRust…',
  },
  Auth: {
    welcome: 'Benvenuto, {name}',
    welcomeBack: 'Bentornato, {name}',
    setupTitle: 'Benvenuto in MikeRust',
    setupSubtitle: 'Crea il tuo profilo locale. Tutto resta su questo dispositivo.',
    setupPinNote:
      "Il PIN protegge l'accesso locale. Non esiste recupero password — conservalo in un luogo sicuro.",
    unlockTitle: 'Sblocca MikeRust',
    unlockTitleNamed: 'Sblocca, {name}',
    unlockSubtitle: 'Inserisci il PIN per continuare.',
    username: 'Nome utente',
    usernamePlaceholder: "Come l'app si rivolge a te",
    displayNameOptional: 'Nome visualizzato (facoltativo)',
    displayNamePlaceholder: 'Mostrato nel saluto',
    pin: 'PIN',
    pinPlaceholder: '{min}–{max} cifre',
    pinEnter: 'Inserisci il PIN',
    confirmPin: 'Conferma PIN',
    confirmPinPlaceholder: 'Reinserisci il PIN',
    pinFormat: 'Il PIN deve avere {min}–{max} cifre',
    pinMismatch: 'I PIN non coincidono',
    createProfile: 'Crea profilo',
    unlock: 'Sblocca',
    useBiometric: 'Usa lo sblocco biometrico',
    biometricReason: 'Sblocca MikeRust con il riconoscimento biometrico',
    lockout: 'Troppi tentativi — riprova tra {secs}s',
  },
  Ui: {
    comingSoonTitle: '{screen} — in arrivo',
    comingSoonShort: 'In arrivo',
    comingSoonBody:
      'Questa schermata verrà realizzata in una fase successiva della riscrittura UI.',
    preset: 'preimpostato',
    hide: 'Nascondi',
    soon: 'presto',
    columnCount: '{n} col.',
    columnCountFull: '{n} colonne',
    allLocales: 'Tutte le lingue',
    clearFiltersHint: 'Prova a rimuovere la ricerca o i filtri.',
    alsoDomain: 'anche: {domain}',
    requiredFieldCount: '{n} campi obbligatori',
    createdOn: 'creato il {date}',
    deleteReview: 'Elimina revisione',
  },
  Workflows: {
    restoredToast: '"{title}" ripristinato',
    hiddenToast: '"{title}" nascosto',
    updateError: 'Impossibile aggiornare il workflow',
    columnsEmptyHint:
      'Aggiungi colonne per definire cosa questo workflow di revisione tabellare estrae da ogni documento.',
    columnPromptPlaceholder:
      "Scrivi il prompt di analisi — descrivi cosa estrarre da ogni documento per questa colonna…",
  },
  ColumnFormats: {
    free_text: 'Testo libero',
    bulleted_list: 'Elenco puntato',
    number: 'Numero',
    percentage: 'Percentuale',
    monetary_amount: 'Importo monetario',
    currency: 'Valuta',
    yes_no: 'Sì / No',
    date: 'Data',
    tags: 'Tag',
  },
  Settings: {
    llmModels: 'Modelli LLM',
    mcpServers: 'Server MCP',
    dataSources: 'Fonti dati',
    dangerZone: 'Zona pericolosa',
  },
  TabularReviews: {
    subtitle:
      'Revisioni multi-documento guidate dalle colonne di un workflow tabellare.',
    selectWorkflowOption: '— seleziona un workflow tabellare —',
    pickWorkflowError: 'Seleziona un workflow tabellare.',
    createdToast: 'Revisione creata',
    deletedToast: 'Revisione eliminata',
    deleteError: 'Impossibile eliminare la revisione',
    emptyHint: 'Crea una revisione da un workflow tabellare per iniziare.',
    inheritsColumns: 'Eredita {n} colonne da questo workflow.',
    scopedToDomain: 'Workflow tabellari nel dominio {domain}.',
    deleteConfirmTitle: 'Eliminare la revisione?',
    deleteConfirmBody: '"{title}" verrà eliminata definitivamente.',
  },
}

/** Add only keys not already present, recursively. Returns count added. */
function mergeDefaults(target, additions) {
  let added = 0
  for (const [ns, keys] of Object.entries(additions)) {
    target[ns] ??= {}
    for (const [k, v] of Object.entries(keys)) {
      if (target[ns][k] === undefined) {
        target[ns][k] = v
        added++
      }
    }
  }
  return added
}

for (const [loc, additions] of [
  ['en', EN],
  ['it', IT],
]) {
  const file = join(localesDir, `${loc}.json`)
  const data = JSON.parse(readFileSync(file, 'utf-8'))
  const added = mergeDefaults(data, additions)
  writeFileSync(file, JSON.stringify(data, null, 2) + '\n', 'utf-8')
  console.log(`${loc}.json: +${added} keys`)
}
