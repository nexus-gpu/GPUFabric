# GPUFabric iOS SDK App 接入指南

更新时间: 2026-06-11

本文面向 iOS App / Objective-C++ / Swift 接入方，说明新版 `gpuf_c_sdk.xcframework` 的集成方式。该版本包含 Remote Worker TLS control stream 相关 C API 和静态 TLS 依赖，App 侧不需要再额外集成 rustls/OpenSSL 文件。

## 交付物

新版 SDK 包建议交付为:

```text
gpuf_c_ios_sdk_tls_20260611.zip
```

包内核心内容:

```text
gpuf_c_sdk.xcframework/
  ios-arm64/
    libgpuf_c_sdk.a
    Headers/
      gpuf_c.h
      gpuf_c_minimal.h
  ios-arm64-simulator/
    libgpuf_c_sdk.a
    Headers/
      gpuf_c.h
      gpuf_c_minimal.h
SHA256SUMS
certs/
  control-ca.pem
IOS_APP_INTEGRATION_CN.md
IOS_SDK_INTEGRATION_EN.md
TEST_ENV_PARAMS.md
RELEASE_NOTES.md
```

当前支持:

- iOS 真机: `arm64`
- Apple Silicon 模拟器: `arm64`

当前不包含:

- Intel Mac 模拟器: `x86_64`

原因是本地缺少 `target/llama-ios/x86_64-apple-ios` 对应的 llama.cpp iOS 预编译库。需要 Intel 模拟器时，先补齐该 slice 再重新执行 `gpuf-c/generate_ios_sdk.sh`。

## Xcode 集成

1. 将 `gpuf_c_sdk.xcframework` 拖入 App 工程。
2. 在 App target 的 `General` -> `Frameworks, Libraries, and Embedded Content` 中确认已添加。
3. 因为这是静态库形式的 XCFramework，`Embed` 选择 `Do Not Embed`。
4. 在 `Build Phases` -> `Link Binary With Libraries` 添加:

```text
Metal.framework
Accelerate.framework
Foundation.framework
libc++.tbd
```

通常系统会自动链接 Objective-C runtime、pthread 和 libSystem。如果项目报相关 undefined symbol，再显式添加:

```text
libobjc.tbd
libSystem.tbd
```

TLS 说明:

- TLS control stream 使用 SDK 内部 Rust TLS 实现和静态依赖。
- App 侧不需要额外放 `rustls`、`ring`、OpenSSL 或其他 TLS 静态库。
- App 侧只需要提供 CA bundle 文件路径、SNI/server name、可选证书 SHA256 pin。

## Swift Bridging Header

新建或更新 bridging header，例如 `GPUFabric-Bridging-Header.h`:

```objc
#ifndef GPUFabric_Bridging_Header_h
#define GPUFabric_Bridging_Header_h

#include <gpuf_c.h>

#endif
```

然后在 target 的:

```text
Build Settings -> Swift Compiler - General -> Objective-C Bridging Header
```

配置该文件路径。

## Remote Worker 普通接入

不启用 TLS 时，旧 API 仍保持兼容:

```swift
let rc = server.withCString { serverPtr in
    "TCP".withCString { workerPtr in
        clientId.withCString { clientPtr in
            start_remote_worker(serverPtr, controlPort, proxyPort, workerPtr, clientPtr)
        }
    }
}
```

推荐调用顺序:

```text
set_remote_worker_model(model_path)
start_remote_worker(...)
gpuf_register_remote_worker_callback(...)
start_remote_worker_tasks()
get_remote_worker_status(...)
stop_remote_worker()
```

## Remote Worker TLS 接入

TLS 是新增 API，不影响旧的 `start_remote_worker`。

推荐调用顺序:

```text
set_remote_worker_model(model_path)
gpuf_validate_mobile_tls_policy(ca_cert_path, control_tls_server_name, cert_sha256_pin)
start_remote_worker_with_tls(...)
gpuf_register_remote_worker_callback(...)
start_remote_worker_tasks()
get_remote_worker_status(...)
stop_remote_worker()
```

参数说明:

| 参数 | 说明 |
| --- | --- |
| `server_addr` | gpuf-s 服务器地址，可以是域名或 IP |
| `control_port` | gpuf-s control 端口；启用 TLS 时服务端也必须开启 control TLS |
| `proxy_port` | gpuf-s proxy/data 端口 |
| `worker_type` | 当前传 `"TCP"` |
| `client_id` | 32 位 hex client id；不要在生产日志打印完整值 |
| `ca_cert_path` | App 沙盒内 CA bundle PEM 文件路径；pin-only 模式可传空字符串 |
| `control_tls_server_name` | TLS SNI 和证书校验 server name；生产建议传 DNS 名 |
| `cert_sha256_pin` | 可选证书 SHA256 pin；不用时传空字符串 |

测试环境参数模板:

```text
server_addr = <test-gpuf-s-host-or-ip>
control_port = 17100
proxy_port = 17101
worker_type = TCP
control_tls_server_name = <test-control-tls-server-name>
ca_cert_path = <App 沙盒或 Bundle 内 control-ca.pem 的绝对路径>
cert_sha256_pin = ""  # 使用 CA bundle 时可留空
client_id = <32 位 hex client id，由后端/测试环境分配>
```

本次测试包内置测试 CA:

```text
certs/control-ca.pem
CA subject: <test-ca-subject>
CA SHA256 fingerprint:
<test-ca-sha256-fingerprint>
有效期: <test-ca-not-before> 至 <test-ca-not-after>
```

测试环境服务端证书信息:

```text
subject: <test-server-subject>
SAN: <test-server-san-list>
server cert SHA256 fingerprint:
<test-server-cert-sha256-fingerprint>
有效期: <test-server-not-before> 至 <test-server-not-after>
```

生产环境填写规则:

```text
server_addr: 生产 gpuf-s 可访问域名或 IP。推荐传域名。
control_port: 生产 gpuf-s control TLS 端口。
proxy_port: 生产 gpuf-s proxy/data 端口。
control_tls_server_name: 必须和生产服务端证书 SAN 匹配。推荐使用生产 DNS 名，例如 <production-gpuf-s-dns>。
ca_cert_path: 生产 CA bundle 在 App Bundle 或沙盒内的绝对路径。
cert_sha256_pin: 可选。若启用 pin，传生产服务端证书 SHA256 指纹或 64 位 hex。
```

不要把测试 CA 当作生产 CA 使用。生产证书换发后，如果 pin 绑定的是 leaf server cert，需要同步更新 App 配置；如果只用 CA bundle，则确保新服务端证书仍由该 CA 签发且 SAN 包含 `control_tls_server_name`。

`cert_sha256_pin` 支持以下格式:

```text
sha256:<64-hex-sha256-pin>
<colon-separated-sha256-pin>
<64-hex-sha256-pin>
```

至少要提供 `ca_cert_path` 或 `cert_sha256_pin` 之一。

Swift 示例:

```swift
let caURL = Bundle.main.url(
    forResource: "control-ca",
    withExtension: "pem"
)!

let server = "<test-gpuf-s-host-or-ip>"
let controlPort: Int32 = 17100
let proxyPort: Int32 = 17101
let serverName = "<test-control-tls-server-name>"
let caCertPath = caURL.path

func startTLSWorker(
    modelPath: String,
    server: String,
    controlPort: Int32,
    proxyPort: Int32,
    clientId: String,
    caCertPath: String,
    serverName: String,
    certPin: String = ""
) -> Int32 {
    let modelRc = modelPath.withCString { set_remote_worker_model($0) }
    guard modelRc == 0 else { return modelRc }

    let policyRc = caCertPath.withCString { caPtr in
        serverName.withCString { namePtr in
            certPin.withCString { pinPtr in
                gpuf_validate_mobile_tls_policy(caPtr, namePtr, pinPtr)
            }
        }
    }
    guard policyRc == 0 else { return policyRc }

    return server.withCString { serverPtr in
        "TCP".withCString { workerPtr in
            clientId.withCString { clientPtr in
                caCertPath.withCString { caPtr in
                    serverName.withCString { namePtr in
                        certPin.withCString { pinPtr in
                            start_remote_worker_with_tls(
                                serverPtr,
                                controlPort,
                                proxyPort,
                                workerPtr,
                                clientPtr,
                                caPtr,
                                namePtr,
                                pinPtr
                            )
                        }
                    }
                }
            }
        }
    }
}
```

如果证书不是放在 App Bundle，而是首次启动后复制到 Documents，请把 `caCertPath` 改成复制后的 Documents 绝对路径。SDK 只读取本地 PEM 文件，不负责从网络下载证书。

TLS policy 返回码:

```text
0   valid
-1  server name 缺失或非法
-2  CA bundle 和 SHA256 pin 都没有提供
-3  CA bundle 文件路径或内容非法
-4  SHA256 pin 格式非法
-5  C 字符串 UTF-8 非法
```

`start_remote_worker_with_tls` 返回码:

```text
0   success
-1  参数、连接或登录失败
-2  TLS policy 无效
```

## 回调接入

推荐使用 `gpuf_register_remote_worker_callback`，不要再把函数指针强转成整数传入。

```swift
private let workerCallback: gpuf_status_callback = { message, userData in
    guard let message else { return }
    let text = String(cString: message)
    DispatchQueue.main.async {
        print("GPUFabric status:", text)
    }
}

_ = gpuf_register_remote_worker_callback(workerCallback, nil)
_ = start_remote_worker_tasks()
```

注意:

- callback 必须是稳定的 C callback，不要捕获 Swift 对象。
- 如需关联 Swift 对象，把对象包装后通过 `user_data` 传递并管理生命周期。
- 生产日志不要打印完整 client id、token、prompt、模型路径、证书 pin。

## 本地推理接入

SDK 同时提供本地文本推理 C API:

```swift
_ = gpuf_init()

let model = modelPath.withCString { gpuf_load_model($0) }
let context = gpuf_create_context(model)

var output = [CChar](repeating: 0, count: 4096)
let rc = prompt.withCString { promptPtr in
    gpuf_generate_final_solution_text(
        model,
        context,
        promptPtr,
        128,
        &output,
        Int32(output.count)
    )
}

if rc > 0 {
    print(String(cString: output))
}

llama_free(context)
llama_model_free(model)
_ = gpuf_cleanup()
```

模型文件必须是 App 可访问的本地 `.gguf` 路径，例如 bundle resource 或 Documents 目录。

## 常见问题

### Header not found

确认 bridging header 使用:

```objc
#include <gpuf_c.h>
```

并确认 `gpuf_c_sdk.xcframework` 已加入 App target，而不只是拖进工程目录。

### Undefined symbol: std::__1...

添加:

```text
libc++.tbd
```

### Undefined symbol: _MTLCreateSystemDefaultDevice

添加:

```text
Metal.framework
```

### Undefined symbol: _cblas_...

添加:

```text
Accelerate.framework
```

### TLS 校验失败

检查:

- `control_tls_server_name` 是否和服务端证书 SAN/CN 匹配。
- `ca_cert_path` 是否指向 App 沙盒内真实存在的 PEM 文件。
- `cert_sha256_pin` 是否为 64 位 hex，或者带 `sha256:` 前缀，或者冒号分隔格式。
- 服务端 gpuf-s 是否启用了 control TLS。

### 测试环境验证结果

2026-06-11 使用本包同版本 SDK 在 iOS Apple Silicon Simulator 复测通过:

```text
TLS control connection: PASS
Remote Worker login: PASS
Heartbeat: PASS
Model status upload: PASS, model id = llama3
DB online/offline lifecycle: PASS
Inference Gateway routed chat request to iOS worker: PASS, HTTP 200
```

服务端日志中的 client id 必须只保留前后少量字符，例如 `<client-prefix>...<client-suffix>`。交付文档和日志不要记录完整 client id、token、prompt、证书私钥或生产 pin。

### 模拟器 slice

当前包支持 Apple Silicon 模拟器 `ios-arm64-simulator`。Intel Mac 模拟器需要额外生成 `ios-x86_64-simulator`。
