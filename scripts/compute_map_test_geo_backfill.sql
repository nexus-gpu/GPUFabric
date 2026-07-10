-- Test-environment-only geo backfill for /api/compute-map.
-- This script fills missing public_ip and geo_* fields with deterministic
-- simulated China city data so historical test devices can render on the map.
--
-- It only updates valid/warning rows that are missing geo data. Real devices
-- overwrite these values on their next gpuf-c login/heartbeat via gpuf-s.

WITH city_pool AS (
    SELECT *
    FROM (
        VALUES
            (1, '39.156.66.10'::inet, 'China', 'Beijing', 'Beijing', 39.9042::double precision, 116.4074::double precision),
            (2, '180.163.150.10'::inet, 'China', 'Shanghai', 'Shanghai', 31.2304::double precision, 121.4737::double precision),
            (3, '14.215.177.10'::inet, 'China', 'Guangdong', 'Shenzhen', 22.5431::double precision, 114.0579::double precision),
            (4, '183.232.231.10'::inet, 'China', 'Guangdong', 'Guangzhou', 23.1291::double precision, 113.2644::double precision),
            (5, '115.236.118.10'::inet, 'China', 'Zhejiang', 'Hangzhou', 30.2741::double precision, 120.1551::double precision),
            (6, '58.213.96.10'::inet, 'China', 'Jiangsu', 'Nanjing', 32.0603::double precision, 118.7969::double precision),
            (7, '182.150.0.10'::inet, 'China', 'Sichuan', 'Chengdu', 30.5728::double precision, 104.0665::double precision),
            (8, '119.84.0.10'::inet, 'China', 'Chongqing', 'Chongqing', 29.5630::double precision, 106.5516::double precision),
            (9, '111.175.0.10'::inet, 'China', 'Hubei', 'Wuhan', 30.5931::double precision, 114.3054::double precision),
            (10, '117.36.0.10'::inet, 'China', 'Shaanxi', 'Xi''an', 34.3416::double precision, 108.9398::double precision),
            (11, '60.28.0.10'::inet, 'China', 'Tianjin', 'Tianjin', 39.0842::double precision, 117.2009::double precision),
            (12, '120.221.0.10'::inet, 'China', 'Shandong', 'Qingdao', 36.0671::double precision, 120.3826::double precision)
    ) AS t(slot, public_ip, country, region, city, lat, lng)
),
targets AS (
    SELECT
        ga.client_id,
        row_number() OVER (ORDER BY ga.updated_at DESC NULLS LAST, ga.client_id) AS rn
    FROM gpu_assets ga
    WHERE COALESCE(ga.valid_status, 'valid') IN ('valid', 'warning')
      AND (
          ga.public_ip IS NULL
          OR ga.geo_city IS NULL
          OR ga.geo_latitude IS NULL
          OR ga.geo_longitude IS NULL
      )
),
assigned AS (
    SELECT
        targets.client_id,
        city_pool.public_ip,
        city_pool.country,
        city_pool.region,
        city_pool.city,
        city_pool.lat,
        city_pool.lng
    FROM targets
    JOIN city_pool
      ON city_pool.slot = ((targets.rn - 1) % (SELECT COUNT(*) FROM city_pool)) + 1
)
UPDATE gpu_assets ga
SET public_ip = assigned.public_ip,
    geo_country = assigned.country,
    geo_region = assigned.region,
    geo_city = assigned.city,
    geo_latitude = assigned.lat,
    geo_longitude = assigned.lng,
    geo_source = 'test-backfill',
    geo_updated_at = NOW(),
    updated_at = NOW()
FROM assigned
WHERE ga.client_id = assigned.client_id;
