-- Production additive schema migration for compute-map and token statistics.
-- Safe to run multiple times. It only adds nullable columns, a new table, and
-- indexes required by gpuf-s api_server.

BEGIN;

ALTER TABLE public.gpu_assets
ADD COLUMN IF NOT EXISTS public_ip INET,
ADD COLUMN IF NOT EXISTS geo_country VARCHAR,
ADD COLUMN IF NOT EXISTS geo_region VARCHAR,
ADD COLUMN IF NOT EXISTS geo_city VARCHAR,
ADD COLUMN IF NOT EXISTS geo_latitude DOUBLE PRECISION,
ADD COLUMN IF NOT EXISTS geo_longitude DOUBLE PRECISION,
ADD COLUMN IF NOT EXISTS geo_source VARCHAR,
ADD COLUMN IF NOT EXISTS geo_updated_at TIMESTAMP WITH TIME ZONE;

CREATE TABLE IF NOT EXISTS public.inference_token_usage (
    id BIGSERIAL PRIMARY KEY,
    request_id VARCHAR,
    token_hash VARCHAR,
    client_id BYTEA NOT NULL,
    model VARCHAR NOT NULL,
    endpoint VARCHAR NOT NULL,
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    success BOOLEAN NOT NULL DEFAULT TRUE,
    stream BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_inference_token_usage_created_at
ON public.inference_token_usage (created_at DESC);

CREATE INDEX IF NOT EXISTS idx_inference_token_usage_client_created
ON public.inference_token_usage (client_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_inference_token_usage_request_endpoint
ON public.inference_token_usage (request_id, token_hash, endpoint)
WHERE request_id IS NOT NULL;

COMMIT;
