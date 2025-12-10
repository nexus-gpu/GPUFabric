# GPUFabric Multimodal Model Testing Guide

## 📋 Overview

GPUFabric now supports multimodal vision models (such as SmolVLM), enabling image understanding and visual Q&A on Android real devices.

## 🎯 Prepared Models

You have already downloaded the following model files:

- **Text Model**: `/home/jack/SmolVLM-500M-Instruct-Q8_0.gguf` (417 MB)
- **Vision Projector**: `/home/jack/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf` (104 MB)

## ✅ Current Support Status

### Implemented Features

1. **C API Multimodal Support** ✅
   - `gpuf_load_multimodal_model()` - Load text model and mmproj
   - `gpuf_create_multimodal_context()` - Create multimodal context
   - `gpuf_generate_multimodal()` - Generate text with image input
   - `gpuf_multimodal_support_vision()` - Check vision support
   - `gpuf_free_multimodal_model()` - Free model resources

2. **JNI Android Interface** ✅
   - `Java_com_gpuf_c_GPUEngine_loadMultimodalModel()` - Load multimodal model
   - `Java_com_gpuf_c_GPUEngine_createMultimodalContext()` - Create context
   - `Java_com_gpuf_c_GPUEngine_generateMultimodal()` - Multimodal generation
   - `Java_com_gpuf_c_GPUEngine_supportsVision()` - Check vision support
   - `Java_com_gpuf_c_GPUEngine_freeMultimodalModel()` - Free resources

3. **libmtmd Library Integration** ✅
   - llama.cpp multimodal tool library compiled
   - `libmtmd.a` included in SDK linking (9.1 MB)
   - Supports image encoding and vision embedding

4. **Build System Support** ✅
   - `generate_sdk.sh` configured with `-DLLAMA_BUILD_MTMD=ON`
   - Automatically copies `libmtmd.a` to SDK
   - Linking script includes multimodal library

## 🚀 Android Testing Steps

### 1. Compile SDK

```bash
cd /home/jack/codedir/GPUFabric/gpuf-c
./generate_sdk.sh
```

This will generate `libgpuf_c_sdk_v9.so` with multimodal support.

### 2. Push Models to Device

```bash
# Push text model
adb push /home/jack/SmolVLM-500M-Instruct-Q8_0.gguf /data/local/tmp/

# Push vision projector
adb push /home/jack/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf /data/local/tmp/

# Push SDK
adb push /home/jack/codedir/GPUFabric/gpuf-c/libgpuf_c_sdk_v9.so /data/local/tmp/libgpuf_c.so
```

### 3. Java Test Code Example

Create `TestMultimodalEngine.java`:

```java
public class TestMultimodalEngine {
    static {
        System.loadLibrary("gpuf_c_sdk_v9");
    }

    // JNI method declarations
    public native long loadMultimodalModel(String textModelPath, String mmprojPath);
    public native long createMultimodalContext(long multimodalModelPtr);
    public native String generateMultimodal(
        long multimodalModelPtr,
        long ctxPtr,
        String textPrompt,
        byte[] imageData,
        int maxTokens,
        float temperature,
        int topK,
        float topP
    );
    public native boolean supportsVision(long multimodalModelPtr);
    public native void freeMultimodalModel(long multimodalModelPtr);

    public static void main(String[] args) {
        TestMultimodalEngine engine = new TestMultimodalEngine();
        
        // 1. Load multimodal model
        System.out.println("Loading multimodal model...");
        long modelPtr = engine.loadMultimodalModel(
            "/data/local/tmp/SmolVLM-500M-Instruct-Q8_0.gguf",
            "/data/local/tmp/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf"
        );
        
        if (modelPtr == 0) {
            System.err.println("Failed to load model!");
            return;
        }
        System.out.println("Model loaded: " + modelPtr);
        
        // 2. Check vision support
        boolean hasVision = engine.supportsVision(modelPtr);
        System.out.println("Vision support: " + hasVision);
        
        // 3. Create context
        System.out.println("Creating context...");
        long ctxPtr = engine.createMultimodalContext(modelPtr);
        if (ctxPtr == 0) {
            System.err.println("Failed to create context!");
            engine.freeMultimodalModel(modelPtr);
            return;
        }
        System.out.println("Context created: " + ctxPtr);
        
        // 4. Load image data (example: read from file)
        byte[] imageData = loadImageFile("/data/local/tmp/test_image.jpg");
        
        // 5. Generate response
        System.out.println("Generating response...");
        String response = engine.generateMultimodal(
            modelPtr,
            ctxPtr,
            "What do you see in this image?",
            imageData,
            100,    // max_tokens
            0.7f,   // temperature
            40,     // top_k
            0.9f    // top_p
        );
        
        System.out.println("Response: " + response);
        
        // 6. Cleanup resources
        engine.freeMultimodalModel(modelPtr);
        System.out.println("Cleanup completed");
    }
    
    private static byte[] loadImageFile(String path) {
        // TODO: Implement image file loading
        // Return RGB format image data
        return new byte[224 * 224 * 3]; // Example placeholder
    }
}
```

### 4. Compile and Run

```bash
# Compile Java code
javac -h . TestMultimodalEngine.java

# Push to device
adb push TestMultimodalEngine.class /data/local/tmp/

# Run on device
adb shell "cd /data/local/tmp && \
  LD_LIBRARY_PATH=. dalvikvm -cp . TestMultimodalEngine"
```

## 📝 C API Test Example

Create `test_multimodal.c`:

```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Declare C API functions
extern void* gpuf_load_multimodal_model(const char* text_model_path, const char* mmproj_path);
extern void* gpuf_create_multimodal_context(void* multimodal_model);
extern int gpuf_generate_multimodal(
    void* multimodal_model,
    void* ctx,
    const char* text_prompt,
    const unsigned char* image_data,
    unsigned long long image_size,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    char* output,
    int output_len
);
extern int gpuf_multimodal_support_vision(void* multimodal_model);
extern void gpuf_free_multimodal_model(void* multimodal_model);

int main() {
    printf("🔥 Testing GPUFabric Multimodal API\n");
    
    // 1. Load model
    void* model = gpuf_load_multimodal_model(
        "/data/local/tmp/SmolVLM-500M-Instruct-Q8_0.gguf",
        "/data/local/tmp/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf"
    );
    
    if (!model) {
        fprintf(stderr, "❌ Failed to load model\n");
        return 1;
    }
    printf("✅ Model loaded\n");
    
    // 2. Check vision support
    int has_vision = gpuf_multimodal_support_vision(model);
    printf("Vision support: %s\n", has_vision ? "Yes" : "No");
    
    // 3. Create context
    void* ctx = gpuf_create_multimodal_context(model);
    if (!ctx) {
        fprintf(stderr, "❌ Failed to create context\n");
        gpuf_free_multimodal_model(model);
        return 1;
    }
    printf("✅ Context created\n");
    
    // 4. Generate response (text-only test)
    char output[4096] = {0};
    int result = gpuf_generate_multimodal(
        model,
        ctx,
        "Hello, how are you?",
        NULL,  // No image data
        0,     // Image size is 0
        50,    // max_tokens
        0.7f,  // temperature
        40,    // top_k
        0.9f,  // top_p
        1.1f,  // repeat_penalty
        output,
        sizeof(output)
    );
    
    if (result > 0) {
        printf("✅ Generation successful\n");
        printf("Response: %s\n", output);
    } else {
        printf("❌ Generation failed: %d\n", result);
    }
    
    // 5. Cleanup
    gpuf_free_multimodal_model(model);
    printf("✅ Cleanup completed\n");
    
    return 0;
}
```

Compile and run:

```bash
# Compile with NDK
$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang \
  test_multimodal.c -o test_multimodal \
  -L. -lgpuf_c_sdk_v9 -llog -ldl -lm

# Push to device
adb push test_multimodal /data/local/tmp/

# Run
adb shell "cd /data/local/tmp && LD_LIBRARY_PATH=. ./test_multimodal"
```

## 🎨 Image Format Requirements

libmtmd expected image format:
- **Format**: RGB raw data
- **Size**: Usually 224x224 (depends on model)
- **Data type**: `uint8_t` array
- **Order**: Row-major, RGB interleaved

### Image Preprocessing Example (Python)

```python
from PIL import Image
import numpy as np

def prepare_image(image_path, size=224):
    # Load and resize
    img = Image.open(image_path).convert('RGB')
    img = img.resize((size, size))
    
    # Convert to numpy array
    img_array = np.array(img, dtype=np.uint8)
    
    # Save as raw bytes
    img_array.tofile('image_data.bin')
    
    return img_array.tobytes()

# Usage
image_bytes = prepare_image('test_image.jpg')
```

## 🔍 Debugging Tips

### 1. View Logs

```bash
adb logcat | grep -E "GPUFabric|mtmd|llama"
```

### 2. Check Library Symbols

```bash
nm -D libgpuf_c_sdk_v9.so | grep multimodal
```

Should see:
```
gpuf_load_multimodal_model
gpuf_create_multimodal_context
gpuf_generate_multimodal
gpuf_multimodal_support_vision
gpuf_free_multimodal_model
Java_com_gpuf_c_GPUEngine_loadMultimodalModel
Java_com_gpuf_c_GPUEngine_createMultimodalContext
Java_com_gpuf_c_GPUEngine_generateMultimodal
Java_com_gpuf_c_GPUEngine_supportsVision
Java_com_gpuf_c_GPUEngine_freeMultimodalModel
```

### 3. Check libmtmd Symbols

```bash
nm -D libgpuf_c_sdk_v9.so | grep mtmd
```

Should see:
```
mtmd_context_params_default
mtmd_init_from_file
mtmd_free
mtmd_support_vision
mtmd_bitmap_init
mtmd_bitmap_free
mtmd_input_chunks_init
mtmd_input_chunks_free
mtmd_tokenize
mtmd_encode_chunk
```

## ⚠️ Important Notes

1. **Memory Requirements**: SmolVLM-500M requires about 1GB RAM
2. **Performance**: First load may take 10-30 seconds
3. **Image Size**: Recommend using 224x224 or smaller images
4. **Concurrency**: Currently does not support multiple concurrent multimodal requests

## 📊 Expected Performance

On Android devices (ARM64):
- **Model Loading**: 10-30 seconds
- **Image Encoding**: 1-3 seconds
- **Text Generation**: 2-5 tokens/second (CPU)

## 🎯 Next Steps

1. ✅ **Compile SDK** - Run `./generate_sdk.sh`
2. ✅ **Push Models** - Use adb push commands
3. ✅ **Test C API** - Test text-only generation first
4. ✅ **Test Image Input** - Add image data testing
5. ✅ **Integrate into App** - Use in Android application

## 📚 References

- [llama.cpp Multimodal Documentation](https://github.com/ggerganov/llama.cpp/tree/master/examples/llava)
- [SmolVLM Model Card](https://huggingface.co/HuggingFaceTB/SmolVLM-500M-Instruct)
- [GPUFabric Build Guide](BUILD_GUIDE.md)
