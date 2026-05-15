-- workflow_hidden.workflow_id used to FK-reference workflows(id):
--
--   workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE
--
-- But the entire purpose of this table is to let a user hide SYSTEM
-- PRESET workflows. Presets are loaded in-memory from config/workflows/
-- *.json and never exist as rows in `workflows` — so inserting a preset
-- id into workflow_hidden violated the foreign key with SQLite error
-- 787 (FOREIGN KEY constraint failed), making the hide feature unusable
-- for exactly the workflows it was built for.
--
-- Rebuild the table keeping the user_id FK (still valid — users are
-- real rows) and dropping the workflow_id FK. workflow_id is now a free
-- text id that may point at either a DB workflow or an in-memory preset.
-- ON DELETE CASCADE for workflow_id is lost, but that is correct: a
-- preset has no DB row to cascade from, and a deleted user workflow
-- leaving a stale hidden-row is harmless (the row simply never matches).

PRAGMA foreign_keys = OFF;

CREATE TABLE workflow_hidden_new (
    user_id     TEXT NOT NULL REFERENCES user_profiles(id) ON DELETE CASCADE,
    workflow_id TEXT NOT NULL,
    PRIMARY KEY (user_id, workflow_id)
);

INSERT INTO workflow_hidden_new (user_id, workflow_id)
    SELECT user_id, workflow_id FROM workflow_hidden;

DROP TABLE workflow_hidden;
ALTER TABLE workflow_hidden_new RENAME TO workflow_hidden;

PRAGMA foreign_keys = ON;
