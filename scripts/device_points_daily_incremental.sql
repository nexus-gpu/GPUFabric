-- Incremental device_points_daily migration and refresh
-- - Preserve existing materialized view data with original multipliers
-- - Backfill once with original multipliers  
-- - Refresh incrementally (today + yesterday) with 0.05 for new data

-- DO NOT update existing device_types multipliers
-- Keep original multipliers for historical data

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public'
          AND c.relname = 'device_points_daily'
          AND c.relkind = 'm'
    ) THEN
        ALTER MATERIALIZED VIEW device_points_daily RENAME TO device_points_daily_mv;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS device_points_daily (
    client_id BYTEA NOT NULL,
    device_index SMALLINT NOT NULL,
    date DATE NOT NULL,
    total_heartbeats INTEGER NOT NULL DEFAULT 0,
    device_id INTEGER,
    device_name VARCHAR(255),
    tflops DOUBLE PRECISION,
    multiplier NUMERIC(10, 4) NOT NULL DEFAULT 1.0,
    base_hours NUMERIC NOT NULL DEFAULT 0,
    points NUMERIC NOT NULL DEFAULT 0,
    refreshed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_device_points_daily_pk
ON device_points_daily (client_id, device_index, date);

CREATE INDEX IF NOT EXISTS idx_device_points_daily_date ON device_points_daily (date);
CREATE INDEX IF NOT EXISTS idx_device_points_daily_client_id ON device_points_daily (client_id);
CREATE INDEX IF NOT EXISTS idx_device_points_daily_device_index ON device_points_daily (device_index);

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public'
          AND c.relname = 'device_points_daily_mv'
          AND c.relkind = 'm'
    ) THEN
        -- Ensure unique index exists before INSERT
        CREATE UNIQUE INDEX IF NOT EXISTS idx_device_points_daily_pk
        ON device_points_daily (client_id, device_index, date);
        
        INSERT INTO device_points_daily (
            client_id,
            device_index,
            date,
            total_heartbeats,
            device_id,
            device_name,
            tflops,
            multiplier,
            base_hours,
            points,
            refreshed_at
        )
        SELECT
            client_id,
            device_index,
            date,
            total_heartbeats,
            device_id,
            device_name,
            tflops,
            multiplier,
            base_hours,
            points,
            refreshed_at
        FROM device_points_daily_mv
        ON CONFLICT (client_id, device_index, date) DO NOTHING;

        DROP MATERIALIZED VIEW device_points_daily_mv;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS device_points_daily_backfill (
    id SMALLINT PRIMARY KEY DEFAULT 1,
    completed_at TIMESTAMPTZ
);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM device_points_daily_backfill WHERE id = 1
    ) THEN
        -- Ensure unique index exists before INSERT
        CREATE UNIQUE INDEX IF NOT EXISTS idx_device_points_daily_pk
        ON device_points_daily (client_id, device_index, date);
        
        INSERT INTO device_points_daily (
            client_id,
            device_index,
            date,
            total_heartbeats,
            device_id,
            device_name,
            tflops,
            multiplier,
            base_hours,
            points,
            refreshed_at
        )
        SELECT
            s.client_id,
            s.device_index,
            s.date,
            s.total_heartbeats,
            di.device_id,
            dt.device_name,
            dt.tflops,
            COALESCE(dt.points_multiplier, 1.0) AS multiplier,
            s.base_hours,
            (s.base_hours::NUMERIC * COALESCE(dt.points_multiplier, 1.0)) AS points,
            NOW() AS refreshed_at
        FROM (
            SELECT
                dds.client_id,
                dds.device_index,
                dds.date,
                dds.total_heartbeats,
                ((dds.total_heartbeats::BIGINT * COALESCE(hcd.heartbeat_interval_secs, 120)::BIGINT) / 3600) AS base_hours
            FROM device_daily_stats dds
            LEFT JOIN heartbeat_config_daily hcd
                ON hcd.date = dds.date
        ) s
        LEFT JOIN device_info di
            ON di.client_id = s.client_id
           AND di.device_index = s.device_index
        LEFT JOIN device_types dt
            ON dt.device_id = di.device_id
        ON CONFLICT (client_id, device_index, date)
        DO UPDATE SET
            total_heartbeats = EXCLUDED.total_heartbeats,
            device_id = EXCLUDED.device_id,
            device_name = EXCLUDED.device_name,
            tflops = EXCLUDED.tflops,
            multiplier = EXCLUDED.multiplier,
            base_hours = EXCLUDED.base_hours,
            points = EXCLUDED.points,
            refreshed_at = NOW();

        INSERT INTO device_points_daily_backfill (id, completed_at)
        VALUES (1, NOW())
        ON CONFLICT (id) DO UPDATE SET completed_at = EXCLUDED.completed_at;
    END IF;
END $$;

CREATE OR REPLACE FUNCTION refresh_device_points_daily()
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    start_date DATE := (CURRENT_DATE - INTERVAL '1 day')::DATE;
    end_date DATE := CURRENT_DATE;
BEGIN
    INSERT INTO device_points_daily (
        client_id,
        device_index,
        date,
        total_heartbeats,
        device_id,
        device_name,
        tflops,
        multiplier,
        base_hours,
        points,
        refreshed_at
    )
    SELECT
        s.client_id,
        s.device_index,
        s.date,
        s.total_heartbeats,
        di.device_id,
        dt.device_name,
        dt.tflops,
        COALESCE(dt.points_multiplier, 0.05) AS multiplier,
        s.base_hours,
        (s.base_hours::NUMERIC * COALESCE(dt.points_multiplier, 0.05)) AS points,
        NOW() AS refreshed_at
    FROM (
        SELECT
            dds.client_id,
            dds.device_index,
            dds.date,
            dds.total_heartbeats,
            ((dds.total_heartbeats::BIGINT * COALESCE(hcd.heartbeat_interval_secs, 120)::BIGINT) / 3600) AS base_hours
        FROM device_daily_stats dds
        LEFT JOIN heartbeat_config_daily hcd
            ON hcd.date = dds.date
        WHERE dds.date BETWEEN start_date AND end_date
    ) s
    LEFT JOIN device_info di
        ON di.client_id = s.client_id
       AND di.device_index = s.device_index
    LEFT JOIN device_types dt
        ON dt.device_id = di.device_id
    ON CONFLICT (client_id, device_index, date)
    DO UPDATE SET
        total_heartbeats = EXCLUDED.total_heartbeats,
        device_id = EXCLUDED.device_id,
        device_name = EXCLUDED.device_name,
        tflops = EXCLUDED.tflops,
        multiplier = EXCLUDED.multiplier,
        base_hours = EXCLUDED.base_hours,
        points = EXCLUDED.points,
        refreshed_at = NOW();
END;
$$;
