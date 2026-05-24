-- Per-chat Accept / Reject decision state for documents that the LLM
-- generated via `generate_docx` (and, by extension, any document the
-- user wants to flag in the chat context).
--
-- Until this migration the document viewer's "Tracked Change / Accept /
-- Reject" trio was a purely client-side preview toggle — flipping it
-- changed what the user saw in the panel but did NOT change what the
-- model received on the next turn. That mismatch made the buttons feel
-- decorative: the rejected version kept slipping back into the LLM
-- payload (load_attached_docs auto-attaches every documents row linked
-- to the chat via chat_id) and the user had no way to communicate WHY
-- the rejection happened.
--
-- The columns added here let the chat handler:
--   * accepted (default) — load the full extracted text into context,
--     same path the user-uploaded docs already take.
--   * rejected — substitute the full text with a synthetic stub:
--       "Previous version of <filename> rejected by the user with
--        reason: <decision_reason>. Summary of the rejected version:
--        <decision_summary>. Take this into account when redrafting."
--     The model never sees the rejected bytes again on subsequent
--     turns; it sees only why the user said no.
--
-- The row stays in `documents` either way — the viewer can reopen the
-- rejected version, the user can change their mind, and a later
-- accept/reject toggle is a single UPDATE (decision_reason and
-- decision_summary are kept across flips so a re-reject doesn't ask
-- the user to retype from scratch).
--
-- Decision scope is **per-chat by design**: every generated docx
-- lives on a single chat_id (set at creation time by generate_docx),
-- so a per-document column is implicitly per-chat. A docx that wants
-- to escape this scope can be re-uploaded as a new attachment in a
-- different chat — that new row gets its own fresh decision state.

ALTER TABLE documents ADD COLUMN decision TEXT NOT NULL DEFAULT 'accepted'
    CHECK (decision IN ('accepted', 'rejected'));
ALTER TABLE documents ADD COLUMN decision_reason TEXT;
ALTER TABLE documents ADD COLUMN decision_summary TEXT;
