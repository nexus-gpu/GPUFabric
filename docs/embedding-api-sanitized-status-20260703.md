# Embedding API 当前情况脱敏记录

记录时间：2026-07-03  
分支：`feature/embedding-api`  
范围：GPUFabric embedding API、Linux/Mac `gpuf-c` 分享、移动 SDK 影响面、测试环境验证。

## 脱敏原则

- 不记录真实 `client-id`、公网 IP、数据库凭据、证书私钥、OSS 凭证、SSH 主机别名背后的敏感信息。
- 测试环境统一记为 `<TEST_GPUF_S>`，线上环境统一记为 `<PROD_GPUF_S>`。
- CA 只记录“使用 CA 公钥证书”，不记录私钥或证书内容。
- 本地模型文件只记录模型名称，不记录用户目录下的完整个人路径。

## 当前结论

1. Embedding API 可以先只支持 Linux/Mac/Windows 新版 `gpuf-c`。
2. Android/iOS SDK 暂时不实现 embedding 推理能力也可以上线，但服务端调度必须过滤移动端。
3. 已在服务端 embedding 调度中跳过 `ANDROID / IOS` 客户端，并要求新版 embedding-capable CommandV1 协议与 Llama engine，避免把 embedding 任务派给 SDK、旧版桌面 worker 或非 Llama worker。
4. 新增 `CommandV1::EmbeddingTask` 和 `CommandV1::EmbeddingResult` 已放在枚举末尾，避免破坏旧客户端已有消息编号。
5. 本地 Vulkan 不是不可用，问题是默认可能选到软件 Vulkan 设备；指定 RADV ICD 后可使用真实 GPU。

## 已实现内容

服务端 `gpuf-s`：

- 新增 OpenAI 兼容接口：`POST /v1/embeddings`。
- 新增 Sophnet 兼容接口：`POST /api/open-apis/projects/:project_id/easyllms/embeddings`。
- 支持 `bge-m3` / `bge-m3-q8_0` 类 1024 维文本向量模型。
- 新增 embedding 任务 pending/timeout/result 处理。
- embedding 调度严格要求模型匹配，不 fallback 到普通客户端。
- embedding 调度过滤 Android/iOS SDK 客户端、旧版协议客户端和非 Llama worker。

客户端 `gpuf-c`：

- Linux/Mac/Windows llama engine 支持执行 embedding 任务。
- Android SDK 目前仅识别 embedding 任务并返回“不支持”，避免接口挂起。

协议 `common`：

- 新增 `EmbeddingTask` / `EmbeddingResult`。
- 新增协议 roundtrip 测试。
- 新增 variant 放在 `CommandV1` 末尾，降低旧客户端兼容风险。

## 当前支持的分享 API

`gpuf-s` inference gateway 当前面向算力分享调用方支持：

| API | 状态 | 说明 |
|---|---|---|
| `POST /v1/chat/completions` | 支持 | OpenAI 风格聊天补全，推荐给普通文本推理 |
| `POST /v1/completions` | 支持 | OpenAI legacy 文本补全 |
| `POST /v1/embeddings` | 支持 | OpenAI 风格文本向量接口，要求非移动端新版 `gpuf-c` 加载兼容 embedding 模型 |
| `POST /api/open-apis/projects/:project_id/easyllms/embeddings` | 支持 | Sophnet 兼容文本向量接口，当前支持 `bge-m3` 1024 维 float 向量 |
| `GET /v1/models` | 支持，简化版 | 返回当前网关模型列表，响应结构不是完整 OpenAI list envelope |
| `POST /v1/messages` | 暂不支持 | Anthropic 风格接口只在 `gpuf-c` standalone 本地服务中存在 |
| `/v1/responses`、图片、音频、文件 API | 暂不支持 | 当前分享网关没有对应 route |

Embedding 分享限制：

- 只支持文本 embedding，不支持图片 embedding / CLIP。
- 当前 `bge-m3` 维度固定为 `1024`。
- 编码格式只支持 `float`。
- Android/iOS SDK worker、旧版桌面 worker、非 Llama worker 会被服务端调度过滤，不参与 embedding 分享。

## 已验证结果

本地/测试环境已验证：

- `cargo fmt` 通过。
- `cargo test -p common` 通过。
- `cargo check -p gpuf-s` 通过。
- `cargo check -p gpuf-c --features vulkan` 通过。
- `cargo build --release -p gpuf-c --features vulkan` 通过。
- 测试环境 `<TEST_GPUF_S>` 可接收新版 `gpuf-c` 连接。
- `/v1/embeddings` 返回 1024 维向量。
- Sophnet 兼容 embedding 接口返回 1024 维向量。
- token usage 记录中可看到 embedding 类型调用。

Vulkan 验证要点：

- 默认 Vulkan loader 可能选到 `llvmpipe` 软件设备。
- 指定 RADV ICD 后，日志确认使用真实 AMD GPU，并将模型层 offload 到 GPU。
- 建议后续在安装脚本或启动命令中处理 Vulkan ICD 自动选择，避免用户机器上误选软件设备。

## SDK 影响面

不更新 Android/iOS SDK 的影响：

- 普通算力分享、聊天推理不应受影响。
- 移动 SDK 暂时不能分享 embedding 模型。
- 服务端已经过滤移动 SDK，因此 embedding 请求不会被派到 Android/iOS。

如果未来要支持移动 SDK embedding：

- 需要补移动端模型加载和 embedding 推理能力。
- 需要确认移动端引擎、模型格式、内存限制和性能。
- 需要增加 SDK 集成测试和回归测试。

## 上线注意

上线前需要重新构建并部署包含以下修正的 `gpuf-s`：

- `CommandV1` 新增 variant 放在末尾的协议兼容修正。
- embedding 调度过滤 Android/iOS、旧协议客户端和非 Llama worker 的修正。

线上客户端包建议：

- Linux/Mac 新版 `gpuf-c` 包含 `ca-cert.pem`，只放 CA 公钥证书。
- 启动命令包含 TLS 参数和 CA 路径。
- 对 Vulkan Linux 包，建议处理 `VK_ICD_FILENAMES` 或其他 GPU 选择策略。

## 待办

1. 重新构建并部署测试环境 `gpuf-s`，确认协议顺序修正和移动端过滤生效。
2. 用新版 Linux/Mac `gpuf-c` 再跑一次 embedding 端到端测试。
3. 再决定是否把 Vulkan ICD 自动选择写入安装脚本或启动文档。
4. 暂缓 Android/iOS SDK embedding 实现，除非明确要移动端分享向量模型。
