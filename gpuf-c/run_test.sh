#!/system/bin/sh
echo "🔥 Starting GPUFabric Android Test..."
echo "======================================="

# Set library path
export LD_LIBRARY_PATH=/data/local/tmp

# Check if files exist
if [ ! -f "/data/local/tmp/android_test" ]; then
    echo "❌ android_test not found"
    exit 1
fi

if [ ! -f "/data/local/tmp/libgpuf_c_sdk_v9.so" ]; then
    echo "❌ libgpuf_c_sdk_v9.so not found"
    exit 1
fi

if [ ! -f "/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf" ]; then
    echo "❌ Model file not found"
    exit 1
fi

echo "✅ All files found"
echo "🚀 Running test..."
cd /data/local/tmp
./android_test

echo "🎉 Test completed"
