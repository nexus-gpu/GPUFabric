-- Additive schema alignment for the test environment.
-- This script only adds columns/tables/indexes that exist in production and
-- are missing from the current test database. It does not drop or copy data.

BEGIN;

ALTER TABLE public.device_points_credit_sync
ADD COLUMN IF NOT EXISTS target_credit_amount BIGINT;

ALTER TABLE public.device_points_credit_sync
ADD COLUMN IF NOT EXISTS previous_credit_amount BIGINT;

CREATE INDEX IF NOT EXISTS idx_device_points_credit_sync_user_status
ON public.device_points_credit_sync (user_id, status);

CREATE TABLE IF NOT EXISTS public.device_points_daily_backup (
    client_id BYTEA,
    device_index SMALLINT,
    date DATE,
    total_heartbeats INTEGER,
    device_id INTEGER,
    device_name VARCHAR,
    tflops DOUBLE PRECISION,
    multiplier NUMERIC,
    base_hours BIGINT,
    points NUMERIC,
    refreshed_at TIMESTAMP WITH TIME ZONE
);

ALTER TABLE public.topup_discount_logs
ADD COLUMN IF NOT EXISTS discount_amount BIGINT;

ALTER TABLE public.topup_discount_logs
ADD COLUMN IF NOT EXISTS final_amount BIGINT;

UPDATE public.topup_discount_logs
SET discount_amount = 0
WHERE discount_amount IS NULL;

UPDATE public.topup_discount_logs
SET final_amount = original_amount - discount_amount
WHERE final_amount IS NULL;

ALTER TABLE public.topup_discount_logs
ALTER COLUMN discount_amount SET NOT NULL;

ALTER TABLE public.topup_discount_logs
ALTER COLUMN final_amount SET NOT NULL;

COMMIT;
