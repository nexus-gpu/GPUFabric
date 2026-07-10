-- Rollback for scripts/prod_schema_add_compute_map_token.sql.
-- Prefer application rollback first. Run this only if the newly added geo and
-- token-stat schema must be removed and any inference_token_usage rows may be
-- discarded.

BEGIN;

DROP INDEX IF EXISTS public.idx_inference_token_usage_request_endpoint;
DROP INDEX IF EXISTS public.idx_inference_token_usage_client_created;
DROP INDEX IF EXISTS public.idx_inference_token_usage_created_at;
DROP TABLE IF EXISTS public.inference_token_usage;

ALTER TABLE public.gpu_assets
DROP COLUMN IF EXISTS geo_updated_at,
DROP COLUMN IF EXISTS geo_source,
DROP COLUMN IF EXISTS geo_longitude,
DROP COLUMN IF EXISTS geo_latitude,
DROP COLUMN IF EXISTS geo_city,
DROP COLUMN IF EXISTS geo_region,
DROP COLUMN IF EXISTS geo_country,
DROP COLUMN IF EXISTS public_ip;

COMMIT;
