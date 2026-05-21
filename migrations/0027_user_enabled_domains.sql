-- Persist the subset of professional verticals the user has explicitly
-- enabled. The full canonical set is `crate::domain::DOMAINS` (10
-- entries: legal, medical, finance, real_estate, hr, insurance, ip,
-- compliance, pa, others). Without an explicit preference the app
-- behaves exactly as before — every domain is visible everywhere.
--
-- Storage: JSON array of canonical English IDs, e.g.
--   '["legal","compliance"]'
--
-- NULL means "no explicit preference" → all domains enabled. The
-- backend collapses the all-enabled case back to NULL on PUT so a
-- restored "factory" state is never observed as the explicit full list.
-- Set via PUT /user/enabled-domains from Settings → Domains.

ALTER TABLE user_settings ADD COLUMN enabled_domains TEXT;
