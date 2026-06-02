-- Apple M Series Device Configuration
-- Based on FP32 GPU performance data, referencing io.net tiering standards

-- Insert Apple M series device types
INSERT INTO device_types (device_id, device_name, tflops, points_multiplier, updated_at) VALUES
-- Apple M Series - Community Pool
(9001, 'Apple M4 (GPU)', 2.9, 0.6, NOW()),
(9002, 'Apple M3 (GPU)', 2.47, 0.5, NOW()),
(9003, 'Apple M2 (GPU)', 2.24, 0.4, NOW()),
(9004, 'Apple M1 (GPU)', 1.36, 0.3, NOW())
ON CONFLICT (device_id) DO UPDATE SET
    device_name = EXCLUDED.device_name,
    tflops = EXCLUDED.tflops,
    points_multiplier = EXCLUDED.points_multiplier,
    updated_at = NOW();

-- Verify insertion results
SELECT
    device_id,
    device_name,
    tflops,
    points_multiplier,
    CASE
        WHEN points_multiplier >= 1.0 THEN '🟢 Enterprise Pool'
        WHEN points_multiplier >= 0.3 THEN '🟡 Community Pool'
        ELSE '🔴 Not Supported'
    END as pool_category
FROM device_types
WHERE device_id BETWEEN 9001 AND 9004
ORDER BY device_id;
