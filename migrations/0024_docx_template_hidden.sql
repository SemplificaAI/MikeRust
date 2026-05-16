-- Lets a user hide DOCX templates they don't want cluttering the list —
-- both shipped system templates (loaded in-memory from
-- config/docx-templates/, never rows in any table) and user templates.
--
-- template_id is therefore a free-text id with no foreign key: it may
-- point at an in-memory system template (`it/diffida-...`) or a user
-- template (`user/...`). Only the user_id FK is real. A stale hidden
-- row left behind by a deleted user template is harmless — it simply
-- never matches anything again.

CREATE TABLE IF NOT EXISTS docx_template_hidden (
    user_id     TEXT NOT NULL REFERENCES user_profiles(id) ON DELETE CASCADE,
    template_id TEXT NOT NULL,
    PRIMARY KEY (user_id, template_id)
);
