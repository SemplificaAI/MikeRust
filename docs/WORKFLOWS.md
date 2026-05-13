# Workflows, Tabular Reviews, and Assistants — User Manual

This manual explains the three core artefacts MikeRust exposes for
repeatable AI work — **Workflows**, **Tabular Reviews**, and the
**Assistant** chat — how they relate, and how to design new ones for
your own profession. MikeRust ships with 14 built-in workflows aimed at
legal practice, but the underlying model is **domain-agnostic**: the
same building blocks describe medical-record reviews, M&A IC memos,
real-estate due diligence, HR resume screening, insurance claims
triage, patent landscapes, compliance audits, and dozens of other
document-driven tasks. The legal slant is in the *built-ins*, not in
the framework.

This manual is written for an end user who wants to *prepare* new
workflows in the UI. For the contributor track (editing the shipped
constants in TypeScript, adding new practice areas, etc.) see the
"Going beyond the UI" section at the bottom.

---

## 1. The mental model

There are three artefacts. Read this section twice — the rest of the
manual assumes you know which one you are designing at each step.

```
┌──────────────────────────────────────────────────────────────────┐
│ WORKFLOW          The recipe. A reusable prompt + (optionally)   │
│                   a column definition. Two flavours: "assistant" │
│                   (a free-form prompt) or "tabular" (the same    │
│                   prompt + a schema of columns).                 │
│                                                                  │
│   ┌────────────────────────────────────────┐                     │
│   │ TABULAR REVIEW   One cooking of a      │                     │
│   │                  tabular workflow      │                     │
│   │                  over a chosen set of  │                     │
│   │                  documents. Produces a │                     │
│   │                  table: one row per    │                     │
│   │                  document, one column  │                     │
│   │                  per workflow column.  │                     │
│   └────────────────────────────────────────┘                     │
│                                                                  │
│   ┌────────────────────────────────────────┐                     │
│   │ ASSISTANT CHAT   A free-form chat. You │                     │
│   │                  can optionally point  │                     │
│   │                  it at an "assistant"  │                     │
│   │                  workflow so the model │                     │
│   │                  starts with that      │                     │
│   │                  role/instructions     │                     │
│   │                  baked in.             │                     │
│   └────────────────────────────────────────┘                     │
└──────────────────────────────────────────────────────────────────┘
```

**Pick a workflow type up front**:

- **`type: "assistant"`** — for any task whose output is **free-form
  text or a generated document**. Examples: "Summarise this credit
  agreement", "Draft a response letter", "Generate a Conditions
  Precedent checklist as a .docx". A user starts a chat, picks the
  workflow, attaches documents, sends the message.
- **`type: "tabular"`** — for any task whose output is **structured
  rows × columns**. Examples: "Extract parties, term, governing law
  from each NDA in this folder", "List all change-of-control clauses
  across these 30 agreements", "Triage 50 insurance claims by severity
  and missing documentation". A user creates a Tabular Review, attaches
  the documents, hits Run, and gets a table back.

The decision is **the shape of the output**, not the topic. A "summary
of regulatory disclosures" can be an assistant workflow (one long
markdown summary per chat) **or** a tabular workflow (one row per
disclosure with columns Topic / Severity / Citation). Same domain,
different shape.

---

## 2. Workflow anatomy

A workflow is a row in the `workflows` table. Five fields matter at
authoring time:

| Field | Meaning |
|---|---|
| `title` | Shown in the workflow list and in the chat composer's workflow picker. Keep it action-oriented: "Review NDA terms", not "NDA workflow". |
| `type` | `"assistant"` or `"tabular"`. Drives where the workflow appears in the UI and how the model is asked to respond. |
| `practice` | A category tag. Used for filtering the workflow list. Pick from the shipped list (see §5) or type a custom one. |
| `prompt_md` | The instructions to the model, written in Markdown. This is the most important field. See §7 for guidance. |
| `columns_config` | (tabular only) An array of column definitions. Each column has a `name`, a `prompt`, and an optional `format`. See §3. |

Two flags are managed by the system, not by the author:

- `is_system: true` for built-in workflows — uneditable, undeletable
  (you can hide them, you can duplicate them to make a custom variant).
- `user_id` — null for built-ins, your user id for your own workflows.

---

## 3. Tabular Reviews — column model

A tabular workflow defines a **schema**. Running the workflow over N
documents produces an N × M table (M = number of columns).

Each column in `columns_config` has:

```ts
{
  index: 0,                              // ordinal — controls column order
  name: "Governing Law",                 // header shown in the table
  prompt: "What law governs this agreement? Cite the clause.",
  format: "text",                        // optional — see below
  tags: []                               // optional — UI filter hints
}
```

The `prompt` is **per-cell**. The model reads it together with the
document content, the workflow-level `prompt_md` (which sets the
overall posture), and any general context the user provided in the
review setup, then produces the value for that one cell.

**Available formats** (drives display and post-processing — `text` is
the default if you omit it):

| Format | Use for | Display |
|---|---|---|
| `text` | Free-form answer, short paragraph. | Rendered as a single-line string with hover for the full text. |
| `bulleted_list` | Multiple items the model should enumerate. | One `•` per line. The prompt should say "list … each on its own line". |
| `number` | A plain count or integer. | Right-aligned numeric cell. |
| `currency` | A monetary value with currency code. | Formatted with locale (e.g. `€ 1,250,000`). |
| `monetary_amount` | Like `currency` but also captures the unit and basis. | Three sub-fields (value / currency / note). |
| `percentage` | A 0-100 number with a % sign. | Right-aligned, suffix `%`. |
| `yes_no` | Boolean. | Pill: green YES / red NO / grey N/A. |
| `date` | An ISO date. | Locale-formatted, sortable. |
| `tag` | A single short label from a small enum (e.g. severity, status). | Coloured pill. The prompt should say "answer with one of: low / medium / high". |

A practical rule of thumb: **the column prompt should describe both
*what* the model is looking for AND *how* to format the answer.** The
`format` setting changes the rendering but doesn't constrain the
model — that's what the prompt is for. A `number` column whose prompt
says "How many parties are involved?" will get a clean integer; the
same column without that instruction may get "There are seven parties
in this agreement", which renders weirdly.

---

## 4. Assistants and workflow injection in chat

The Assistant view is the chat interface. By default it's a generic
conversation: you type, the model responds, you attach documents, etc.

A workflow can be injected into a chat **only if it's an
`"assistant"`-type workflow**. The flow:

1. In the chat composer, click "Add workflow".
2. Pick an assistant workflow from the list (built-ins + your own).
3. Type any additional context you want and send.

Mechanically the frontend prefixes the message with a marker like
`[Workflow: Generate CP Checklist (id: builtin-cp-checklist)]`. The
backend recognises the marker, fetches the workflow's `prompt_md`, and
prepends it to the conversation as system instructions for that turn.
Subsequent messages in the same chat reuse the same context implicitly
until you remove the workflow.

**Why "assistant" workflows only?** Tabular workflows have no "chat"
shape — they produce a table. They're invoked through the Tabular
Review view, not the chat composer. If your workflow needs to be
chat-driven, set its `type` to `"assistant"`.

---

## 5. Practice areas — how to use them outside law

The `practice` field is a free string but the dropdown is populated
from a shipped list (19 entries):

```
General Transactions, Corporate, Finance, Litigation, Real Estate,
Tax, Employment, IP, Competition, Tech Transactions, Project Finance,
EC/VC, Private Equity, Private Credit, ECM, DCM, Lev Fin, Arbitration,
Others
```

These are clearly law-firm-ish. For non-legal use:

- **"Others" + a custom string**: pick `Others` in the dropdown and
  type your own practice area (e.g. `Medical Records Review`,
  `Real Estate Inspection`, `Resume Screening`). The string is stored
  verbatim and shown wherever the workflow appears. You can filter
  your workflow list by practice, and `Others` collects every custom
  one alongside.
- **Recurring custom areas → ship them**: if your team uses the same
  custom practice repeatedly (e.g. an MD always categorises everything
  as `Clinical Trial Review`), it's worth adding it to the shipped
  dropdown. See "Going beyond the UI" at the bottom.

The practice field has **no functional effect** on what the model does
— it's purely a UI categoriser. Don't put behaviour-changing
instructions there; put them in `prompt_md`.

---

## 6. Writing a good `prompt_md`

The `prompt_md` is what makes the workflow useful. Treat it as a
hand-off note to a smart contractor who's about to look at the
documents for the first time. Markdown is allowed — headings, lists,
code blocks, bold.

**For assistant workflows**, structure suggested:

```markdown
## Role and goal
You are a [profession] reviewing [doc type]. The goal is to [outcome].

## What to read first
Skim the [specific section]. Identify [signal].

## How to answer
Reply with a [structure]:
- [Section 1]
- [Section 2]
- A short flag list of anything missing.

## Things to double-check
- [Pitfall 1]
- [Pitfall 2]

## Tools
[Tell the model what tools to use, if any — e.g. generate_docx for an
output document.]
```

**For tabular workflows**, the `prompt_md` sets the **overall posture**
and each `column.prompt` does the per-cell work. Pattern:

```markdown
## Goal
This review extracts the key terms of [doc type] across [N] documents.

## Reading order
For each document, first identify [anchor]. Then answer the columns in
order.

## Style
- One-line answers; cite the clause number in parentheses where helpful.
- If the document doesn't address a column, answer "Not addressed" —
  do not invent.
```

Then each column prompt focuses on **one specific extraction** and
**the answer shape**:

- Good: `"What is the governing law of this agreement? State the
  jurisdiction (e.g. 'New York', 'England & Wales') and the clause
  number. If silent, answer 'Not specified'."`
- Bad: `"Governing law"` (too vague — you'll get inconsistent shapes)

**Three universally useful clauses to add to most prompts**:

1. **"Cite the clause/section/page you took this from."** Mike's
   citation system surfaces these inline as clickable pills.
2. **"If the document is silent on this, answer X — do not invent."**
   Forces an honest fail-mode instead of hallucination.
3. **"Be concise; output the answer only, no preamble."** Stops the
   model from saying "Sure, I'll be glad to help! …" in front of every
   answer.

---

## 7. Step by step — creating a new workflow

### 7.1 Assistant workflow

1. Go to **Workflows** in the sidebar.
2. Click **+ Create new**.
3. Fill in:
   - **Title** (action-oriented).
   - **Type**: `Assistant`.
   - **Practice area**: pick from the list or use `Others`.
   - **Prompt**: write your `prompt_md` following the structure in §6.
4. Save.
5. To use it: go to **Assistant**, attach the relevant document(s),
   click "Add workflow", pick yours, send your question.

### 7.2 Tabular workflow

1. Go to **Workflows** → **+ Create new**.
2. Fill in:
   - **Title**, **Type**: `Tabular`, **Practice area**.
   - **Prompt**: the workflow-level instructions (posture + style — see §6).
3. Add columns one by one:
   - **Name**: header text.
   - **Prompt**: per-cell extraction instruction.
   - **Format**: pick from the table in §3.
4. Save.
5. To use it: go to **Tabular Reviews** → **+ New review**, pick the
   workflow, attach the documents, hit Run.

---

## 8. Examples across professions

These are illustrative — copy them, adapt to your own data.

### 8.1 Medical — Patient record extraction (tabular)

- **Practice**: `Others — Medical Records Review`
- **Workflow prompt**: "You are a clinical analyst reviewing patient
  charts for a longitudinal cohort study. For each chart, extract the
  fields below. Cite the page number. If a field is not in the chart,
  answer `Not recorded` — do not infer."
- **Columns**:

| Name | Format | Prompt |
|---|---|---|
| Patient ID | `text` | "What is the patient ID or MRN on this chart? Page reference required." |
| Age at intake | `number` | "What is the patient's age at the date of the first visit recorded? Give an integer in years." |
| Primary diagnosis (ICD-10) | `text` | "What is the primary diagnosis? Provide the ICD-10 code if present, else the textual diagnosis." |
| Comorbidities | `bulleted_list` | "List the patient's documented comorbidities, one per line. Do not include negative findings." |
| Last visit date | `date` | "Date of the last clinical encounter recorded in this chart. ISO format." |
| Smoker | `yes_no` | "Is the patient currently a smoker? Answer YES / NO / N/A based on the social history section." |
| Risk tier | `tag` | "Assign a tier: `low`, `moderate`, `high`. Base on age + comorbidities + most recent labs. State your reasoning briefly." |

### 8.2 Finance — M&A IC memo summary (assistant)

- **Practice**: `Corporate` or `Others — M&A`
- **Workflow prompt**:

```markdown
## Role and goal
You are an analyst preparing a one-page IC memo from the attached
information memorandum and accompanying due diligence files.

## Output structure
Produce a Markdown document with these sections in this order:
- **Transaction at a glance** — 3 bullets (target, sector, deal size).
- **Strategic rationale** — 2 short paragraphs.
- **Financial highlights** — table: FY-3 / FY-2 / LTM / Run-rate, rows
  for Revenue / EBITDA / EBITDA margin / Net debt.
- **Key risks** — bulleted list, max 6.
- **Open diligence items** — bulleted list of things the docs don't
  cover.

## Style
- No marketing language. Skeptical, factual tone.
- Cite specific schedules/exhibits where helpful.
- If a section can't be filled from the docs, say so explicitly.
```

### 8.3 Real estate — Property inspection checklist (tabular)

- **Practice**: `Real Estate`
- **Workflow prompt**: "You are reviewing engineering inspection
  reports for a real-estate acquisition. For each report, extract the
  condition assessment by trade. State the page reference for each
  finding."
- **Columns**: Property address (`text`), Inspection date (`date`),
  Roof condition (`tag`: good/fair/poor), HVAC age years (`number`),
  Electrical compliance (`yes_no`), Outstanding items (`bulleted_list`),
  Estimated CapEx 12 months (`currency`).

### 8.4 HR — Resume screening (tabular)

- **Practice**: `Others — Recruiting`
- **Workflow prompt**: "Screen each CV against the role spec attached
  to this review. Be objective; cite the exact phrases from the CV
  that support your answer."
- **Columns**: Candidate name (`text`), Years of relevant experience
  (`number`), Required skills present (`bulleted_list`), Skills
  missing (`bulleted_list`), Compensation expectation (`currency`),
  English level inferred (`tag`: native/fluent/intermediate/basic),
  Recommendation (`tag`: advance/hold/reject), Rationale (`text`).

### 8.5 Insurance — Claims triage (tabular)

- **Practice**: `Others — Insurance`
- **Workflow prompt**: "Triage each claim packet. Flag any packet
  where documentation is incomplete or where the loss description
  doesn't match the policy coverage."
- **Columns**: Claim number (`text`), Date of loss (`date`), Cause of
  loss (`tag`: fire/water/wind/theft/other), Reserve amount
  (`currency`), Documentation complete (`yes_no`), Missing items
  (`bulleted_list`), Coverage match (`yes_no`), Triage priority
  (`tag`: urgent/standard/low).

### 8.6 IP — Patent landscape (tabular)

- **Practice**: `IP`
- **Workflow prompt**: "Summarise each patent for a competitive
  landscape study. Cite claim numbers."
- **Columns**: Patent number (`text`), Filing date (`date`), Owner
  (`text`), Independent claim 1 paraphrase (`text`), Key technical
  feature (`bulleted_list`), Relevance to product X (`tag`:
  blocking/adjacent/orthogonal), Freedom-to-operate concern (`yes_no`).

### 8.7 Compliance — Audit findings register (tabular)

- **Practice**: `Others — Compliance`
- **Workflow prompt**: "Review each audit report. For each finding,
  produce a row. Map findings to your firm's risk register codes if
  visible."
- **Columns**: Finding ID (`text`), Domain (`tag`: financial / IT /
  operational / regulatory), Severity (`tag`: low/medium/high/critical),
  Remediation owner (`text`), Target close date (`date`), Status
  (`tag`: open/in-progress/closed/overdue), Linked control (`text`).

---

## 9. Working with built-in workflows

MikeRust ships 14 built-in workflows, all currently in the
legal/transactional domain (`Generate CP Checklist`, `Change of Control
Review`, `Credit Agreement Summary`, `NDA Review`, `SPA Review`, etc.).
They live as TypeScript constants in
[`frontend/src/app/components/workflows/builtinWorkflows.ts`](../frontend/src/app/components/workflows/builtinWorkflows.ts)
and are merged with your own user workflows at runtime.

In the UI:

- Built-ins have an "Intégrato" / "Built-in" badge.
- You cannot edit or delete a built-in.
- You **can hide** it (right-click → Hide) so it doesn't clutter your
  list. Hidden built-ins are reachable from the **Hidden** tab.
- You **can duplicate** it (right-click → Duplicate). The duplicate
  becomes your own editable user workflow — useful as a starting point
  for variants ("NDA Review — Italian jurisdiction tweaks").

**Tip — designing a new workflow for an unfamiliar domain**:

1. Find a built-in whose *shape* is closest to what you need (a
   tabular review with N columns, or an assistant that emits a .docx
   document, etc.).
2. Duplicate it.
3. Rewrite the title, practice, `prompt_md`, and column prompts to
   match your domain. Keep the structural scaffolding (column count,
   formats) for the first pass.
4. Run it on 2–3 real documents. Iterate on the column prompts.

---

## 10. Limits, gotchas, and known pitfalls

- **No DB seed for built-ins.** They live in the frontend bundle, not
  in the database. This means: deleting your `mike.db` doesn't lose
  them; but if you customise a built-in you have to duplicate it first
  (the original is immutable in your DB anyway).
- **Practice list is shipped.** Adding a permanent new practice area
  to the dropdown requires editing
  [`practices.ts`](../frontend/src/app/components/workflows/practices.ts)
  and recompiling. End users have `Others` + free text as the runtime
  escape hatch.
- **`columns_config` is JSON in the DB.** Schema changes to columns
  (renaming a column, changing a format) on an existing tabular review
  do not retroactively rewrite the cell values — you'll see mixed
  shapes until you re-run those rows.
- **No conditional columns.** Every column is filled for every row.
  If a column doesn't apply to some documents, instruct the prompt to
  answer `Not applicable` and rely on visual filtering in the table.
- **Column prompts can't reference other columns.** They're independent.
  If column B needs the result of column A, fold that logic into B's
  own prompt ("Identify the parties (column A's task), then for each
  party state their role").
- **Tabular review rerun policy.** Rerunning a row replaces all its
  cells. Rerunning a single cell replaces just that one. Rerunning a
  column re-runs that column across all rows.

---

## 11. Going beyond the UI

If you find yourself working in a non-legal domain regularly, two
upgrades make sense:

### 11.1 Add a practice area to the shipped dropdown

Edit
[`frontend/src/app/components/workflows/practices.ts`](../frontend/src/app/components/workflows/practices.ts):

```ts
export const PRACTICE_OPTIONS = [
    // existing legal entries…
    "Others",
    "Medical Records Review",        // ← add your domain
    "Property Inspection",
    "Resume Screening",
] as const;
```

Also add an i18n entry per locale in
[`frontend/messages/it.json`](../frontend/messages/it.json),
[`en.json`](../frontend/messages/en.json),
[`fr.json`](../frontend/messages/fr.json) under
`Workflows.practiceLabels` so the new area renders with a proper
display name. Rebuild the frontend (or wait for the dev-mode HMR pass).

### 11.2 Ship a built-in workflow for your domain

Edit
[`frontend/src/app/components/workflows/builtinWorkflows.ts`](../frontend/src/app/components/workflows/builtinWorkflows.ts)
and append a new entry to the `BUILT_IN_WORKFLOWS` array following the
shape of the existing ones. Keep `is_system: true`, `user_id: null`,
and use a stable `id` prefixed with `builtin-` (the prefix is the
convention; the loader treats anything in this array as system-shipped).

Re-launch the app. Your new workflow appears alongside the legal
built-ins in the workflow list. If you're sharing your build with
colleagues, this is how they get the workflow without having to type
it themselves.

### 11.3 Long-term — a workflows-plugin manifest

Right now built-ins are TypeScript constants. The plugin system used
for legal-corpus connectors (see
[`docs/CORPUS_PLUGINS.md`](CORPUS_PLUGINS.md)) is a natural template
for a future workflows plugin format: drop a JSON file into
`workflow-plugins/` and have it merge into the registry at startup.
This is on the roadmap but not yet implemented. If you'd find it
useful, open an issue —
[github.com/SemplificaAI/MikeRust/issues](https://github.com/SemplificaAI/MikeRust/issues).

---

## 12. Reference — file locations

| What | Where |
|---|---|
| Workflow type definition | [`frontend/src/app/components/shared/types.ts:305-318`](../frontend/src/app/components/shared/types.ts#L305-L318) |
| Column type + formats | [`frontend/src/app/components/shared/types.ts:253-270`](../frontend/src/app/components/shared/types.ts#L253-L270) |
| Built-in workflows | [`frontend/src/app/components/workflows/builtinWorkflows.ts`](../frontend/src/app/components/workflows/builtinWorkflows.ts) |
| Practice areas dropdown | [`frontend/src/app/components/workflows/practices.ts`](../frontend/src/app/components/workflows/practices.ts) |
| Workflow editor UI | [`frontend/src/app/components/workflows/NewWorkflowModal.tsx`](../frontend/src/app/components/workflows/NewWorkflowModal.tsx) |
| Tabular review UI | [`frontend/src/app/components/tabular/`](../frontend/src/app/components/tabular/) |
| Workflow injection in chat | [`src/routes/chat.rs`](../src/routes/chat.rs) (search for `[Workflow:`) |
| Backend routes | [`src/routes/workflows.rs`](../src/routes/workflows.rs), [`src/routes/tabular_reviews.rs`](../src/routes/tabular_reviews.rs) |
| DB schema | [`migrations/0001_initial.sql`](../migrations/0001_initial.sql), [`migrations/0002_tabular_workflow_hidden.sql`](../migrations/0002_tabular_workflow_hidden.sql), [`migrations/0010_workflows_extend.sql`](../migrations/0010_workflows_extend.sql) |
| i18n strings | [`frontend/messages/{it,en,fr}.json`](../frontend/messages/) under `Workflows`, `TabularReviews`, `Assistant`, `WorkflowColumns` |
