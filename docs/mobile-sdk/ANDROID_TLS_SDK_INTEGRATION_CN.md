# GPUFabric Android SDK TLS 接入指南

更新时间: 2026-07-02

本文面向 Android App / JNI 接入方，说明新版 GPUFabric Android SDK 的 Remote Worker TLS control stream 接入方式。TLS 能力是新增接口，旧的明文 `startRemoteWorker(...)` / `start_remote_worker(...)` 仍保持兼容。

## 交付物

Android SDK 包建议包含:

```text
gpufabric-android-sdk-v<version>/
  libs/
    libgpuf_c_sdk_v9.so
  include/
    gpuf_c.h
  docs/
    ANDROID_TLS_SDK_INTEGRATION_CN.md
  examples/
    test_jni_symbols
    test_jni_symbols.c
  build.sh
  README.md
  VERSION
  SHA256SUMS
```

如果测试环境使用自签 CA，测试包可以额外提供:

```text
certs/
  control-ca.pem
```

不要把服务端私钥、生产 token、生产 client id 或数据库连接信息放进 SDK 包。

## Android 工程集成

1. 将 `libs/libgpuf_c_sdk_v9.so` 放到 App 的 `src/main/jniLibs/arm64-v8a/`。
2. 将 CA bundle 放入 `assets/` 或通过后端下发到 App 私有目录。
3. App 启动时加载动态库:

```kotlin
object GPUFabricNative {
    init {
        System.loadLibrary("gpuf_c_sdk_v9")
    }
}
```

### C++ Runtime / React Native / fbjni 注意事项

GPUFabric Android SDK 不交付 `libc++_shared.so`，也不要从 GPUFabric SDK 目录向 App 的 `jniLibs` 复制 `libc++_shared.so`。

原因是 `libgpuf_c_sdk_v9.so` 当前没有声明 `libc++_shared.so` 依赖；如果 App 同时使用 React Native/fbjni，APK 合包阶段只能保留某一个 ABI 下的 `libc++_shared.so`。错误版本的 C++ runtime 可能覆盖 React Native/fbjni 需要的版本，导致 `dlopen libfbjni.so` 时出现 `basic_stringstream` 等 C++ 符号缺失。

建议:

1. 只从 GPUFabric SDK 拷贝 `libs/libgpuf_c_sdk_v9.so`。
2. 由宿主 App / React Native / fbjni 依赖链提供它们自己的 `libc++_shared.so`。
3. 如果工程里有 `packagingOptions { pickFirst "**/libc++_shared.so" }`，确认最终 APK 选中的不是旧 NDK 或 GPUFabric SDK 目录中的 runtime。
4. 修改后执行一次 clean build，避免 Gradle 复用旧的 merged native libs。

排查最终 APK 中的 C++ runtime:

```bash
unzip -l app-release.apk | grep 'lib/arm64-v8a/libc++_shared.so'
unzip -p app-release.apk lib/arm64-v8a/libc++_shared.so > /tmp/apk-libc++_shared.so
llvm-nm -D /tmp/apk-libc++_shared.so | grep basic_stringstream
readelf -d libfbjni.so | grep NEEDED
```

## JNI 接口

Remote Worker 相关 JNI 类名为 `com.gpuf.c.RemoteWorker`。TLS 相关新增接口:

```java
public final class RemoteWorker {
    public static native int validateMobileTlsPolicy(
        String caCertPath,
        String serverName,
        String certSha256Pin
    );

    public static native int startRemoteWorkerWithTls(
        String serverAddr,
        int controlPort,
        int proxyPort,
        String workerType,
        String clientId,
        String caCertPath,
        String controlTlsServerName,
        String certSha256Pin
    );

    public static native int startRemoteWorkerTasks(long callbackFunctionPtr);
    public static native String getRemoteWorkerStatus();
    public static native int stopRemoteWorker();
}
```

旧接口仍可用:

```java
public static native int startRemoteWorker(
    String serverAddr,
    int controlPort,
    int proxyPort,
    String workerType,
    String clientId
);
```

## TLS 调用顺序

推荐顺序:

```text
1. 将 CA bundle 复制到 App 私有目录，得到绝对路径 caCertPath
2. validateMobileTlsPolicy(caCertPath, controlTlsServerName, certSha256Pin)
3. startRemoteWorkerWithTls(...)
4. startRemoteWorkerTasks(...)
5. getRemoteWorkerStatus()
6. stopRemoteWorker()
```

最小 Kotlin 示例:

```kotlin
val rcPolicy = RemoteWorker.validateMobileTlsPolicy(
    caCertPath,
    controlTlsServerName,
    ""
)
require(rcPolicy == 0) { "invalid TLS policy: $rcPolicy" }

val rcStart = RemoteWorker.startRemoteWorkerWithTls(
    serverAddr,
    controlPort,
    proxyPort,
    "TCP",
    clientId,
    caCertPath,
    controlTlsServerName,
    ""
)
require(rcStart == 0) { "start remote worker failed: $rcStart" }

val rcTasks = RemoteWorker.startRemoteWorkerTasks(0L)
require(rcTasks == 0) { "start remote worker tasks failed: $rcTasks" }

val status = RemoteWorker.getRemoteWorkerStatus()
```

## CA 文件复制

如果 CA 放在 `assets/control-ca.pem`，可以在首次启动时复制到 App 私有目录:

```kotlin
fun copyAssetToFiles(context: Context, assetName: String): File {
    val outFile = File(context.filesDir, assetName)
    context.assets.open(assetName).use { input ->
        outFile.outputStream().use { output ->
            input.copyTo(output)
        }
    }
    return outFile
}

val caFile = copyAssetToFiles(context, "control-ca.pem")
val caCertPath = caFile.absolutePath
```

## 参数说明

| 参数 | 说明 |
| --- | --- |
| `serverAddr` | gpuf-s 地址，可以是域名或 IP |
| `controlPort` | gpuf-s control 端口；TLS 模式要求服务端已开启 control TLS |
| `proxyPort` | gpuf-s proxy/data 端口 |
| `workerType` | 当前传 `"TCP"` |
| `clientId` | 后端分配的 32 位 hex client id；生产日志中不要打印完整值 |
| `caCertPath` | App 私有目录中的 CA bundle PEM 文件绝对路径；pin-only 模式可传空字符串 |
| `controlTlsServerName` | TLS SNI 和证书校验 server name；生产建议使用 DNS 名称 |
| `certSha256Pin` | 可选 leaf certificate SHA256 pin；使用 CA bundle 时可传空字符串 |

`validateMobileTlsPolicy(...)` 返回码:

| 返回码 | 含义 |
| --- | --- |
| `0` | TLS 参数有效 |
| `-1` | server name 缺失或非法 |
| `-2` | CA bundle 和 SHA256 pin 都未提供 |
| `-3` | CA bundle 路径或内容非法 |
| `-4` | SHA256 pin 非法 |
| `-5` | C 字符串包含非法 UTF-8 |

`startRemoteWorkerWithTls(...)` 返回码:

| 返回码 | 含义 |
| --- | --- |
| `0` | 启动成功 |
| `-1` | 参数、连接或登录失败 |
| `-2` | TLS policy 校验失败 |

## 兼容性

- 旧 Android SDK 调用 `startRemoteWorker(...)` 的代码不需要改签名。
- TLS 模式通过新增 `startRemoteWorkerWithTls(...)` 显式启用。
- 服务端没有开启 control TLS 时，客户端不能使用 TLS 入口连接该 control port。
- 同一个 `clientId` 同时只能有一个有效 worker 连接，重复登录可能被服务端拒绝或触发旧连接替换，取决于服务端版本。

## 安全要求

- 生产环境不要跳过证书校验。
- 生产环境建议使用 DNS 名称作为 `controlTlsServerName`。
- 自签测试环境必须提供 CA bundle 或 SHA256 pin，不能把服务端私钥下发到 App。
- App 日志中只打印 client id 的前后几位，避免完整输出。
- SDK 包发布前必须校验 `SHA256SUMS`。
