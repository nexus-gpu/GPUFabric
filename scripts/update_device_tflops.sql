-- Update device_types with accurate TFLOPS values
-- Run this on your PostgreSQL database to update existing records
-- Test environment: postgres://postgres_test:pwd_postgres_aliyun_test1234@8.140.251.142:5432/aliyun_test

-- Apple devices
UPDATE device_types SET tflops = 2.6 WHERE device_id = 1;
UPDATE device_types SET tflops = 5.2 WHERE device_id = 2;
UPDATE device_types SET tflops = 10.4 WHERE device_id = 3;
UPDATE device_types SET tflops = 20.8 WHERE device_id = 4;
UPDATE device_types SET tflops = 3.6 WHERE device_id = 5;
UPDATE device_types SET tflops = 6.8 WHERE device_id = 6;
UPDATE device_types SET tflops = 13.6 WHERE device_id = 7;
UPDATE device_types SET tflops = 27.2 WHERE device_id = 8;
UPDATE device_types SET tflops = 4.6 WHERE device_id = 9;
UPDATE device_types SET tflops = 7.4 WHERE device_id = 10;
UPDATE device_types SET tflops = 14.8 WHERE device_id = 11;
UPDATE device_types SET tflops = 6.5 WHERE device_id = 12;
UPDATE device_types SET tflops = 15.8 WHERE device_id = 13;

-- NVIDIA GeForce RTX 30 series
UPDATE device_types SET tflops = 29.8, device_name = 'GeForce RTX 3080 20GB' WHERE device_id = 5229;
UPDATE device_types SET tflops = 20.4, device_name = 'GeForce RTX 3070 16GB' WHERE device_id = 5294;
UPDATE device_types SET tflops = 40.0, device_name = 'GeForce RTX 3090 Ti' WHERE device_id = 8707;
UPDATE device_types SET tflops = 35.6, device_name = 'GeForce RTX 3090' WHERE device_id = 8708;
UPDATE device_types SET tflops = 34.1, device_name = 'GeForce RTX 3080 Ti' WHERE device_id = 8709;
UPDATE device_types SET tflops = 29.8, device_name = 'GeForce RTX 3080' WHERE device_id = 8710;
UPDATE device_types SET tflops = 21.8, device_name = 'GeForce RTX 3070 Ti' WHERE device_id = 8711;
UPDATE device_types SET tflops = 30.6, device_name = 'GeForce RTX 3080 12GB' WHERE device_id = 8714;
UPDATE device_types SET tflops = 29.8, device_name = 'GeForce RTX 3080 Lite Hash Rate' WHERE device_id = 8726;
UPDATE device_types SET tflops = 29.8, device_name = 'GeForce RTX 3080 11GB / 12GB Engineering Sample' WHERE device_id = 8751;
UPDATE device_types SET tflops = 24.0, device_name = 'GeForce RTX 3080 Ti Mobile' WHERE device_id = 9248;
UPDATE device_types SET tflops = 24.0, device_name = 'GeForce RTX 3080 Ti Laptop' WHERE device_id = 9312;
UPDATE device_types SET tflops = 20.4, device_name = 'GeForce RTX 3070 Lite Hash Rate' WHERE device_id = 9352;
UPDATE device_types SET tflops = 20.4, device_name = 'GeForce RTX 3070' WHERE device_id = 9357;
UPDATE device_types SET tflops = 16.6, device_name = 'GeForce RTX 3070 Mobile' WHERE device_id = 9373;
UPDATE device_types SET tflops = 16.6, device_name = 'GeForce RTX 3070 Laptop' WHERE device_id = 9376;
UPDATE device_types SET tflops = 20.4, device_name = 'GeForce RTX 3070 Engineering Sample' WHERE device_id = 9391;
UPDATE device_types SET tflops = 20.4, device_name = 'GeForce RTX 3070 GDDR6X' WHERE device_id = 9416;

-- NVIDIA GeForce RTX 40 series
UPDATE device_types SET tflops = 82.6, device_name = 'GeForce RTX 4090' WHERE device_id = 9860;
UPDATE device_types SET tflops = 40.0, device_name = 'GeForce RTX 4070 Ti SUPER' WHERE device_id = 9865;
UPDATE device_types SET tflops = 73.5, device_name = 'GeForce RTX 4090 D' WHERE device_id = 9879;
UPDATE device_types SET tflops = 52.2, device_name = 'GeForce RTX 4080 Super' WHERE device_id = 9986;
UPDATE device_types SET tflops = 49.0, device_name = 'GeForce RTX 4080' WHERE device_id = 9988;
UPDATE device_types SET tflops = 29.0, device_name = 'GeForce RTX 4070' WHERE device_id = 9993;
UPDATE device_types SET tflops = 26.9, device_name = 'GeForce RTX 4070 Ti' WHERE device_id = 10114;
UPDATE device_types SET tflops = 36.0, device_name = 'GeForce RTX 4070 SUPER' WHERE device_id = 10115;
UPDATE device_types SET tflops = 22.1, device_name = 'GeForce RTX 4060 Ti' WHERE device_id = 10120;
UPDATE device_types SET tflops = 34.0, device_name = 'GeForce RTX 4080 Max-Q / Mobile' WHERE device_id = 10144;
UPDATE device_types SET tflops = 22.1, device_name = 'GeForce RTX 4060 Ti 16GB' WHERE device_id = 10245;
UPDATE device_types SET tflops = 15.1, device_name = 'GeForce RTX 4060' WHERE device_id = 10248;
UPDATE device_types SET tflops = 20.0, device_name = 'GeForce RTX 4070 Max-Q / Mobile' WHERE device_id = 10272;
UPDATE device_types SET tflops = 11.6, device_name = 'GeForce RTX 4060 Max-Q / Mobile' WHERE device_id = 10400;

-- NVIDIA GeForce RTX 50 series
UPDATE device_types SET tflops = 104.8, device_name = 'GeForce RTX 5090' WHERE device_id = 11141;
UPDATE device_types SET tflops = 94.0, device_name = 'GeForce RTX 5090 D' WHERE device_id = 11143;
UPDATE device_types SET tflops = 56.3, device_name = 'GeForce RTX 5080' WHERE device_id = 11266;
UPDATE device_types SET tflops = 35.9, device_name = 'GeForce RTX 5070 Ti' WHERE device_id = 11269;
UPDATE device_types SET tflops = 75.0, device_name = 'GeForce RTX 5090 Max-Q / Mobile' WHERE device_id = 11288;
UPDATE device_types SET tflops = 45.0, device_name = 'GeForce RTX 5080 Max-Q / Mobile' WHERE device_id = 11289;
UPDATE device_types SET tflops = 23.0, device_name = 'GeForce RTX 5060 Ti' WHERE device_id = 11524;
UPDATE device_types SET tflops = 18.9, device_name = 'GeForce RTX 5060' WHERE device_id = 11525;
UPDATE device_types SET tflops = 14.0, device_name = 'GeForce RTX 5060 Max-Q / Mobile' WHERE device_id = 11545;
UPDATE device_types SET tflops = 30.7, device_name = 'GeForce RTX 5070' WHERE device_id = 12036;
UPDATE device_types SET tflops = 28.0, device_name = 'GeForce RTX 5070 Ti Mobile' WHERE device_id = 12056;

-- NVIDIA Data Center
UPDATE device_types SET tflops = 165, device_name = 'NVIDIA A30' WHERE device_id = 25;
UPDATE device_types SET tflops = 36.1, device_name = 'NVIDIA L40' WHERE device_id = 26;
UPDATE device_types SET tflops = 46.1, device_name = 'NVIDIA L40S' WHERE device_id = 27;
UPDATE device_types SET tflops = 29.4, device_name = 'NVIDIA L20' WHERE device_id = 28;
UPDATE device_types SET tflops = 1482, device_name = 'H100 NVL' WHERE device_id = 8954;
UPDATE device_types SET tflops = 1513, device_name = 'H100 PCIe' WHERE device_id = 8960;
UPDATE device_types SET tflops = 1680, device_name = 'H100 SXM' WHERE device_id = 8961;
UPDATE device_types SET tflops = 1680, device_name = 'H100 80GB HBM3' WHERE device_id = 8965;
UPDATE device_types SET tflops = 1979, device_name = 'H100 80GB HBM3e' WHERE device_id = 8966;

-- A800 (new devices)
INSERT INTO device_types (device_id, device_name, tflops, points_multiplier, created_at, updated_at)
VALUES 
    (8349, 'A800 SXM4 40GB', 312, 0.05, NOW(), NOW()),
    (8435, 'A800 SXM4 80GB', 624, 0.05, NOW(), NOW()),
    (8437, 'A800 80GB PCIe', 624, 0.05, NOW(), NOW()),
    (8438, 'A800 40GB PCIe', 312, 0.05, NOW(), NOW())
ON CONFLICT (device_id) DO UPDATE SET
    device_name = EXCLUDED.device_name,
    tflops = EXCLUDED.tflops,
    points_multiplier = EXCLUDED.points_multiplier,
    updated_at = NOW();

-- AMD (new device)
INSERT INTO device_types (device_id, device_name, tflops, points_multiplier, created_at, updated_at)
VALUES 
    (5510, 'AMD Ryzen AI Max+ 395', 16.0, 0.05, NOW(), NOW())
ON CONFLICT (device_id) DO UPDATE SET
    device_name = EXCLUDED.device_name,
    tflops = EXCLUDED.tflops,
    points_multiplier = EXCLUDED.points_multiplier,
    updated_at = NOW();

-- Set all device types points_multiplier to 0.05 (uniform policy)
UPDATE device_types
SET
    points_multiplier = 0.05,
    updated_at = NOW();

-- Refresh device_points_daily table to apply changes
SELECT refresh_device_points_daily();

-- Verify updates
SELECT device_id, device_name, tflops, points_multiplier 
FROM device_types 
WHERE tflops > 0 
ORDER BY tflops DESC;
