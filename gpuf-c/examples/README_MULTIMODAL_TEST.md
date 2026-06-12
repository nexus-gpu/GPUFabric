# Android 多模态测试快速指南

## 📦 文件说明

- **test_multimodal_android.c** - 完整的 C 语言测试程序
- **build_and_test_multimodal.sh** - 自动化构建和测试脚本

## 🚀 快速开始

### 方法 1: 使用自动化脚本（推荐）

```bash
cd <repo>/gpuf-c/examples
./build_and_test_multimodal.sh
```

这个脚本会自动：
1. ✅ 检查 NDK 和 SDK 是否存在
2. ✅ 编译测试程序
3. ✅ 推送文件到 Android 设备
4. ✅ 运行测试
5. ✅ 收集日志

### 方法 2: 手动步骤

#### 1. 编译测试程序

```bash
cd <repo>/gpuf-c/examples

$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang \
  test_multimodal_android.c \
  -o test_multimodal_android \
  -L../. \
  -lgpuf_c_sdk_v9 \
  -llog -ldl -lm \
  -pie
```

#### 2. 推送文件到设备

```bash
# 推送 SDK 库
adb push ../libgpuf_c_sdk_v9.so /data/local/tmp/

# 推送测试程序
adb push test_multimodal_android /data/local/tmp/
adb shell chmod +x /data/local/tmp/test_multimodal_android

# 推送模型文件
adb push "$HOME/SmolVLM-500M-Instruct-Q8_0.gguf" /data/local/tmp/
adb push "$HOME/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf" /data/local/tmp/
```

#### 3. 运行测试

```bash
adb shell "cd /data/local/tmp && LD_LIBRARY_PATH=. ./test_multimodal_android"
```

## 🧪 测试内容

测试程序包含 6 个测试用例：

### Test 1: 加载多模态模型
- 加载 SmolVLM 文本模型
- 加载 mmproj 视觉投影器
- 测量加载时间

### Test 2: 检查视觉支持
- 验证模型是否支持视觉输入
- 确认 libmtmd 正常工作

### Test 3: 创建推理上下文
- 创建 llama_context
- 测量上下文创建时间

### Test 4: 纯文本生成
- 测试不带图像的文本生成
- 验证基础推理功能
- 测量生成速度（tokens/sec）

### Test 5: 多模态生成（文本+图像）
- 使用虚拟图像数据测试
- 验证图像编码功能
- 测试完整的视觉-语言推理

### Test 6: 多次查询
- 连续执行多个推理请求
- 测试稳定性和一致性

## 📊 预期输出

```
╔════════════════════════════════════════════════════════════╗
║  GPUFabric Multimodal Test for Android                    ║
║  SmolVLM-500M Vision-Language Model                        ║
╚════════════════════════════════════════════════════════════╝

========================================
  Initializing GPUFabric Backend
========================================
✅ Backend initialized

========================================
  Test 1: Loading Multimodal Model
========================================
Text model: /data/local/tmp/SmolVLM-500M-Instruct-Q8_0.gguf
MMProj: /data/local/tmp/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf
Model loaded in 15234 ms
✅ Multimodal model loaded successfully

========================================
  Test 2: Checking Vision Support
========================================
✅ Model supports vision input

========================================
  Test 3: Creating Inference Context
========================================
Context created in 523 ms
✅ Inference context created successfully

========================================
  Test 4: Text-Only Generation
========================================
Prompt: "Hello! Please introduce yourself."
ℹ️  Generating response (text-only)...

--- Response ---
Hello! I'm SmolVLM, a vision-language AI assistant...
--- End ---
Generated 45 tokens in 9234 ms
Speed: 4.87 tokens/sec
✅ Text generation successful

========================================
  Test 5: Multimodal Generation (Text + Image)
========================================
ℹ️  Created dummy 224x224 RGB test image
Image size: 150528 bytes
Prompt: "What do you see in this image? Describe it in detail."
ℹ️  Generating response with image input...

--- Response ---
I see a colorful gradient pattern with red, green, and blue colors...
--- End ---
Generated 67 tokens in 15432 ms
Speed: 4.34 tokens/sec
✅ Multimodal generation successful

========================================
  Test 6: Multiple Queries
========================================

[Query 1/3] What is 2+2?
Response: 2+2 equals 4.

[Query 2/3] Tell me a short joke.
Response: Why did the chicken cross the road? To get to the other side!

[Query 3/3] What is the capital of France?
Response: The capital of France is Paris.

Success rate: 3/3
✅ All queries completed successfully

========================================
  Cleanup
========================================
✅ Model freed
✅ Backend cleaned up

╔════════════════════════════════════════════════════════════╗
║  ✅ ALL TESTS PASSED                                      ║
╚════════════════════════════════════════════════════════════╝
```

## 🔧 自定义测试

### 使用真实图像

1. 准备 RGB 格式的图像数据（224x224）：

```python
from PIL import Image
import numpy as np

img = Image.open('your_image.jpg').convert('RGB')
img = img.resize((224, 224))
img_array = np.array(img, dtype=np.uint8)
img_array.tofile('test_image.rgb')
```

2. 推送到设备：

```bash
adb push test_image.rgb /data/local/tmp/
```

3. 测试程序会自动检测并使用该图像

### 修改测试参数

编辑 `test_multimodal_android.c` 中的参数：

```c
// 生成参数
max_tokens = 100;      // 最大生成 token 数
temperature = 0.7f;    // 温度（0.0-1.0）
top_k = 40;           // Top-K 采样
top_p = 0.9f;         // Top-P 采样
repeat_penalty = 1.1f; // 重复惩罚
```

## 📝 日志和调试

### 查看实时日志

```bash
# 在另一个终端运行
adb logcat | grep -E "GPUFabric|llama|mtmd|ggml"
```

### 保存日志到文件

```bash
adb logcat -d | grep -E "GPUFabric|llama|mtmd" > multimodal_test.log
```

### 检查库符号

```bash
# 检查多模态函数
adb shell "cd /data/local/tmp && nm -D libgpuf_c_sdk_v9.so | grep multimodal"

# 检查 libmtmd 函数
adb shell "cd /data/local/tmp && nm -D libgpuf_c_sdk_v9.so | grep mtmd"
```

## ⚠️ 常见问题

### 1. 模型加载失败

**问题**: `Failed to load multimodal model`

**解决方案**:
- 检查模型文件是否存在：`adb shell ls -lh /data/local/tmp/*.gguf`
- 检查文件权限：`adb shell chmod 644 /data/local/tmp/*.gguf`
- 确保有足够的存储空间：`adb shell df -h /data/local/tmp`

### 2. 库加载失败

**问题**: `error while loading shared libraries`

**解决方案**:
```bash
# 确保设置了 LD_LIBRARY_PATH
adb shell "cd /data/local/tmp && LD_LIBRARY_PATH=. ./test_multimodal_android"

# 或者使用绝对路径
adb shell "LD_LIBRARY_PATH=/data/local/tmp /data/local/tmp/test_multimodal_android"
```

### 3. 内存不足

**问题**: `Out of memory` 或模型加载卡住

**解决方案**:
- 关闭其他应用释放内存
- 使用更小的模型（Q4 量化版本）
- 检查设备可用内存：`adb shell cat /proc/meminfo`

### 4. 生成速度慢

**问题**: 生成速度 < 1 token/sec

**解决方案**:
- 这是正常的 CPU 推理速度
- SmolVLM-500M 在 ARM CPU 上预期速度为 2-5 tokens/sec
- 考虑使用更小的模型或更快的设备

## 📈 性能基准

| 设备类型 | CPU | 加载时间 | 生成速度 |
|---------|-----|---------|---------|
| 高端手机 | Snapdragon 8 Gen 2 | 10-15s | 4-6 t/s |
| 中端手机 | Snapdragon 778G | 20-30s | 2-4 t/s |
| 低端手机 | Snapdragon 665 | 40-60s | 1-2 t/s |

## 🎯 下一步

1. ✅ 运行基础测试确认功能正常
2. ✅ 使用真实图像测试视觉理解
3. ✅ 集成到 Android 应用中
4. ✅ 优化性能和内存使用
5. ✅ 添加更多测试用例

## 📚 相关文档

- [多模态测试指南](../docs/MULTIMODAL_TESTING.md)
- [构建指南](../docs/BUILD_GUIDE.md)
- [API 文档](../docs/API_REFERENCE.md)
