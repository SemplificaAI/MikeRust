-- tabular_reviews.workflow_id used to FK-reference workflows(id):
--
--   workflow_id TEXT REFERENCES workflows(id) ON DELETE SET NULL
--
-- The same issue migration 0022 fixed for `workflow_hidden`: built-in
-- workflow presets are loaded in-memory from config/workflow-presets/
-- *.json and never exist as rows in `workflows`, so creating a tabular
-- review against any preset (the medico-legale "Inventario documenti
-- medico-legali" was the canary on 2026-06-05) failed with SQLite
-- error 787 (FOREIGN KEY constraint failed). The user-visible symptom
-- is the "Nuova revisione" dialog surfacing the DB error inline and
-- refusing to create the review.
--
-- Rebuild the table keeping user_id and project_id FKs (both still
-- valid — those rows exist in the DB) and dropping the workflow_id
-- FK. workflow_id is now a free text id that may point at either a
-- DB workflow or an in-memory preset. ON DELETE SET NULL for
-- workflow_id is lost, but that is correct: a preset has no DB row to
-- cascade from, and a deleted user workflow leaving a stale
-- workflow_id on a review is harmless (the row simply never matches
-- when we look the workflow up at chat / render time).
--
-- The `domain` column was added by migration 0018 — preserve it. The
-- `idx_tabular_reviews_domain` index dies with the old table and is
-- recreated below.

PRAGMA foreign_keys = OFF;

CREATE TABLE tabular_reviews_new (
    id              TEXT PRIMARY KEY,
    user_id         TEXT NOT NULL REFERENCES user_profiles(id) ON DELETE CASCADE,
    project_id      TEXT REFERENCES projects(id) ON DELETE SET NULL,
    workflow_id     TEXT,
    title           TEXT NOT NULL DEFAULT 'Untitled Review',
    columns_config  TEXT NOT NULL DEFAULT '[]',
    status          TEXT NOT NULL DEFAULT 'pending',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    domain          TEXT NOT NULL DEFAULT 'legal'
);

INSERT INTO tabular_reviews_new
    (id, user_id, project_id, workflow_id, title, columns_config, status, created_at, updated_at, domain)
SELECT
    id, user_id, project_id, workflow_id, title, columns_config, status, created_at, updated_at, domain
FROM tabular_reviews;

DROP TABLE tabular_reviews;
ALTER TABLE tabular_reviews_new RENAME TO tabular_reviews;

CREATE INDEX IF NOT EXISTS idx_tabular_reviews_domain
    ON tabular_reviews (user_id, domain);

PRAGMA foreign_keys = ON;
