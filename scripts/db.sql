-- Create database if not exists (for manual initialization)
-- Note: Docker entrypoint will create database via POSTGRES_DB env var
SELECT 'CREATE DATABASE "GPUFabric"'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'GPUFabric')\gexec

-- Connect to GPUFabric database
\c GPUFabric

-- Create tokens table for authentication (only used fields)
CREATE TABLE IF NOT EXISTS "public"."tokens" (
    "id" BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    "user_id" BIGINT NOT NULL,
    "key" CHAR(48) UNIQUE NOT NULL,
    "status" BIGINT DEFAULT 1,
    "expired_time" BIGINT DEFAULT -1,  -- -1: never expires, otherwise unix timestamp
    "deleted_at" TIMESTAMP WITH TIME ZONE,
    "access_level" INTEGER DEFAULT 1  -- -1: all devices, 1: user's devices only
);

CREATE INDEX IF NOT EXISTS idx_tokens_key ON "public"."tokens" ("key");
CREATE INDEX IF NOT EXISTS idx_tokens_user_id ON "public"."tokens" ("user_id");
CREATE INDEX IF NOT EXISTS idx_tokens_deleted_at ON "public"."tokens" ("deleted_at");

DO $$
BEGIN
    BEGIN
        CREATE UNIQUE INDEX IF NOT EXISTS idx_tokens_key_unique ON "public"."tokens" ("key");
    EXCEPTION
        WHEN others THEN
            NULL;
    END;
END $$;

-- Create GPU assets table for client info

CREATE TABLE  IF NOT EXISTS  "public"."gpu_assets" (
    "user_id" VARCHAR,
    "client_id" BYTEA PRIMARY KEY,
    "client_name" VARCHAR,
    "client_status" VARCHAR DEFAULT 'active',
    "valid_status" VARCHAR DEFAULT 'valid',
    "os_type" VARCHAR,
    "outo_set_model" BOOLEAN DEFAULT TRUE,
    "model" VARCHAR,
    "model_version" VARCHAR,
    "model_version_code" BIGINT,
    "created_at" TIMESTAMP DEFAULT NOW(),
    "updated_at" TIMESTAMP DEFAULT NOW()
);

 CREATE INDEX IF NOT EXISTS idx_gpu_assets_user_id_client_name
 ON "public"."gpu_assets" ("user_id", "client_name");

CREATE TABLE IF NOT EXISTS "public"."pod_info" (
    "pod_id" UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    "client_id" BYTEA NOT NULL REFERENCES "public"."gpu_assets" ("client_id") ON DELETE CASCADE,
    "pod_name" VARCHAR(255) NOT NULL,
    "node_name" VARCHAR(255),
    "pod_type" VARCHAR(64) DEFAULT 'compute',
    "device_count" SMALLINT DEFAULT 0,
    "total_memory_mb" INTEGER DEFAULT 0,
    "total_power_w" INTEGER DEFAULT 0,
    "auto_set_model" BOOLEAN DEFAULT TRUE,
    "model" VARCHAR(255),
    "model_version" VARCHAR(255),
    "model_version_code" BIGINT,
    "created_at" TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    "updated_at" TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(client_id, pod_name)
);

CREATE INDEX IF NOT EXISTS idx_pod_client_id ON "public"."pod_info" ("client_id");

CREATE TABLE  IF NOT EXISTS  "public"."system_info" (
    client_id BYTEA PRIMARY KEY,
    cpu_usage   SMALLINT,
    mem_usage   SMALLINT,
    disk_usage  SMALLINT,
    device_memsize BIGINT,
    device_count INTEGER DEFAULT 1,
    total_tflops INTEGER DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE  IF NOT EXISTS  "public"."device_info" (
    client_id BYTEA NOT NULL,
    device_index SMALLINT,
    device_name VARCHAR(255) DEFAULT NULL,
    device_id INTEGER,
    vendor_id INTEGER,
    device_memusage SMALLINT,
    device_gpuusage SMALLINT,
    device_powerusage SMALLINT,
    device_temp SMALLINT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (client_id, device_index)
);

CREATE TABLE  IF NOT EXISTS  client_models (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,      
    version VARCHAR(50) NOT NULL,    
    version_code BIGINT NOT NULL,    
    is_active BOOLEAN DEFAULT TRUE,  
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    engine_type SMALLINT NOT NULL DEFAULT 1,
    min_memory_mb INTEGER,           
    min_gpu_memory_gb INTEGER,       
    UNIQUE(name, version),
    UNIQUE(name, version_code),
    CONSTRAINT version_code_check CHECK (version_code > 0)
);
-- Add download_url, checksum, expected_size columns to client_models table
ALTER TABLE client_models 
ADD COLUMN IF NOT EXISTS download_url TEXT,
ADD COLUMN IF NOT EXISTS checksum VARCHAR(128),
ADD COLUMN IF NOT EXISTS expected_size BIGINT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_client_models_name_version_unique
ON client_models (name, version);

CREATE TABLE IF NOT EXISTS heartbeat (
  id SERIAL,
  client_id   BYTEA NOT NULL,
  cpu_usage   SMALLINT,
  mem_usage   SMALLINT,
  disk_usage  SMALLINT,
  network_up BIGINT NOT NULL DEFAULT 0,
  network_down BIGINT NOT NULL DEFAULT 0,
  timestamp   TIMESTAMPTZ NOT NULL,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (client_id, timestamp)
);

CREATE INDEX IF NOT EXISTS idx_heartbeat_client_id_timestamp 
ON heartbeat (client_id, timestamp DESC);

CREATE TABLE IF NOT EXISTS client_daily_stats (
    id BIGSERIAL PRIMARY KEY,
    date DATE NOT NULL,
    client_id BYTEA NOT NULL,
    total_heartbeats INTEGER NOT NULL DEFAULT 0,
    avg_cpu_usage FLOAT,
    avg_memory_usage FLOAT,
    avg_disk_usage FLOAT,
    total_network_in_bytes BIGINT DEFAULT 0,
    total_network_out_bytes BIGINT DEFAULT 0,
    last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_heartbeat_bucket BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (client_id, date)
);

ALTER TABLE client_daily_stats
ADD COLUMN IF NOT EXISTS last_heartbeat_bucket BIGINT NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_client_daily_stats_client_id_date 
ON client_daily_stats (client_id, date DESC);

CREATE TABLE IF NOT EXISTS device_daily_stats (
    id BIGSERIAL,
    date DATE NOT NULL,
    client_id BYTEA NOT NULL,
    device_index SMALLINT NOT NULL,                
    device_name VARCHAR(255),            
    total_heartbeats INTEGER NOT NULL DEFAULT 0,
    avg_utilization FLOAT,
    avg_temperature FLOAT,
    avg_power_usage FLOAT,
    avg_memory_usage FLOAT,
    last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_heartbeat_bucket BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (client_id, device_index, date)
);

ALTER TABLE device_daily_stats
ADD COLUMN IF NOT EXISTS last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW();

ALTER TABLE device_daily_stats
ADD COLUMN IF NOT EXISTS last_heartbeat_bucket BIGINT NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_device_daily_stats_date ON device_daily_stats (date);
CREATE INDEX IF NOT EXISTS idx_device_daily_stats_client_id ON device_daily_stats (client_id);
CREATE INDEX IF NOT EXISTS idx_device_daily_stats_device_index ON device_daily_stats (device_index);

CREATE TABLE IF NOT EXISTS heartbeat_config_daily (
    date DATE PRIMARY KEY,
    heartbeat_interval_secs INTEGER NOT NULL DEFAULT 120,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_heartbeat_config_daily_date ON heartbeat_config_daily (date);

CREATE TABLE IF NOT EXISTS device_types (
    device_id INTEGER PRIMARY KEY,
    device_name VARCHAR(255) NOT NULL,
    tflops DOUBLE PRECISION NOT NULL DEFAULT 0,
    points_multiplier NUMERIC(10,4) NOT NULL DEFAULT 1.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (device_name)
);

INSERT INTO device_types (device_id, device_name, tflops)
VALUES
    (1, 'Apple M1', 2.6),
    (2, 'Apple M1 Pro', 5.2),
    (3, 'Apple M1 Max', 10.4),
    (4, 'Apple M1 Ultra', 20.8),
    (5, 'Apple M2', 3.6),
    (6, 'Apple M2 Pro', 6.8),
    (7, 'Apple M2 Max', 13.6),
    (8, 'Apple M2 Ultra', 27.2),
    (9, 'Apple M3', 4.6),
    (10, 'Apple M3 Pro', 7.4),
    (11, 'Apple M3 Max', 14.8),
    (12, 'Apple M4 Max', 6.5),
    (13, 'Apple M4', 15.8),
    (5229, 'GeForce RTX 3080 20GB', 29.8),
    (5294, 'GeForce RTX 3070 16GB', 20.4),
    (8707, 'GeForce RTX 3090 Ti', 40.0),
    (8708, 'GeForce RTX 3090', 35.6),
    (8709, 'GeForce RTX 3080 Ti', 34.1),
    (8710, 'GeForce RTX 3080', 29.8),
    (8711, 'GeForce RTX 3070 Ti', 21.8),
    (8714, 'GeForce RTX 3080 12GB', 30.6),
    (8726, 'GeForce RTX 3080 Lite Hash Rate', 29.8),
    (8751, 'GeForce RTX 3080 11GB / 12GB Engineering Sample', 29.8),
    (9248, 'GeForce RTX 3080 Ti Mobile', 24.0),
    (9312, 'GeForce RTX 3080 Ti Laptop', 24.0),
    (9352, 'GeForce RTX 3070 Lite Hash Rate', 20.4),
    (9357, 'GeForce RTX 3070', 20.4),
    (9373, 'GeForce RTX 3070 Mobile', 16.6),
    (9376, 'GeForce RTX 3070 Laptop', 16.6),
    (9391, 'GeForce RTX 3070 Engineering Sample', 20.4),
    (9416, 'GeForce RTX 3070 GDDR6X', 20.4),
    (9860, 'GeForce RTX 4090', 82.6),
    (9865, 'GeForce RTX 4070 Ti SUPER', 40.0),
    (9879, 'GeForce RTX 4090 D', 73.5),
    (9986, 'GeForce RTX 4080 Super', 52.2),
    (9988, 'GeForce RTX 4080', 49.0),
    (9993, 'GeForce RTX 4070', 29.0),
    (10114, 'GeForce RTX 4070 Ti', 26.9),
    (10115, 'GeForce RTX 4070 SUPER', 36.0),
    (10120, 'GeForce RTX 4060 Ti', 22.1),
    (10144, 'GeForce RTX 4080 Max-Q / Mobile', 34.0),
    (10245, 'GeForce RTX 4060 Ti 16GB', 22.1),
    (10248, 'GeForce RTX 4060', 15.1),
    (10272, 'GeForce RTX 4070 Max-Q / Mobile', 20.0),
    (10400, 'GeForce RTX 4060 Max-Q / Mobile', 11.6),
    (11141, 'GeForce RTX 5090', 104.8),
    (11143, 'GeForce RTX 5090 D', 94.0),
    (11266, 'GeForce RTX 5080', 56.3),
    (11269, 'GeForce RTX 5070 Ti', 35.9),
    (11288, 'GeForce RTX 5090 Max-Q / Mobile', 75.0),
    (11289, 'GeForce RTX 5080 Max-Q / Mobile', 45.0),
    (11524, 'GeForce RTX 5060 Ti', 23.0),
    (11525, 'GeForce RTX 5060', 18.9),
    (11545, 'GeForce RTX 5060 Max-Q / Mobile', 14.0),
    (12036, 'GeForce RTX 5070', 30.7),
    (12056, 'GeForce RTX 5070 Ti Mobile', 28.0),
    (8954, 'H100 NVL', 1482),
    (8960, 'H100 PCIe', 1513),
    (8961, 'H100 SXM', 1680),
    (8965, 'H100 80GB HBM3', 1680),
    (8966, 'H100 80GB HBM3e', 1979),
    (20, 'A100 PCIe 40GB', 312),
    (21, 'A100 PCIe 80GB', 624),
    (22, 'A100 SXM 40GB', 312),
    (23, 'A100 SXM 80GB', 624),
    (24, 'A100 80GB HBM2e', 624),
    (25, 'NVIDIA A30', 165),
    (26, 'NVIDIA L40', 36.1),
    (27, 'NVIDIA L40S', 46.1),
    (28, 'NVIDIA L20', 29.4),
    (30, 'H200', 1979),
    (31, 'B100', 2500),
    (8349, 'A800 SXM4 40GB', 312),
    (8435, 'A800 SXM4 80GB', 624),
    (8437, 'A800 80GB PCIe', 624),
    (8438, 'A800 40GB PCIe', 312),
    (5510, 'AMD Ryzen AI Max+ 395', 16.0)
ON CONFLICT (device_id) DO UPDATE SET
    device_name = EXCLUDED.device_name,
    tflops = EXCLUDED.tflops,
    updated_at = NOW();

DO $$
BEGIN
    IF to_regclass('public.device_points_multiplier') IS NOT NULL THEN
        INSERT INTO device_types (device_id, device_name, tflops, points_multiplier)
        SELECT
            device_id,
            CONCAT('device_', device_id),
            0,
            MAX(multiplier)
        FROM device_points_multiplier
        GROUP BY device_id
        ON CONFLICT (device_id) DO UPDATE SET
            points_multiplier = EXCLUDED.points_multiplier,
            updated_at = NOW();

        DROP TABLE device_points_multiplier;
    END IF;
END
$$;

-- Device points daily migration and incremental refresh
-- See: scripts/device_points_daily_incremental.sql

-- Insert test data for client_models
-- engine_type: 1=Ollama, 2=Vllm, 3=TensorRT, 4=ONNX, 5=None, 6=Llama
-- expected_size unit: bytes
INSERT INTO client_models (name, version, version_code, is_active, created_at, engine_type, min_memory_mb, min_gpu_memory_gb, download_url, checksum, expected_size) 
VALUES 
    ('llama3.2:latest', 'latest', 12314, true, '2025-10-14 16:19:31.846481+08', 1, NULL, 8, NULL, NULL, NULL),
    ('TheBloke/gemma-3-12b-it-GPTQ', 'GPTQ', 12315, true, '2025-10-14 23:34:40.663107+08', 2, NULL, 8, NULL, NULL, NULL),
    ('facebook/opt-125m', 'latest', 12316, true, '2025-10-15 11:06:15.912991+08', 2, NULL, 8, NULL, NULL, NULL),
    ('Qwen3-32B-Q6_K.gguf', 'Q6_K', 12317, true, CURRENT_TIMESTAMP, 6, NULL, 24, 'https://modelscope.cn/models/Qwen/Qwen3-32B-GGUF/resolve/master/Qwen3-32B-Q6_K.gguf', 'c4c7c3cb6da260df1fe1d3cfd090a32dc7cc348f1278158be18e301f390d6f6e', 26369368064),
    ('Qwen3-14B-Q8_0.gguf', 'Q8_0', 12318, true, CURRENT_TIMESTAMP, 6, NULL, 16, 'https://modelscope.cn/models/Qwen/Qwen3-14B-GGUF/resolve/master/Qwen3-14B-Q8_0.gguf', 'a0dfe649137410b7d82f06a209240508e218f32f5b6fd81b69d6932160cfcd9d', 15698533728),
    ('Qwen3-8B-Q8_0.gguf', 'Q8_0', 12319, true, CURRENT_TIMESTAMP, 6, NULL, 12, 'https://modelscope.cn/models/Qwen/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q8_0.gguf', '408b955510e196121c1c375201744783b5c9a43c7956d73fc78df54c66e883d6', 8988692480)
ON CONFLICT (name, version) DO NOTHING;

-- Insert test data for tokens
DO $$
BEGIN
    IF to_regclass('public.users') IS NOT NULL THEN
        IF EXISTS (SELECT 1 FROM public.users WHERE id = 2)
           AND NOT EXISTS (
               SELECT 1 FROM tokens
               WHERE key = 'HSSb0OFrZon7wapKUduWqSxqpELMI62eTPyW017QanhnMyy4'
           )
        THEN
            INSERT INTO tokens (user_id, key, status, expired_time, deleted_at, access_level)
            VALUES (2, 'HSSb0OFrZon7wapKUduWqSxqpELMI62eTPyW017QanhnMyy4', 1, -1, NULL, 1);
        END IF;
    END IF;
END $$;

-- APK version management
CREATE TABLE IF NOT EXISTS "public"."apk_versions" (
    "id" BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    "package_name" VARCHAR(255) NOT NULL,
    "version_name" VARCHAR(64) NOT NULL,
    "version_code" BIGINT NOT NULL,
    "download_url" TEXT NOT NULL,
    "channel" VARCHAR(32) DEFAULT 'stable',
    "min_os_version" VARCHAR(32),
    "sha256" CHAR(64),
    "file_size_bytes" BIGINT,
    "is_active" BOOLEAN NOT NULL DEFAULT TRUE,
    "released_at" TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (package_name, version_code),
    CONSTRAINT apk_versions_version_code_check CHECK (version_code > 0)
);

CREATE INDEX IF NOT EXISTS idx_apk_versions_package_name
ON "public"."apk_versions" (package_name);

CREATE INDEX IF NOT EXISTS idx_apk_versions_package_active
ON "public"."apk_versions" (package_name, is_active);

CREATE INDEX IF NOT EXISTS idx_apk_versions_released_at
ON "public"."apk_versions" (released_at DESC);