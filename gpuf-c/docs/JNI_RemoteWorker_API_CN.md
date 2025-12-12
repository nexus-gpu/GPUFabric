# GPUFabric Remote Worker JNI API 文档

## 概述

本文档描述了 GPUFabric Remote Worker 的 JNI (Java Native Interface) API，用于在 Android 应用中集成分布式 LLM 推理功能。这些 API 允许 Android 设备作为远程工作节点，连接到 GPUFabric 服务器并执行 LLM 推理任务。

**源文件**: `/home/jack/codedir/GPUFabric/gpuf-c/src/jni_remote_worker.rs`

**Java 包名**: `com.gpuf.c.RemoteWorker`

**Native 库**: `libgpuf_c_sdk_v9.so`

---

## API 列表

### 1. setRemoteWorkerModel

**功能**: 设置或热切换 LLM 模型

**描述**: 加载指定路径的 GGUF 模型文件。支持热切换（hot swapping），可以在工作器运行时更换模型而无需重启连接。

**Java 方法签名**:
```java
public static native int setRemoteWorkerModel(String modelPath);
```

**参数**:
| 参数名 | 类型 | 说明 |
|--------|------|------|
| `modelPath` | String | GGUF 模型文件的完整路径<br>例如: `/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf` |

**返回值**:
- `0`: 成功加载模型
- `-1`: 失败（详细错误信息会输出到日志）

**使用场景**:
- 初始化时加载模型
- 运行时切换不同的模型
- 更新模型版本

**示例**:
```java
String modelPath = "/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf";
int result = RemoteWorker.setRemoteWorkerModel(modelPath);
if (result == 0) {
    Log.i("GPUFabric", "模型加载成功");
} else {
    Log.e("GPUFabric", "模型加载失败");
}
```

---

### 2. startRemoteWorker

**功能**: 启动远程工作器并连接到 GPUFabric 服务器

**描述**: 建立与 GPUFabric 服务器的网络连接，注册设备为可用的推理节点。必须在调用 `startRemoteWorkerTasks()` 之前调用。

**Java 方法签名**:
```java
public static native int startRemoteWorker(
    String serverAddr,
    int controlPort,
    int proxyPort,
    String workerType,
    String clientId
);
```

**参数**:
| 参数名 | 类型 | 说明 |
|--------|------|------|
| `serverAddr` | String | 服务器 IP 地址或主机名<br>例如: `"8.140.251.142"` |
| `controlPort` | int | 控制端口号<br>例如: `17000` |
| `proxyPort` | int | 代理端口号<br>例如: `17001` |
| `workerType` | String | 工作器类型<br>可选值: `"TCP"` 或 `"WS"` (WebSocket) |
| `clientId` | String | 客户端唯一标识符（32位十六进制字符）<br>例如: `"50ef7b5e7b5b4c79991087bb9f62cef1"` |

**返回值**:
- `0`: 成功连接到服务器
- `-1`: 连接失败（详细错误信息会输出到日志）

**注意事项**:
- `clientId` 必须是32个十六进制字符（128位）
- 确保网络权限已授予
- 服务器地址和端口必须可访问

**示例**:
```java
int result = RemoteWorker.startRemoteWorker(
    "8.140.251.142",  // 服务器地址
    17000,            // 控制端口
    17001,            // 代理端口
    "TCP",            // 连接类型
    "50ef7b5e7b5b4c79991087bb9f62cef1"  // 客户端ID
);
if (result == 0) {
    Log.i("GPUFabric", "远程工作器启动成功");
} else {
    Log.e("GPUFabric", "远程工作器启动失败");
}
```

---

### 3. startRemoteWorkerTasks

**功能**: 启动后台任务处理线程

**描述**: 启动心跳线程和推理任务处理线程。必须在 `startRemoteWorker()` 成功后调用。

**Java 方法签名**:
```java
public static native int startRemoteWorkerTasks();
```

**参数**: 无

**返回值**:
- `0`: 成功启动后台任务
- `-1`: 启动失败（详细错误信息会输出到日志）

**功能说明**:
- 启动心跳线程：定期向服务器发送设备状态（CPU、内存、磁盘使用率）
- 启动任务处理线程：监听并处理来自服务器的推理请求

**示例**:
```java
int result = RemoteWorker.startRemoteWorkerTasks();
if (result == 0) {
    Log.i("GPUFabric", "后台任务启动成功");
} else {
    Log.e("GPUFabric", "后台任务启动失败");
}
```

---

### 4. getRemoteWorkerStatus

**功能**: 获取远程工作器当前状态

**描述**: 查询工作器的运行状态，包括连接状态、模型信息等。

**Java 方法签名**:
```java
public static native String getRemoteWorkerStatus();
```

**参数**: 无

**返回值**:
- 成功: 返回状态字符串（例如: `"Worker is running"`）
- 失败: 返回 `null`

**状态信息可能包含**:
- 工作器运行状态
- 当前加载的模型
- 连接状态
- 系统资源使用情况

**示例**:
```java
String status = RemoteWorker.getRemoteWorkerStatus();
if (status != null) {
    Log.i("GPUFabric", "工作器状态: " + status);
} else {
    Log.e("GPUFabric", "获取状态失败");
}
```

---

### 5. stopRemoteWorker

**功能**: 停止远程工作器并清理资源

**描述**: 断开与服务器的连接，停止所有后台线程，释放模型和上下文资源。

**Java 方法签名**:
```java
public static native int stopRemoteWorker();
```

**参数**: 无

**返回值**:
- `0`: 成功停止工作器
- `-1`: 停止失败（详细错误信息会输出到日志）

**清理内容**:
- 关闭网络连接
- 停止心跳线程
- 停止任务处理线程
- 释放 LLM 模型内存
- 清理上下文缓存

**示例**:
```java
int result = RemoteWorker.stopRemoteWorker();
if (result == 0) {
    Log.i("GPUFabric", "工作器已停止");
} else {
    Log.e("GPUFabric", "停止工作器失败");
}
```

---

## 完整使用流程

### 基本流程

```java
// 1. 加载 Native 库
static {
    System.loadLibrary("gpuf_c_sdk_v9");
}

// 2. 设置模型
String modelPath = "/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf";
int result = RemoteWorker.setRemoteWorkerModel(modelPath);
if (result != 0) {
    Log.e("GPUFabric", "模型加载失败");
    return;
}

// 3. 启动远程工作器
result = RemoteWorker.startRemoteWorker(
    "8.140.251.142",
    17000,
    17001,
    "TCP",
    "50ef7b5e7b5b4c79991087bb9f62cef1"
);
if (result != 0) {
    Log.e("GPUFabric", "工作器启动失败");
    return;
}

// 4. 启动后台任务
result = RemoteWorker.startRemoteWorkerTasks();
if (result != 0) {
    Log.e("GPUFabric", "后台任务启动失败");
    return;
}

// 5. 监控状态（可选）
new Thread(() -> {
    while (true) {
        String status = RemoteWorker.getRemoteWorkerStatus();
        Log.i("GPUFabric", "状态: " + status);
        Thread.sleep(30000); // 每30秒检查一次
    }
}).start();

// 6. 热切换模型（可选）
String newModelPath = "/data/local/tmp/models/another-model.gguf";
result = RemoteWorker.setRemoteWorkerModel(newModelPath);

// 7. 停止工作器
RemoteWorker.stopRemoteWorker();
```

---

## 错误处理

所有 API 调用都应该检查返回值并处理错误：

```java
int result = RemoteWorker.startRemoteWorker(...);
if (result != 0) {
    // 检查 logcat 获取详细错误信息
    // adb logcat | grep "GPUFabric\|JNI"
    Log.e("GPUFabric", "操作失败，返回码: " + result);
    
    // 可能的错误原因：
    // - 网络连接问题
    // - 服务器不可达
    // - 参数格式错误
    // - 模型文件不存在
    // - 内存不足
}
```

---

## 日志输出

所有 JNI 函数都会输出详细的日志信息，可以通过 logcat 查看：

```bash
# 查看所有 GPUFabric 相关日志
adb logcat | grep "GPUFabric\|JNI"

# 查看特定标签
adb logcat -s "GPUFabric"
```

**日志标记**:
- `🔥` - JNI 函数调用
- `✅` - 操作成功
- `❌` - 操作失败
- `📂` - 文件路径
- `📡` - 网络连接
- `📊` - 状态信息

---

## 性能考虑

### 模型加载
- 首次加载模型需要较长时间（取决于模型大小）
- 热切换模型会短暂阻塞推理请求（通常 < 1秒）
- 建议在应用启动时预加载模型

### 网络连接
- TCP 连接延迟较低，适合局域网
- WebSocket 连接适合需要穿透防火墙的场景
- 心跳间隔默认为 30 秒

### 内存使用
- 模型会占用大量内存（1B 模型约 1-2GB）
- 确保设备有足够的可用内存
- 停止工作器会释放所有模型内存

---

## 线程安全

- 所有 API 都是线程安全的
- 可以从任何线程调用
- 内部使用互斥锁保护共享资源
- 建议在后台线程中调用耗时操作（如模型加载）

---

## 权限要求

Android 应用需要以下权限：

```xml
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
<uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" />
<uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE" />
```

---

## 故障排查

### 模型加载失败
- 检查文件路径是否正确
- 确认文件存在且可读
- 验证 GGUF 格式是否正确
- 检查内存是否充足

### 连接失败
- 验证服务器地址和端口
- 检查网络连接
- 确认防火墙设置
- 验证 clientId 格式（32个十六进制字符）

### 推理无响应
- 检查后台任务是否启动
- 查看 logcat 日志
- 验证模型是否正确加载
- 检查服务器是否正常运行

---

## 版本信息

- **SDK 版本**: v9.0.0
- **支持的 Android 版本**: API 21+ (Android 5.0+)
- **支持的架构**: ARM64 (aarch64)
- **llama.cpp 版本**: 最新稳定版

---

## 相关文档

- [C API 文档](./C_API_Reference.md)
- [服务器配置指南](./Server_Configuration.md)
- [性能优化指南](./Performance_Tuning.md)
- [示例代码](../examples/android_test.c)

---

## 技术支持

如有问题，请查看：
- GitHub Issues: https://github.com/your-repo/GPUFabric
- 文档: https://your-docs-site.com
- 邮件: support@gpufabric.com
