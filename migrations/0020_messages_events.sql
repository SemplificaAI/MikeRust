-- Persist per-message non-text events on assistant turns.
--
-- The chat SSE stream emits several event types beyond plain text — most
-- notably `doc_created`, which produces the download card for a .docx /
-- .xlsx / … generated via `generate_docx`. Today those events live only
-- in the live stream; reopening the chat (`GET /chat/:id`) returns just
-- the prose content, so the download card disappears and the user has
-- no way to retrieve the file other than by digging into the storage
-- directory by hand.
--
-- Storing the events as a JSON array on the message row keeps the same
-- "single source of truth" architecture as `annotations` (migration
-- 0012): the live SSE path emits a `doc_created` event for the UI to
-- render immediately, the chat-history loader hydrates the same shape
-- from the persisted column, and both render through the existing
-- `DocDownloadBlock` frontend component. NULL means "no events" — i.e.
-- the message was pure prose, identical to today's behaviour.

ALTER TABLE messages ADD COLUMN events TEXT;
