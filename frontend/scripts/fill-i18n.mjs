// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.
//
// i18n parity tool. en.json is the canonical key set; this script
// ensures it/fr/de/es/pt carry every key en has. en/it are already
// complete — the table below supplies fr/de/es/pt translations for the
// keys that were missing. Re-runnable: only fills absent keys, never
// overwrites. Fails loudly if en gains a key with no table entry.

import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const localesDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'locales')

// key -> { fr, de, es, pt }. Covers every key en/it have but the other
// four locales lacked (the 70 added during the Svelte rewrite plus ~31
// DocxTemplates/EmbeddingStatus keys the legacy bundle never localised).
const T = {
  'Common.or': { fr: 'ou', de: 'oder', es: 'o', pt: 'ou' },

  'Boot.cannotReach': { fr: 'Impossible de joindre le backend', de: 'Backend nicht erreichbar', es: 'No se puede conectar con el backend', pt: 'Não é possível contactar o backend' },
  'Boot.cannotReachHint': { fr: "Assurez-vous que le backend MikeRust est en cours d'exécution. En développement, lancez-le depuis la racine du dépôt avec", de: 'Stellen Sie sicher, dass das MikeRust-Backend läuft. In der Entwicklung starten Sie es im Repository-Stammverzeichnis mit', es: 'Asegúrate de que el backend de MikeRust esté en ejecución. En desarrollo, inícialo desde la raíz del repositorio con', pt: 'Certifique-se de que o backend do MikeRust está em execução. Em desenvolvimento, inicie-o a partir da raiz do repositório com' },
  'Boot.connecting': { fr: 'Connexion à MikeRust…', de: 'Verbindung mit MikeRust…', es: 'Conectando con MikeRust…', pt: 'A ligar ao MikeRust…' },

  'Auth.welcome': { fr: 'Bienvenue, {name}', de: 'Willkommen, {name}', es: 'Bienvenido, {name}', pt: 'Bem-vindo, {name}' },
  'Auth.welcomeBack': { fr: 'Bon retour, {name}', de: 'Willkommen zurück, {name}', es: 'Bienvenido de nuevo, {name}', pt: 'Bem-vindo de volta, {name}' },
  'Auth.setupTitle': { fr: 'Bienvenue dans MikeRust', de: 'Willkommen bei MikeRust', es: 'Te damos la bienvenida a MikeRust', pt: 'Bem-vindo ao MikeRust' },
  'Auth.setupSubtitle': { fr: 'Créez votre profil local. Tout reste sur cette machine.', de: 'Erstellen Sie Ihr lokales Profil. Alles bleibt auf diesem Gerät.', es: 'Crea tu perfil local. Todo permanece en este equipo.', pt: 'Crie o seu perfil local. Tudo permanece neste computador.' },
  'Auth.setupPinNote': { fr: "Votre PIN protège l'accès local. Il n'existe aucune récupération de mot de passe — conservez-le en lieu sûr.", de: 'Ihr PIN schützt den lokalen Zugriff. Es gibt keine Passwortwiederherstellung — bewahren Sie ihn sicher auf.', es: 'Tu PIN protege el acceso local. No hay recuperación de contraseña — guárdalo en un lugar seguro.', pt: 'O seu PIN protege o acesso local. Não existe recuperação de palavra-passe — guarde-o em local seguro.' },
  'Auth.unlockTitle': { fr: 'Déverrouiller MikeRust', de: 'MikeRust entsperren', es: 'Desbloquear MikeRust', pt: 'Desbloquear o MikeRust' },
  'Auth.unlockTitleNamed': { fr: 'Déverrouiller, {name}', de: 'Entsperren, {name}', es: 'Desbloquear, {name}', pt: 'Desbloquear, {name}' },
  'Auth.unlockSubtitle': { fr: 'Saisissez votre PIN pour continuer.', de: 'Geben Sie Ihren PIN ein, um fortzufahren.', es: 'Introduce tu PIN para continuar.', pt: 'Introduza o seu PIN para continuar.' },
  'Auth.username': { fr: "Nom d'utilisateur", de: 'Benutzername', es: 'Nombre de usuario', pt: 'Nome de utilizador' },
  'Auth.usernamePlaceholder': { fr: "Comment l'application s'adresse à vous", de: 'Wie die App Sie anspricht', es: 'Cómo se dirige la aplicación a ti', pt: 'Como a aplicação se dirige a si' },
  'Auth.displayNameOptional': { fr: 'Nom affiché (facultatif)', de: 'Anzeigename (optional)', es: 'Nombre visible (opcional)', pt: 'Nome a apresentar (opcional)' },
  'Auth.displayNamePlaceholder': { fr: "Affiché dans le message d'accueil", de: 'Wird in der Begrüßung angezeigt', es: 'Se muestra en el saludo', pt: 'Apresentado na saudação' },
  'Auth.pin': { fr: 'PIN', de: 'PIN', es: 'PIN', pt: 'PIN' },
  'Auth.pinPlaceholder': { fr: '{min}–{max} chiffres', de: '{min}–{max} Ziffern', es: '{min}–{max} dígitos', pt: '{min}–{max} dígitos' },
  'Auth.pinEnter': { fr: 'Saisissez votre PIN', de: 'PIN eingeben', es: 'Introduce tu PIN', pt: 'Introduza o seu PIN' },
  'Auth.confirmPin': { fr: 'Confirmer le PIN', de: 'PIN bestätigen', es: 'Confirmar PIN', pt: 'Confirmar PIN' },
  'Auth.confirmPinPlaceholder': { fr: 'Saisissez à nouveau votre PIN', de: 'PIN erneut eingeben', es: 'Vuelve a introducir tu PIN', pt: 'Reintroduza o seu PIN' },
  'Auth.pinFormat': { fr: 'Le PIN doit comporter de {min} à {max} chiffres', de: 'Der PIN muss {min}–{max} Ziffern haben', es: 'El PIN debe tener entre {min} y {max} dígitos', pt: 'O PIN deve ter {min}–{max} dígitos' },
  'Auth.pinMismatch': { fr: 'Les PIN ne correspondent pas', de: 'PINs stimmen nicht überein', es: 'Los PIN no coinciden', pt: 'Os PIN não coincidem' },
  'Auth.createProfile': { fr: 'Créer le profil', de: 'Profil erstellen', es: 'Crear perfil', pt: 'Criar perfil' },
  'Auth.unlock': { fr: 'Déverrouiller', de: 'Entsperren', es: 'Desbloquear', pt: 'Desbloquear' },
  'Auth.useBiometric': { fr: 'Utiliser le déverrouillage biométrique', de: 'Biometrische Entsperrung verwenden', es: 'Usar desbloqueo biométrico', pt: 'Usar desbloqueio biométrico' },
  'Auth.biometricReason': { fr: 'Déverrouillez MikeRust avec votre identification biométrique', de: 'MikeRust mit Ihrer biometrischen Authentifizierung entsperren', es: 'Desbloquea MikeRust con tu identificación biométrica', pt: 'Desbloqueie o MikeRust com a sua biometria' },
  'Auth.lockout': { fr: 'Trop de tentatives — réessayez dans {secs}s', de: 'Zu viele Versuche — erneut in {secs}s', es: 'Demasiados intentos — reinténtalo en {secs}s', pt: 'Demasiadas tentativas — tente novamente em {secs}s' },

  'Ui.comingSoonTitle': { fr: '{screen} — bientôt disponible', de: '{screen} — demnächst verfügbar', es: '{screen} — próximamente', pt: '{screen} — brevemente' },
  'Ui.comingSoonShort': { fr: 'Bientôt disponible', de: 'Demnächst verfügbar', es: 'Próximamente', pt: 'Brevemente' },
  'Ui.comingSoonBody': { fr: "Cet écran sera réalisé dans une phase ultérieure de la refonte de l'interface.", de: 'Dieser Bildschirm wird in einer späteren Phase der UI-Neugestaltung umgesetzt.', es: 'Esta pantalla se desarrollará en una fase posterior de la reescritura de la interfaz.', pt: 'Este ecrã será criado numa fase posterior da reescrita da interface.' },
  'Ui.preset': { fr: 'prédéfini', de: 'Vorlage', es: 'predefinido', pt: 'predefinido' },
  'Ui.hide': { fr: 'Masquer', de: 'Ausblenden', es: 'Ocultar', pt: 'Ocultar' },
  'Ui.soon': { fr: 'bientôt', de: 'bald', es: 'pronto', pt: 'brevemente' },
  'Ui.columnCount': { fr: '{n} col.', de: '{n} Sp.', es: '{n} col.', pt: '{n} col.' },
  'Ui.columnCountFull': { fr: '{n} colonnes', de: '{n} Spalten', es: '{n} columnas', pt: '{n} colunas' },
  'Ui.allLocales': { fr: 'Toutes les langues', de: 'Alle Sprachen', es: 'Todos los idiomas', pt: 'Todos os idiomas' },
  'Ui.clearFiltersHint': { fr: "Essayez d'effacer la recherche ou les filtres.", de: 'Versuchen Sie, die Suche oder Filter zurückzusetzen.', es: 'Prueba a borrar la búsqueda o los filtros.', pt: 'Experimente limpar a pesquisa ou os filtros.' },
  'Ui.alsoDomain': { fr: 'aussi : {domain}', de: 'auch: {domain}', es: 'también: {domain}', pt: 'também: {domain}' },
  'Ui.requiredFieldCount': { fr: '{n} champs requis', de: '{n} Pflichtfelder', es: '{n} campos obligatorios', pt: '{n} campos obrigatórios' },
  'Ui.createdOn': { fr: 'créé le {date}', de: 'erstellt am {date}', es: 'creado el {date}', pt: 'criado em {date}' },
  'Ui.deleteReview': { fr: 'Supprimer la revue', de: 'Prüfung löschen', es: 'Eliminar revisión', pt: 'Eliminar revisão' },

  'Settings.llmModels': { fr: 'Modèles LLM', de: 'LLM-Modelle', es: 'Modelos LLM', pt: 'Modelos LLM' },
  'Settings.mcpServers': { fr: 'Serveurs MCP', de: 'MCP-Server', es: 'Servidores MCP', pt: 'Servidores MCP' },
  'Settings.dataSources': { fr: 'Sources de données', de: 'Datenquellen', es: 'Fuentes de datos', pt: 'Fontes de dados' },
  'Settings.dangerZone': { fr: 'Zone sensible', de: 'Gefahrenzone', es: 'Zona de peligro', pt: 'Zona de perigo' },

  'Workflows.restoredToast': { fr: '« {title} » restauré', de: '„{title}“ wiederhergestellt', es: '«{title}» restaurado', pt: '“{title}” restaurado' },
  'Workflows.hiddenToast': { fr: '« {title} » masqué', de: '„{title}“ ausgeblendet', es: '«{title}» ocultado', pt: '“{title}” ocultado' },
  'Workflows.updateError': { fr: 'Impossible de mettre à jour le workflow', de: 'Workflow konnte nicht aktualisiert werden', es: 'No se pudo actualizar el flujo de trabajo', pt: 'Não foi possível atualizar o fluxo de trabalho' },
  'Workflows.columnsEmptyHint': { fr: 'Ajoutez des colonnes pour définir ce que ce workflow de revue tabulaire extrait de chaque document.', de: 'Fügen Sie Spalten hinzu, um festzulegen, was dieser tabellarische Prüf-Workflow aus jedem Dokument extrahiert.', es: 'Añade columnas para definir qué extrae este flujo de revisión tabular de cada documento.', pt: 'Adicione colunas para definir o que este fluxo de revisão tabular extrai de cada documento.' },
  'Workflows.columnPromptPlaceholder': { fr: "Rédigez le prompt d'analyse — décrivez ce qui doit être extrait de chaque document pour cette colonne…", de: 'Schreiben Sie den Analyse-Prompt — beschreiben Sie, was aus jedem Dokument für diese Spalte extrahiert werden soll…', es: 'Escribe el prompt de análisis — describe qué debe extraerse de cada documento para esta columna…', pt: 'Escreva o prompt de análise — descreva o que deve ser extraído de cada documento para esta coluna…' },

  'ColumnFormats.free_text': { fr: 'Texte libre', de: 'Freitext', es: 'Texto libre', pt: 'Texto livre' },
  'ColumnFormats.bulleted_list': { fr: 'Liste à puces', de: 'Aufzählungsliste', es: 'Lista con viñetas', pt: 'Lista com marcadores' },
  'ColumnFormats.number': { fr: 'Nombre', de: 'Zahl', es: 'Número', pt: 'Número' },
  'ColumnFormats.percentage': { fr: 'Pourcentage', de: 'Prozentsatz', es: 'Porcentaje', pt: 'Percentagem' },
  'ColumnFormats.monetary_amount': { fr: 'Montant monétaire', de: 'Geldbetrag', es: 'Importe monetario', pt: 'Montante monetário' },
  'ColumnFormats.currency': { fr: 'Devise', de: 'Währung', es: 'Moneda', pt: 'Moeda' },
  'ColumnFormats.yes_no': { fr: 'Oui / Non', de: 'Ja / Nein', es: 'Sí / No', pt: 'Sim / Não' },
  'ColumnFormats.date': { fr: 'Date', de: 'Datum', es: 'Fecha', pt: 'Data' },
  'ColumnFormats.tags': { fr: 'Étiquettes', de: 'Tags', es: 'Etiquetas', pt: 'Etiquetas' },

  'TabularReviews.subtitle': { fr: "Revues multi-documents pilotées par les colonnes d'un workflow tabulaire.", de: 'Mehrdokument-Prüfungen, gesteuert durch die Spalten eines tabellarischen Workflows.', es: 'Revisiones multidocumento guiadas por las columnas de un flujo de trabajo tabular.', pt: 'Revisões multidocumento orientadas pelas colunas de um fluxo de trabalho tabular.' },
  'TabularReviews.selectWorkflowOption': { fr: '— sélectionnez un workflow tabulaire —', de: '— tabellarischen Workflow auswählen —', es: '— selecciona un flujo de trabajo tabular —', pt: '— selecione um fluxo de trabalho tabular —' },
  'TabularReviews.pickWorkflowError': { fr: 'Choisissez un workflow tabulaire.', de: 'Wählen Sie einen tabellarischen Workflow.', es: 'Elige un flujo de trabajo tabular.', pt: 'Escolha um fluxo de trabalho tabular.' },
  'TabularReviews.createdToast': { fr: 'Revue créée', de: 'Prüfung erstellt', es: 'Revisión creada', pt: 'Revisão criada' },
  'TabularReviews.deletedToast': { fr: 'Revue supprimée', de: 'Prüfung gelöscht', es: 'Revisión eliminada', pt: 'Revisão eliminada' },
  'TabularReviews.deleteError': { fr: 'Impossible de supprimer la revue', de: 'Prüfung konnte nicht gelöscht werden', es: 'No se pudo eliminar la revisión', pt: 'Não foi possível eliminar a revisão' },
  'TabularReviews.emptyHint': { fr: 'Créez une revue à partir d’un workflow tabulaire pour commencer.', de: 'Erstellen Sie eine Prüfung aus einem tabellarischen Workflow, um zu beginnen.', es: 'Crea una revisión a partir de un flujo de trabajo tabular para empezar.', pt: 'Crie uma revisão a partir de um fluxo de trabalho tabular para começar.' },
  'TabularReviews.inheritsColumns': { fr: 'Hérite de {n} colonnes de ce workflow.', de: 'Übernimmt {n} Spalten aus diesem Workflow.', es: 'Hereda {n} columnas de este flujo de trabajo.', pt: 'Herda {n} colunas deste fluxo de trabalho.' },
  'TabularReviews.scopedToDomain': { fr: 'Affichage des workflows tabulaires du domaine {domain}.', de: 'Tabellarische Workflows der Domäne {domain}.', es: 'Mostrando flujos de trabajo tabulares del dominio {domain}.', pt: 'A mostrar fluxos de trabalho tabulares do domínio {domain}.' },
  'TabularReviews.deleteConfirmTitle': { fr: 'Supprimer la revue ?', de: 'Prüfung löschen?', es: '¿Eliminar la revisión?', pt: 'Eliminar a revisão?' },
  'TabularReviews.deleteConfirmBody': { fr: '« {title} » sera supprimée définitivement.', de: '„{title}“ wird endgültig gelöscht.', es: '«{title}» se eliminará de forma permanente.', pt: '“{title}” será eliminada definitivamente.' },

  'DocxTemplates.title': { fr: 'Modèles DOCX', de: 'DOCX-Vorlagen', es: 'Plantillas DOCX', pt: 'Modelos DOCX' },
  'DocxTemplates.subtitle': { fr: 'Modèles de documents professionnels italiens. Sélectionnez-en un depuis le compositeur de chat pour produire un .docx préformaté.', de: 'Professionelle italienische Dokumentvorlagen. Wählen Sie eine im Chat-Editor aus, um eine vorformatierte .docx zu erstellen.', es: 'Plantillas profesionales de documentos italianos. Selecciona una en el redactor de chat para generar un .docx preformateado.', pt: 'Modelos profissionais de documentos italianos. Selecione um no compositor de chat para produzir um .docx pré-formatado.' },
  'DocxTemplates.noTemplates': { fr: 'Aucun modèle disponible', de: 'Keine Vorlagen verfügbar', es: 'No hay plantillas disponibles', pt: 'Nenhum modelo disponível' },
  'DocxTemplates.searchPlaceholder': { fr: 'Rechercher des modèles…', de: 'Vorlagen suchen…', es: 'Buscar plantillas…', pt: 'Procurar modelos…' },
  'DocxTemplates.alsoApplicableTo': { fr: 'Également applicable à', de: 'Auch anwendbar auf', es: 'También aplicable a', pt: 'Também aplicável a' },
  'DocxTemplates.automationLevel': { fr: "Niveau d'automatisation", de: 'Automatisierungsgrad', es: 'Nivel de automatización', pt: 'Nível de automatização' },
  'DocxTemplates.automationL1': { fr: 'L1 — Publipostage simple', de: 'L1 — Einfacher Seriendruck', es: 'L1 — Combinación de correspondencia simple', pt: 'L1 — Impressão em série simples' },
  'DocxTemplates.automationL2': { fr: 'L2 — Branches conditionnelles', de: 'L2 — Bedingte Verzweigungen', es: 'L2 — Ramas condicionales', pt: 'L2 — Ramificações condicionais' },
  'DocxTemplates.automationL3': { fr: 'L3 — Blocs répétés', de: 'L3 — Wiederholte Blöcke', es: 'L3 — Bloques repetidos', pt: 'L3 — Blocos repetidos' },
  'DocxTemplates.automationL4': { fr: 'L4 — Intégration backend', de: 'L4 — Backend-Integration', es: 'L4 — Integración con backend', pt: 'L4 — Integração com backend' },
  'DocxTemplates.requiredMetadata': { fr: 'Champs requis', de: 'Pflichtfelder', es: 'Campos obligatorios', pt: 'Campos obrigatórios' },
  'DocxTemplates.sectionSkeleton': { fr: 'Structure du document', de: 'Dokumentstruktur', es: 'Estructura del documento', pt: 'Estrutura do documento' },
  'DocxTemplates.promptPreview': { fr: 'Contrat de rédaction', de: 'Erstellungsvertrag', es: 'Contrato de redacción', pt: 'Contrato de redação' },
  'DocxTemplates.previewPrompt': { fr: 'Aperçu du prompt', de: 'Prompt-Vorschau', es: 'Vista previa del prompt', pt: 'Pré-visualização do prompt' },
  'DocxTemplates.openInChat': { fr: 'Utiliser dans le chat', de: 'Im Chat verwenden', es: 'Usar en el chat', pt: 'Usar no chat' },
  'DocxTemplates.applyToChat': { fr: 'Appliquer au chat', de: 'Auf Chat anwenden', es: 'Aplicar al chat', pt: 'Aplicar ao chat' },
  'DocxTemplates.sourceReference': { fr: 'Spécification de référence', de: 'Maßgebliche Spezifikation', es: 'Especificación de referencia', pt: 'Especificação de referência' },
  'DocxTemplates.paper': { fr: 'Format de papier', de: 'Papierformat', es: 'Tamaño de papel', pt: 'Tamanho do papel' },
  'DocxTemplates.typography': { fr: 'Typographie', de: 'Typografie', es: 'Tipografía', pt: 'Tipografia' },
  'DocxTemplates.margins': { fr: 'Marges', de: 'Ränder', es: 'Márgenes', pt: 'Margens' },
  'DocxTemplates.templateActive': { fr: 'Modèle actif', de: 'Aktive Vorlage', es: 'Plantilla activa', pt: 'Modelo ativo' },
  'DocxTemplates.remove': { fr: 'Retirer le modèle', de: 'Vorlage entfernen', es: 'Quitar plantilla', pt: 'Remover modelo' },
  'DocxTemplates.system': { fr: 'Système', de: 'System', es: 'Sistema', pt: 'Sistema' },
  'DocxTemplates.pickTemplate': { fr: 'Ouvrir le modèle', de: 'Vorlage öffnen', es: 'Abrir plantilla', pt: 'Abrir modelo' },
  'DocxTemplates.pickTemplateTitle': { fr: 'Choisir un modèle pour le document', de: 'Vorlage für das Dokument auswählen', es: 'Elige una plantilla para el documento', pt: 'Escolha um modelo para o documento' },
  'DocxTemplates.repeatingBlock': { fr: 'Bloc répété', de: 'Wiederholter Block', es: 'Bloque repetido', pt: 'Bloco repetido' },

  'EmbeddingStatus.loadingModelTitle': { fr: "Chargement du modèle d'embedding…", de: 'Embedding-Modell wird geladen…', es: 'Cargando el modelo de embeddings…', pt: 'A carregar o modelo de embeddings…' },
  'EmbeddingStatus.loadingModelDetail': { fr: 'Initialisation de la session ONNX multilingual-e5-base (~265 Mo quantifiée INT8). Uniquement à la première requête après le lancement — les suivantes sont instantanées.', de: 'Initialisierung der multilingual-e5-base-ONNX-Sitzung (~265 MB INT8-quantisiert). Nur bei der ersten Anfrage nach dem Start — alle weiteren sind sofort.', es: 'Inicializando la sesión ONNX multilingual-e5-base (~265 MB cuantizada INT8). Solo en la primera solicitud tras el inicio — las siguientes son instantáneas.', pt: 'A inicializar a sessão ONNX multilingual-e5-base (~265 MB quantizada INT8). Apenas no primeiro pedido após o arranque — os seguintes são instantâneos.' },
  'EmbeddingStatus.downloadingTitle': { fr: "Téléchargement du modèle d'embedding…", de: 'Embedding-Modell wird heruntergeladen…', es: 'Descargando el modelo de embeddings…', pt: 'A transferir o modelo de embeddings…' },
  'EmbeddingStatus.embeddingTitle': { fr: 'Calcul des embeddings…', de: 'Embeddings werden berechnet…', es: 'Calculando embeddings…', pt: 'A calcular embeddings…' },
  'EmbeddingStatus.failedTitle': { fr: "Échec du chargement du modèle d'embedding", de: 'Embedding-Modell konnte nicht geladen werden', es: 'No se pudo cargar el modelo de embeddings', pt: 'Falha ao carregar o modelo de embeddings' },

  // Keys added with the Projects screen — full six-locale entries
  // (en/it included so the script can seed every locale at once).
  'Projects.editProject': { en: 'Edit project', it: 'Modifica progetto', fr: 'Modifier le projet', de: 'Projekt bearbeiten', es: 'Editar proyecto', pt: 'Editar projeto' },
  'Projects.descriptionPlaceholder': { en: 'Add a description (optional)', it: 'Aggiungi una descrizione (facoltativa)', fr: 'Ajoutez une description (facultative)', de: 'Beschreibung hinzufügen (optional)', es: 'Añade una descripción (opcional)', pt: 'Adicione uma descrição (opcional)' },
  'Projects.deletedToast': { en: 'Project deleted', it: 'Progetto eliminato', fr: 'Projet supprimé', de: 'Projekt gelöscht', es: 'Proyecto eliminado', pt: 'Projeto eliminado' },
  'Projects.deleteError': { en: 'Could not delete project', it: 'Impossibile eliminare il progetto', fr: 'Impossible de supprimer le projet', de: 'Projekt konnte nicht gelöscht werden', es: 'No se pudo eliminar el proyecto', pt: 'Não foi possível eliminar o projeto' },
  'Projects.deleteConfirmTitle': { en: 'Delete project?', it: 'Eliminare il progetto?', fr: 'Supprimer le projet ?', de: 'Projekt löschen?', es: '¿Eliminar el proyecto?', pt: 'Eliminar o projeto?' },
  'Projects.deleteConfirmBody': { en: '"{name}" and its contents will be permanently deleted.', it: '"{name}" e i suoi contenuti verranno eliminati definitivamente.', fr: '« {name} » et son contenu seront supprimés définitivement.', de: '„{name}“ und seine Inhalte werden endgültig gelöscht.', es: '«{name}» y su contenido se eliminarán de forma permanente.', pt: '“{name}” e o respetivo conteúdo serão eliminados definitivamente.' },
}

function flat(o, p = '') {
  const out = {}
  for (const [k, v] of Object.entries(o)) {
    const kk = `${p}${k}`
    if (v && typeof v === 'object') Object.assign(out, flat(v, kk + '.'))
    else out[kk] = v
  }
  return out
}

function setPath(obj, dotted, value) {
  const parts = dotted.split('.')
  let cur = obj
  for (let i = 0; i < parts.length - 1; i++) {
    cur[parts[i]] ??= {}
    cur = cur[parts[i]]
  }
  cur[parts.at(-1)] = value
}

const LOCALES = ['en', 'it', 'fr', 'de', 'es', 'pt']
let hadError = false

// Pass 1 — fill every locale with any T key it lacks.
for (const loc of LOCALES) {
  const file = join(localesDir, `${loc}.json`)
  const data = JSON.parse(readFileSync(file, 'utf-8'))
  const have = new Set(Object.keys(flat(data)))
  let added = 0
  for (const [key, langs] of Object.entries(T)) {
    if (have.has(key)) continue
    if (!langs[loc]) {
      console.error(`  no ${loc} translation for ${key}`)
      hadError = true
      continue
    }
    setPath(data, key, langs[loc])
    added++
  }
  writeFileSync(file, JSON.stringify(data, null, 2) + '\n', 'utf-8')
  console.log(`${loc}.json: +${added} keys (${Object.keys(flat(data)).length} total)`)
}

// Pass 2 — assert every locale now carries the identical key set.
const keySets = Object.fromEntries(
  LOCALES.map((l) => [
    l,
    new Set(Object.keys(flat(JSON.parse(readFileSync(join(localesDir, `${l}.json`), 'utf-8'))))),
  ]),
)
const base = keySets.en
for (const loc of LOCALES) {
  const missing = [...base].filter((k) => !keySets[loc].has(k))
  const extra = [...keySets[loc]].filter((k) => !base.has(k))
  if (missing.length || extra.length) {
    hadError = true
    for (const k of missing) console.error(`  ${loc} missing ${k}`)
    for (const k of extra) console.error(`  ${loc} has extra ${k}`)
  }
}

if (hadError) {
  console.error('\ni18n parity INCOMPLETE — see errors above')
  process.exit(1)
}
console.log(`\ni18n parity OK — all ${LOCALES.length} locales carry ${base.size} keys`)
