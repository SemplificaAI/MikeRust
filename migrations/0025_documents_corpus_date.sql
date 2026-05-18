-- Persist source publication/date metadata for corpus-backed documents.
--
-- `corpus_date` stores the date surfaced by the upstream source (when
-- available), e.g. publication or act date in ISO-like text format.
-- Nullable for existing rows and sources that do not expose a date.

ALTER TABLE documents ADD COLUMN corpus_date TEXT;

CREATE INDEX IF NOT EXISTS idx_documents_corpus_date
    ON documents(corpus_id, corpus_date);

-- Backfill existing italian-legal rows from the local italian_corpus index.
UPDATE documents
SET corpus_date = (
    SELECT ic.date
    FROM italian_corpus ic
    WHERE ic.hf_id = documents.corpus_identifier
)
WHERE corpus_id = 'italian-legal'
  AND corpus_date IS NULL
  AND corpus_identifier IS NOT NULL;

-- Backfill existing bulk-corpus rows (DILA-like) from corpus_documents.
UPDATE documents
SET corpus_date = (
    SELECT COALESCE(NULLIF(cd.date_texte, ''), NULLIF(cd.date_publi, ''))
    FROM corpus_documents cd
    WHERE cd.corpus_id = documents.corpus_id
      AND cd.identifier = documents.corpus_identifier
)
WHERE corpus_date IS NULL
  AND corpus_id IS NOT NULL
  AND corpus_identifier IS NOT NULL;
