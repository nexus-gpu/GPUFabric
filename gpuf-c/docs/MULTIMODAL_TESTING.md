# GPUFabric 多模态模型测试指南

## 📋 概述

GPUFabric 现在支持多模态视觉模型（如 SmolVLM），可以在 Android 真机上进行图像理解和视觉问答。

## 🎯 已准备的模型

您已经下载了以下模型文件：

- **文本模型**: `/home/jack/SmolVLM-500M-Instruct-Q8_0.gguf` (417 MB)
- **视觉投影器**: `/home/jack/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf` (104 MB)

## ✅ 当前支持状态

### 已实现的功能

1. **C API 多模态支持** ✅
   - `gpuf_load_multimodal_model()` - 加载文本模型和 mmproj
   - `gpuf_create_multimodal_context()` - 创建多模态上下文
   - `gpuf_generate_multimodal()` - 生成带图像输入的文本
   - `gpuf_multimodal_support_vision()` - 检查视觉支持
   - `gpuf_free_multimodal_model()` - 释放模型资源

2. **JNI Android 接口** ✅
   - `Java_com_gpuf_c_GPUEngine_loadMultimodalModel()` - 加载多模态模型
   - `Java_com_gpuf_c_GPUEngine_createMultimodalContext()` - 创建上下文
   - `Java_com_gpuf_c_GPUEngine_generateMultimodal()` - 多模态生成
   - `Java_com_gpuf_c_GPUEngine_supportsVision()` - 检查视觉支持
   - `Java_com_gpuf_c_GPUEngine_freeMultimodalModel()` - 释放资源

3. **libmtmd 库集成** ✅
   - llama.cpp 的多模态工具库已编译
   - `libmtmd.a` 已包含在 SDK 链接中 (9.1 MB)
   - 支持图像编码和视觉嵌入

4. **构建系统支持** ✅
   - `generate_sdk.sh` 已配置 `-DLLAMA_BUILD_MTMD=ON`
   - 自动复制 `libmtmd.a` 到 SDK
   - 链接脚本包含多模态库

## 🚀 Android 测试步骤

### 1. 编译 SDK

```bash
cd /home/jack/codedir/GPUFabric/gpuf-c
./generate_sdk.sh
```

这将生成包含多模态支持的 `libgpuf_c_sdk_v9.so`。

### 2. 推送模型到设备

```bash
# 推送文本模型
adb push /home/jack/SmolVLM-500M-Instruct-Q8_0.gguf /data/local/tmp/

# 推送视觉投影器
adb push /home/jack/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf /data/local/tmp/

# 推送 SDK
adb push /home/jack/codedir/GPUFabric/gpuf-c/libgpuf_c_sdk_v9.so /data/local/tmp/libgpuf_c.so
```

### 3. Java 测试代码示例

创建 `TestMultimodalEngine.java`:

```java
public class TestMultimodalEngine {
    static {
        System.loadLibrary("gpuf_c_sdk_v9");
    }

    // JNI 方法声明
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
        
        // 1. 加载多模态模型
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
        
        // 2. 检查视觉支持
        boolean hasVision = engine.supportsVision(modelPtr);
        System.out.println("Vision support: " + hasVision);
        
        // 3. 创建上下文
        System.out.println("Creating context...");
        long ctxPtr = engine.createMultimodalContext(modelPtr);
        if (ctxPtr == 0) {
            System.err.println("Failed to create context!");
            engine.freeMultimodalModel(modelPtr);
            return;
        }
        System.out.println("Context created: " + ctxPtr);
        
        // 4. 加载图像数据（示例：从文件读取）
        byte[] imageData = loadImageFile("/data/local/tmp/test_image.jpg");
        
        // 5. 生成响应
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
        
        // 6. 清理资源
        engine.freeMultimodalModel(modelPtr);
        System.out.println("Cleanup completed");
    }
    
    private static byte[] loadImageFile(String path) {
        // TODO: 实现图像文件加载
        // 返回 RGB 格式的图像数据
        return new byte[224 * 224 * 3]; // 示例占位符
    }
}
```

### 4. 编译和运行

```bash
# 编译 Java 代码
javac -h . TestMultimodalEngine.java

# 推送到设备
adb push TestMultimodalEngine.class /data/local/tmp/

# 在设备上运行
adb shell "cd /data/local/tmp && \
  LD_LIBRARY_PATH=. dalvikvm -cp . TestMultimodalEngine"
```

## 📝 C API 测试示例

创建 `test_multimodal.c`:

```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// 声明 C API 函数
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
    
    // 1. 加载模型
    void* model = gpuf_load_multimodal_model(
        "/data/local/tmp/SmolVLM-500M-Instruct-Q8_0.gguf",
        "/data/local/tmp/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf"
    );
    
    if (!model) {
        fprintf(stderr, "❌ Failed to load model\n");
        return 1;
    }
    printf("✅ Model loaded\n");
    
    // 2. 检查视觉支持
    int has_vision = gpuf_multimodal_support_vision(model);
    printf("Vision support: %s\n", has_vision ? "Yes" : "No");
    
    // 3. 创建上下文
    void* ctx = gpuf_create_multimodal_context(model);
    if (!ctx) {
        fprintf(stderr, "❌ Failed to create context\n");
        gpuf_free_multimodal_model(model);
        return 1;
    }
    printf("✅ Context created\n");
    
    // 4. 生成响应（纯文本测试）
    char output[4096] = {0};
    int result = gpuf_generate_multimodal(
        model,
        ctx,
        "Hello, how are you?",
        NULL,  // 无图像数据
        0,     // 图像大小为 0
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
    
    // 5. 清理
    gpuf_free_multimodal_model(model);
    printf("✅ Cleanup completed\n");
    
    return 0;
}
```

编译和运行：

```bash
# 使用 NDK 编译
$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang \
  test_multimodal.c -o test_multimodal \
  -L. -lgpuf_c_sdk_v9 -llog -ldl -lm

# 推送到设备
adb push test_multimodal /data/local/tmp/

# 运行
adb shell "cd /data/local/tmp && LD_LIBRARY_PATH=. ./test_multimodal"
```

## 🎨 图像格式要求

libmtmd 期望的图像格式：
- **格式**: RGB 原始数据
- **尺寸**: 通常 224x224（取决于模型）
- **数据类型**: `uint8_t` 数组
- **顺序**: 行优先，RGB 交错

### 图像预处理示例（Python）

```python
from PIL import Image
import numpy as np

def prepare_image(image_path, size=224):
    # 加载并调整大小
    img = Image.open(image_path).convert('RGB')
    img = img.resize((size, size))
    
    # 转换为 numpy 数组
    img_array = np.array(img, dtype=np.uint8)
    
    # 保存为原始字节
    img_array.tofile('image_data.bin')
    
    return img_array.tobytes()

# 使用
image_bytes = prepare_image('test_image.jpg')
```

## 🔍 调试技巧

### 1. 查看日志

```bash
adb logcat | grep -E "GPUFabric|mtmd|llama"
```

### 2. 检查库符号

```bash
nm -D libgpuf_c_sdk_v9.so | grep multimodal
```

应该看到：
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

### 3. 检查 libmtmd 符号

```bash
nm -D libgpuf_c_sdk_v9.so | grep mtmd
```

应该看到：
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

## ⚠️ 注意事项

1. **内存要求**: SmolVLM-500M 需要约 1GB RAM
2. **性能**: 首次加载可能需要 10-30 秒
3. **图像大小**: 建议使用 224x224 或更小的图像
4. **并发**: 当前不支持多个并发多模态请求

## 📊 预期性能

在 Android 设备上（ARM64）：
- **模型加载**: 10-30 秒
- **图像编码**: 1-3 秒
- **文本生成**: 2-5 tokens/秒（CPU）

## 🎯 下一步

1. ✅ **编译 SDK** - 运行 `./generate_sdk.sh`
2. ✅ **推送模型** - 使用 adb push 命令
3. ✅ **测试 C API** - 先测试纯文本生成
4. ✅ **测试图像输入** - 添加图像数据测试
5. ✅ **集成到应用** - 在 Android 应用中使用

## 📚 参考资料

- [llama.cpp 多模态文档](https://github.com/ggerganov/llama.cpp/tree/master/examples/llava)
- [SmolVLM 模型卡](https://huggingface.co/HuggingFaceTB/SmolVLM-500M-Instruct)
- [GPUFabric 构建指南](BUILD_GUIDE.md)
