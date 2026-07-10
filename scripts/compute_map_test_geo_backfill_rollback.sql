-- Roll back the test-environment-only geo backfill for /api/compute-map.
--
-- This intentionally only clears rows marked by compute_map_test_geo_backfill.sql.
-- Real gpuf-c/gpuf-s geo updates use geo_source = 'endpoint' and are preserved.

UPDATE gpu_assets
SET public_ip = NULL,
    geo_country = NULL,
    geo_region = NULL,
    geo_city = NULL,
    geo_latitude = NULL,
    geo_longitude = NULL,
    geo_source = NULL,
    geo_updated_at = NULL,
    updated_at = NOW()
WHERE geo_source = 'test-backfill';
