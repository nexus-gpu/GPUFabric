# GPUFabric `gpuf-c` 安全整改收敛计划

> 更新日期：2026-06-16 | 目标版本：v1.1.0 | 状态：P0-P3 代码层已全部实现并验证（45/45 test pass, fmt clean, secret scan clean, workspace zero warnings）；Android arm64 target 源码 warning 已清零；Android SDK/Remote Worker/TLS 与 Linux sanitizer 证据已附；mobile strict gate 已按 `status:` 阻断未闭环证据；剩余 escalate_to_human：iOS raw runtime、owner sign-off、生产签名/SBOM（4 项）；剩余 CI gate：cargo-audit/cargo-deny（2 项）；2026-06-16 新增 P4 算力分享 KV cache / worker 粘性路由实现计划，并已落地第一阶段 gpuf-s 服务端 session -> worker 粘性路由、响应 metadata、route 上限、sticky route metrics、`CommandV1` session/cache 透传、worker session/cache decision scaffold，以及默认关闭的 worker llama.cpp session state checkpoint 桥接路径（常驻内存 per-session `LlamaContext` / 真正 KV context pool、KV bytes 上限和端到端推理验收仍未完成，不阻断 v1.1.0）

## 目标

本计划只解决 `gpuf-c` 发布、移动 SDK 分发、模型下载、P2P data-plane、standalone API server 和安装脚本的安全上线风险。目标是把原先宽泛清单收敛成可拆分、可验收、可回滚的工程任务。

交付完成的定义：

- 默认配置、示例、SDK 和安装脚本不会自动连接公网 IP、暴露 `0.0.0.0` 服务、跳过完整性校验或保存明文长期凭据。
- 所有远程下载的模型、SDK 发布包和安装 payload 都有 SHA256 或签名校验，MD5 只能作为兼容显示信息，不能作为安全校验。
- standalone OpenAI/Anthropic 兼容 API、P2P TCP/UDP/TURN data-plane 和移动 SDK 都有明确认证、资源上限、日志脱敏和生命周期测试。
- `cargo audit` / `cargo deny` / secret scan / targeted tests 在 CI 或发布 gate 中可重复执行。

## 当前事实基线

以下结论来自 2026-06-04 对当前工作区的源码审计，后续执行时需要重新跑命令确认：

| 风险点 | 当前命中 |
|---|---|
| 硬编码公网服务地址 | `src/handle/worker_sdk.rs` 曾存在 `<legacy-public-endpoint>:17000` 回退；`docs/JNI_RemoteWorker_API_CN.md` 和 `examples/ios_sim_test/GPUFIosSimTest/ContentView.swift` 也曾包含旧公网端点示例。 |
| 模型下载完整性 | `src/main.rs` 直接从 HuggingFace 下载默认 TinyLlama GGUF 并 `std::fs::write` 到最终路径；`src/util/model_downloader.rs` 的 `checksum` 是 `Option<String>`，未强制。 |
| 下载落盘/删除保护 | downloader 会对 `output_path` 执行删除和直接写最终文件，未统一限制到 models dir，也未使用校验后原子 rename。 |
| YAML 依赖 | `Cargo.toml` 使用 `serde_yaml = "0.9.34-deprecated"`；`src/util/config.rs` 仅用于 Docker compose YAML 序列化/反序列化。原计划建议的 `serde_yml` 不能采用，因为 RustSec/OSV 已发布 `RUSTSEC-2025-0068`，标记其 unsound 且 unmaintained。 |
| HF token 传递 | `src/llm_engine/vllm_engine.rs` 将 token 放进 docker argv；当前参数序列还会把 token 作为裸 argv 暴露，且可能破坏 `docker run` 参数解析。 |
| Docker 默认隔离 | vLLM 在 Linux 使用 `--network host --ipc host`；Ollama 使用 GPU/render device 直通，并设置 `OLLAMA_HOST=0.0.0.0`。 |
| 安装脚本供应链 | `install.sh` 有 `curl ... | sh`、自动改 docker group/systemd、默认 `OLLAMA_HOST=0.0.0.0`；`install_client.sh` / `install_client.ps1` 使用 MD5 前缀校验。 |
| standalone API | `src/llm_engine/llama_server.rs` / `anthropic_server.rs` 没有认证、速率限制、prompt/token 上限或统一错误脱敏。 |
| P2P data-plane | `common::CommandV2` 的 P2P 消息只携带 `connection_id`；UDP 监听默认 `0.0.0.0:<port>`，失败后回退 `0.0.0.0:0`；fragment reassembly 缺少 TTL/总量限制；无 HMAC 和 replay window。 |
| 移动/FFI 面 | `src/lib.rs` 有 `static mut` 和大量 `unsafe`；JNI/C callback 生命周期、null/invalid UTF-8、重复 init/destroy、后台恢复还没有专门安全矩阵。 |
| SSE 生命周期 | Anthropic streaming 把无限 `ping_stream` chain 在 footer 之前，`message_stop` 可能永远不会发送。 |
| 本地产物泄露 | 当前 `.gitignore` 有 `*.claude`，但没有明确忽略 `.claude/`；`gpuf-c/src/target/` 当前出现在工作区。 |
| 算力分享 KV cache / worker 粘性路由 | 当前 `gpuf-c` 算力分享已缓存 LLAMA backend/model/global engine；旧 `generate_with_cached_model_sampling` / `stream_with_cached_model_sampling` 仍每次 `new_context(...)`，新 `stream_with_session_state_sampling` 已在算力分享 TCP worker 路径接入默认关闭的 llama.cpp `state_save_file/state_load_file` checkpoint：启用 `GPUF_ENABLE_SESSION_STATE_CACHE=1` 或 `GPUF_ENABLE_SESSION_KV_CACHE=1` 且请求带 `session_id` 时，按 session/model/runtime/prompt hash 保存“最后一个 prompt token 前”的本地 state，命中时 load 后 replay 最后 token，`bypass/unknown` 不读写，`reset` 清理该 session hash 目录后冷启动并重新保存。checkpoint 目录可用 `GPUF_WORKER_SESSION_STATE_DIR` 指定，并已支持 `GPUF_WORKER_SESSION_STATE_MAX_BYTES` 本地磁盘 quota（默认 0 表示只统计不淘汰），超额时按 mtime 淘汰旧 `.state` 文件并记录 metrics；state 文件旁边写 `.meta` sidecar，加载前校验版本、model/runtime hash、prompt hash 和 state SHA256，校验失败按 miss/error 处理，不把损坏文件送进 llama.cpp。`gpuf-s` 已新增第一阶段内存态 `session_id -> worker/client_id` 粘性路由，支持 `x-gpuf-session-id` / body `session_id`、`cache_policy=auto/bypass/reset` 和 bearer token hash owner scope 隔离，并将外部 `session_id` / `client_id` / `cache_status` 透传到 API 响应；route 表已有 TTL、最大条目、LRU 淘汰和受认证 metrics snapshot；`common::CommandV1` 已把 owner-scoped 派生 session key / `cache_policy` 透传到 worker，避免不同 owner 复用同外部 `session_id` 时在 worker 本地 checkpoint 撞 key；`gpuf-c` worker 已统一解析策略、脱敏记录 session 短 hash，并记录 decision、metadata 和 state checkpoint metrics。常驻内存 per-session `LlamaContext` / 真正 KV context pool、真实 KV byte 上限/eviction metrics、Redis 持久化和端到端推理验收尚未落地。 |

## 优先级总览

| 优先级 | 数量 | 时间盒 | 发布要求 |
|---|---:|---:|---|
| P0 上线阻断 | 6 | 4-6 天 | v1.1.0 发布前必须全部完成并有证据。 |
| P1 本迭代必修 | 8 | 8-12 天 | 可拆 PR 并行，但合并前必须进 CI 或发布 gate。 |
| P2 下迭代加固 | 6 | 8-12 天 | 不阻断 v1.1.0，阻断移动 SDK 大规模分发。 |
| P3 长期治理 | 4 | 持续 | 进入 release engineering backlog。 |
| P4 算力分享 KV cache / 粘性路由 | 1 | 分阶段实现 | P4-A/P4-B/P4-C 已落地，P4-D 已有默认关闭的 llama.cpp state checkpoint 桥接路径；常驻内存 per-session `LlamaContext` / 真正 KV context pool、KV 资源上限、持久化和端到端验收未完成，不阻断 v1.1.0。 |

## 2026-06-04 执行收敛状态

本轮已经把 P0/P1 从计划项推进到代码、脚本、CI 和文档层面的可验收实现。2026-06-09 已在拉取最新代码后完成 Android SDK 重新构建、SHA256 校验、Android arm64 target check 和 ARM64 Android 10 真机推理脚本验证。2026-06-10 已补 Android plaintext/TLS Remote Worker C harness raw runtime、Linux nightly ASAN/TSAN mobile unit sanitizer、Android arm64 target compile 刷新证据，并加固 mobile strict gate：正式移动发布现在会检查必需证据文件的 `status:`，带 `REQUIRED` / `PENDING` / `FOLLOWUP` / `CONDITIONAL` / `NOT_RERUN` 等未闭环标记时失败。iOS/macOS raw runtime、Android/iOS runtime sanitizer 或 release-owner substitute approval、生产 App 层 Android Keystore/iOS Keychain/日志审计签收、真实 release artifact 签名的部分仍保留为 release/mobile gate；不能在当前 Linux 工作区伪造为已完成。

| 范围 | 当前状态 | 本地/CI 证据 |
|---|---|---|
| P0-1 到 P0-6 | 已实现 | 硬编码公网 IP、MD5 信任路径、deprecated YAML 依赖、裸模型下载、默认公网监听面已收敛；CI 包含 source grep、cargo audit/deny、secret scan gate。 |
| P0-2 本地产物治理补充 | 已实现 | iOS SDK/Xcode 生成目录 `gpuf-c/build_ios/dist/`、`gpuf-c/build_ios/package/`、`gpuf-c/build_llama_ios/`、`gpuf-c/examples/**/DerivedData/` 已加入忽略规则；已跟踪的 iOS 构建产物从 Git index 移除，后续通过 release artifact + `SHA256SUMS` 分发。 |
| P1-1 到 P1-8 | 已实现 | standalone API auth/limits、Docker/HF token hardening、P2P HMAC/replay、UDP reassembly 限制、模型路径校验、SSE lifecycle、safe command wrapper 均有 targeted tests 或 grep gate。 |
| P2-1/P2-2 | Android C harness 与 Linux sanitizer 证据补齐，iOS/app-layer 证据仍为 release gate | C/JNI 已新增 `start_remote_worker_with_tls` / `startRemoteWorkerWithTls`，在旧协议外包 TLS，并支持 CA bundle、SNI/server name、SHA256 leaf pin；旧 `start_remote_worker` / `startRemoteWorker` 保持明文兼容。Android arm64 target compile、packaged Android SDK 真机推理脚本、Android plaintext Remote Worker C harness、Android TLS Remote Worker C harness 均已通过并附 raw runtime logs；Linux nightly ASAN/TSAN `gpuf-c mobile_` unit sanitizer 均通过。仍需要 iOS raw simulator/device runtime logs、Android/iOS runtime sanitizer 或 release-owner substitute approval，以及生产 App 层 Android Keystore/iOS Keychain、权限/日志审计签收；`scripts/mobile_sdk_release_gate.sh` 已支持 `GPUF_REQUIRE_MOBILE_EVIDENCE=1` 并检查证据 `status:`，正式移动 SDK release 缺平台证据或存在 blocking status 会失败。 |
| P2-3/P2-4/P2-6 | 部分完成 | `static mut` 源码 grep 清零；全局状态改锁/原子；TURN/P2P secret redacted/zeroize；`SecurityConfig` 和高危配置告警已落地。`worker_sdk`、JNI、Android SDK 和 `src/lib.rs` llama.cpp/multimodal FFI 路径继续移除不必要 `unsafe`，并为 chat-template FFI、C 字符串、callback `user_data`、token buffer、sampler/context/model helper 调用补充 `SAFETY` 说明；生成 token 文本日志改为长度脱敏。Linux `cargo check`、Android arm64 target check 和 Linux 上的 `--features ios-sdk` 检查均通过；真实 iOS target/simulator/device 仍需 macOS/Xcode release gate。2026-06-10 又补充 Android multimodal streaming callback、model-info/vision-token、single-token、model hot-swap 和 status-buffer 路径的 `SAFETY` 说明，并移除两个冗余 unsafe block；随后又清理低风险未使用参数/变量，2026-06-11 继续收敛 Android/JNI/handle/system-info 路径条件编译 warning；Android arm64 target 已无 gpuf-c 源码 warning（仅剩 workspace profile 与 build.rs 信息性 warning）。Linux lib 仍保留 13 个既有非 Android warning，后续继续跟踪 deprecated llama.cpp API、P2P TURN 错误记录和非 Android system-info 参数清理。 |
| P2-5 | TLS opt-in、证书回归和 Android additive TLS transport 证据已落地，iOS 运行证据仍为 release gate | `gpuf-s --control-tls` 与 `gpuf-c --control-tls --control-tls-server-name ...` 已实现；本地明文兼容、TLS 握手、同 CA 轮换证书、过期证书拒绝测试通过。移动 C/JNI 新增 `start_remote_worker_with_tls` / `startRemoteWorkerWithTls`，旧入口保持明文兼容；Linux `cargo check`、mobile TLS policy/config 单测、`aarch64-linux-android` target compile、Android 真机本地推理脚本、Android plaintext Remote Worker C harness 和 Android TLS Remote Worker C harness 通过。当前 Linux 主机无 `xcodebuild`/`xcrun`；iOS raw simulator/device TLS runtime log、Android/iOS runtime sanitizer 或 release-owner substitute approval、生产签名/SBOM 仍归入 release gate。 |
| P3-1/P3-4 | 基线完成 | `deny.toml`、`.github/workflows/security.yml`、`scripts/security_release_evidence.sh`、`docs/security-release-report.md` 已新增。 |
| P3-2/P3-3 | 部分完成/release gate | SBOM/签名已有模板和证据脚本；Android SDK archive `target/gpufabric-android-sdk-v9.0.0.tar.gz` 已重新生成并通过 `target/SHA256SUMS` 校验，当前 SHA256 为 `5cc92d884f04c431ccbe7cebefa7fb9c912baf5d4225966eb19750a078b9edef`。证据脚本已支持 `GPUF_REQUIRE_ARTIFACTS=1` / `GPUF_REQUIRE_SIGNING=1` 严格模式，缺签名证明会失败。当前 Linux 主机未安装 `cosign`/`minisign`，`gpg` 无 secret key；真实签名仍需 release job 使用正式 key 生成。standalone API 已有 opt-in content safety 和 metrics 测试，插件化/全链路策略仍为后续治理。 |

SDK 兼容性结论：当前改动保持 `gpuf-c` 既有公开 C/JNI/移动 SDK 函数名和参数签名兼容，未删除或重命名旧 public header/API。Android SDK 重新构建和真机测试修复只改变构建缓存选择、NDK ABI 校验和测试脚本调用方式，不要求旧集成方改 Java/C/REST 调用签名。新增 TLS 能力采用 additive API：C `start_remote_worker_with_tls(...)` 与 JNI `RemoteWorker.startRemoteWorkerWithTls(...)`；旧 `start_remote_worker(...)` / `startRemoteWorker(...)` 仍保持明文兼容，且 Android 真机 C harness 已验证 plaintext 与 TLS 两条 Remote Worker 入口。行为层面更严格：远程 worker 不再回退到硬编码公网地址，集成方必须显式传入 `serverAddr`/端口；远程模型和 SDK 发布包必须使用 SHA256 校验；公开监听、跳过校验、明文控制连接等高风险路径必须显式配置并进入日志/metrics。CLI/config 方式的 `gpuf-c` 已支持控制连接 TLS opt-in；iOS raw runtime log 仍作为移动 release gate 单独闭环。

本轮新增/要求的 release 验证命令记录在 `agent_artifacts/test_matrix.md` 和 `docs/security-release-report.md`，前端接入影响记录在 `docs/api_server.md`、`gui/doc.md`、`gpuf-s/api/api_server.md` 和 `gpuf-s/src/api_server/README.md`。

## P0 - 上线阻断

### P0-1 移除硬编码公网地址和不安全示例默认值

**范围**：`src/handle/worker_sdk.rs`、`docs/JNI_RemoteWorker_API_CN.md`、`docs/INFERENCE_SERVICE_ARCHITECTURE.md`、`examples/ios_sim_test/GPUFIosSimTest/ContentView.swift`、Android/iOS 示例、生成脚本输出模板。

**改法**：

- `worker_sdk` 没有显式 `server_addr` / `control_port` 时直接返回错误，并通过 callback 输出可诊断错误；禁止回退到任何公网 IP。
- 示例和文档只允许使用 `127.0.0.1`、`localhost`、`<your-server-host>` 这类占位，不出现真实公网 IP、token、TURN password。
- SDK 初始化 API 明确要求调用方显式传入服务地址；移动示例不得在 first-run 自动连接外部服务。

**验收**：

```bash
rg -n "8[.]140[.]251[.]142|17000" gpuf-c/src gpuf-c/docs gpuf-c/examples --glob '!**/DerivedData/**' --glob '!src/target/**'
```

允许剩余 `17000` 只作为端口说明或 CLI 默认值，不允许和真实公网 IP 组合出现。新增 targeted test：未配置 server addr 时 `start_worker_tasks_with_callback_ptr` 返回错误或 callback fatal，不启动后台 handler。

### P0-2 凭据和本地产物泄露治理

**范围**：`.gitignore`、仓库历史、CI、开发机工作流。

**改法**：

- `.gitignore` 明确加入 `.claude/`、`.agents/` 中本地授权文件、`gpuf-c/src/target/`、`gpuf-c/examples/**/DerivedData/`、本地 token/env 文件、临时下载 parts/tmp。
- 执行 secret scan：至少覆盖 `gitleaks detect` 或 `trufflehog git file://...`，并记录版本和命令输出。
- 对已进入 Git 历史或曾被复制到远程机器的 token/password 执行轮换；不能只删除文件。
- CI 增加 secret scan gate，拦截 HuggingFace token、私钥、`sshpass` 明文密码参数、云凭据、明文 TURN password、`.claude/`。

**验收**：

```bash
git status --short
git check-ignore -v .claude/settings.json gpuf-c/src/target/rust-analyzer/flycheck0/stdout
gitleaks detect --source . --redact --no-git
```

若没有安装扫描器，发布 gate 必须在 CI 环境安装并执行；本地报告写入发布记录。

### P0-3 模型下载强制完整性校验和原子落盘

**范围**：`src/main.rs`、`src/util/model_downloader.rs`、`src/util/model_downloader_example.rs`、模型管理文档。

**改法**：

- 引入 `Checksum { algorithm: Sha256, value }` 类型。所有远程模型下载必须传 `sha256`，没有 checksum 时拒绝下载；本地用户显式传入的已有文件只做路径/扩展名校验，不自动下载。
- `main.rs` 默认 TinyLlama 下载不能使用裸 `reqwest::get` + `std::fs::write`；必须走 downloader 或删除自动下载逻辑。
- 下载写入 `<output>.tmp.<pid>`；校验 expected size + SHA256 后 `rename` 到最终文件。
- 校验失败只删除受控 models dir 内的普通临时文件，不能删除任意 `output_path`。

**验收**：

- 单元测试：正确 SHA256 通过；错误 SHA256 返回错误并删除临时文件；最终路径不会出现半成品。
- 单元测试：`../../../etc/passwd`、symlink 出 models dir、非 `.gguf/.bin/.safetensors` 被拒绝。
- `rg -n "reqwest::get\(|std::fs::write\(&model_path" gpuf-c/src/main.rs gpuf-c/src/util` 无裸下载写法。

### P0-4 发布包和安装 payload 使用 SHA256/签名校验

**范围**：`install_client.sh`、`install_client.ps1`、`generate_sdk.sh`、`generate_ios_sdk.sh`、release artifact 生成流程。

**改法**：

- MD5 前缀校验降级为兼容提示，不能作为信任依据。
- 每个 `.tar.gz`、`.zip`、`.so`、`.a`、`.aar`、`.xcframework` 输出 `SHA256SUMS`；安装脚本必须下载并验证 SHA256 后再解压/安装。
- 发布包可选使用 cosign/minisign/GPG 签名；至少 SHA256 manifest 是 P0。
- 安装日志不打印 token、完整带 query 的 URL、远程密码或本地授权规则。

**验收**：

```bash
rg -n "MD5|md5|Get-FileHash -Algorithm MD5|md5sum" gpuf-c/install_client.sh gpuf-c/install_client.ps1
rg -n "SHA256|sha256|sha256sum|Get-FileHash -Algorithm SHA256" gpuf-c/install_client.sh gpuf-c/install_client.ps1 gpuf-c/generate_*.sh
```

发布 gate 用损坏包验证安装脚本会失败。

### P0-5 安装脚本和默认监听面收敛到 localhost

**范围**：`install.sh`、`install_client.sh`、`install_client.ps1`、`src/util/config.rs`、`src/llm_engine/ollama_engine.rs`、`src/handle/handle_tcp.rs` 中启动 Ollama 的路径。

**改法**：

- 删除 `curl ... | sh`，改为固定版本、固定 URL、GPG key/SHA256 校验的安装步骤。
- 默认 `OLLAMA_HOST=127.0.0.1`；只有显式 `--listen-public` 或配置项 `allow_public_listen = true` 时允许 `0.0.0.0`。
- 所有 `sudo`、systemd、docker group 修改前输出影响说明，并要求显式确认或非交互 `--yes`。
- Docker compose 模板和运行时容器默认只映射 localhost。

**验收**：

```bash
rg -n "0\.0\.0\.0|curl .*\| *sh|get\.docker\.com|OLLAMA_HOST" gpuf-c/install.sh gpuf-c/install_client.sh gpuf-c/install_client.ps1 gpuf-c/src
```

剩余 `0.0.0.0` 必须在显式 public-listen 分支内，并有高危日志。

### P0-6 YAML 依赖风险改成可审计决策，不迁移到 `serde_yml`

**范围**：`Cargo.toml`、`src/util/config.rs`、CI audit/deny。

**改法**：

- 不采用原计划中的 `serde_yml`，因为 `RUSTSEC-2025-0068` 标记其 unsound/unmaintained。
- 优先方案：Docker compose 生成改为 `serde_norway` 或其他经 `cargo audit` / `cargo deny` 验证的维护中替代库。
- 若只需要写出固定 compose 模板，评估删除 YAML 解析依赖，改为类型安全模板 + round-trip 测试。
- CI 增加 `cargo audit` 和 `cargo deny check advisories`，禁止新增 vulnerable/yanked/unsound crates；unmaintained crate 必须在 `deny.toml` 有带到期日的例外。

**验收**：

```bash
cargo audit
cargo deny check advisories
rg -n "serde_yaml|serde_yml" gpuf-c/Cargo.toml gpuf-c/src
```

`serde_yml` 不得出现在依赖中。

## P1 - 本迭代必修

### P1-1 standalone API 认证、限流和请求配额

**范围**：`src/llm_engine/llama_server.rs`、`src/llm_engine/anthropic_server.rs`、`src/util/cmd.rs`。

**改法**：

- 增加 `--api-key` / `GPUF_API_KEY`，release 模式默认要求认证；未配置 key 时只允许绑定 loopback。
- 对 `/v1/chat/completions`、`/v1/completions`、`/v1/messages`、`/v1/models` 应用同一认证中间件。
- 增加 `SecurityLimits`：`max_prompt_bytes`、`max_max_tokens`、`max_concurrent_generations`、`max_sse_connections`、`request_body_limit_bytes`。
- 超限请求在进入模型推理前拒绝。

**验收**：无 key 访问返回 401；正确 Bearer token 返回 200；超长 prompt、大 `max_tokens`、超并发均被拒绝且不调用模型。

### P1-2 HuggingFace token 不进入 argv、日志或 docker inspect 明文字段

**范围**：`src/llm_engine/vllm_engine.rs`、配置文档。

**改法**：

- 修复当前 docker 参数构造；不要把 token 作为裸 argv。
- 首选使用只读 secret 文件挂载：`/run/secrets/hf_token`，并用镜像支持的 token-file 机制；若镜像不支持，使用临时 env-file，但要记录 residual risk：Docker daemon 管理员仍可读取。
- token 文件权限 `0600`，容器启动后生命周期可控，日志只打印是否配置，不打印值。
- HF token 建议 fine-grained read-only，不使用账户主 token。

**验收**：启动容器后 `ps aux`、调试日志、错误日志中不出现 `hf_`；错误路径不会把 token 打进 `anyhow`。

### P1-3 Docker 容器隔离和镜像版本固定

**范围**：`src/llm_engine/vllm_engine.rs`、`src/llm_engine/ollama_engine.rs`、`src/util/config.rs`。

**改法**：

- 移除 vLLM 默认 `--network host --ipc host`；改为 localhost port mapping，必要时为性能/兼容提供显式高危开关。
- 添加 `--security-opt no-new-privileges:true`、合理 `--cap-drop`，只恢复经验证必需 capability。
- GPU device 直通和 `--gpus all` 做最小化配置；AMD/NVIDIA 路径分别测试。
- 镜像禁止 `latest` 作为发布默认，改为版本或 digest pin。
- 解析 Docker 返回的真实 container id，停止/检查容器时使用真实 id 或固定 name 二选一，避免混用。

**验收**：`docker inspect` 证明网络、cap、image digest 符合配置；GPU smoke test 通过。

### P1-4 P2P data-plane 消息认证和重放防护

**范围**：`common::CommandV2`、`src/handle/handle_tcp.rs`、`src/handle/handle_udp.rs`、服务端对应协议处理。

**改法**：

- 控制面为每个 P2P 连接下发 `data_plane_secret`，只保存在内存中。
- P2P request/chunk/done/cancel 和 UDP fragment header 增加 HMAC-SHA256 或 AEAD tag。
- MAC 覆盖 `connection_id`、`task_id`、`seq/msg_id`、fragment index/count、payload、timestamp/nonce。
- 增加 replay window，拒绝重复 `msg_id`、过期 timestamp、跨连接复用。
- TURN password 不复用为 data-plane secret。

**验收**：篡改 payload/tag、重放旧包、跨连接转发、乱序重复 fragment 都被拒绝并记录安全事件。

### P1-5 P2P UDP 绑定和 fragment reassembly 资源限制

**范围**：`src/util/cmd.rs`、`src/handle/handle_tcp.rs`、`src/handle/handle_udp.rs`。

**改法**：

- 增加 `--p2p-bind-addr`，默认 loopback 或明确 advertise IP；`0.0.0.0` 必须显式 `--p2p-public-listen`。
- 绑定失败不得静默回退到 `0.0.0.0:0`，必须返回错误或清晰降级到安全地址。
- reassembly map 限制总连接、每连接消息数、总 bytes、fragment count、fragment TTL。
- 对异常来源 IP 做短期 rate limit/ban；ACK 只发给已通过基础校验的包。

**验收**：fragment flood 测试不超过内存上限；公开监听未显式开启时无法绑定公网地址。

### P1-6 模型路径遍历、删除保护和文件类型限制

**范围**：`src/llm_engine/llama_engine.rs`、`src/main.rs`、`src/util/model_downloader.rs`。

**改法**：

- 引入 `validate_model_path(raw, allowed_models_dir)`：canonicalize、symlink 解析、扩展名白名单、目录边界校验。
- 允许显式绝对路径时必须通过 `--allow-external-model-path`，并打印风险提示。
- 删除操作只允许删除 models dir 内的普通临时文件；最终模型文件删除需要更严格条件。

**验收**：路径穿越、symlink escape、目录路径、非模型扩展名都返回错误。

### P1-7 SSE 结束、取消和错误脱敏

**范围**：`src/llm_engine/anthropic_server.rs`、`src/llm_engine/llama_server.rs`。

**改法**：

- Anthropic SSE 使用 merge/select keepalive，不把无限 ping chain 在 footer 前。
- stream 完成必须发送 `message_stop` / `[DONE]` 并关闭。
- 客户端断开时取消 token generation，释放推理资源。
- `AppError` release 返回脱敏错误，详细错误只写 server log。

**验收**：stream 集成测试能收到 stop event 并 EOF；断开连接后 generation task 退出；release 响应不泄露本地路径。

### P1-8 外部命令执行最小化、超时和输出上限

**范围**：`src/util/system_info.rs`、`src/util/system_info_vulkan.rs`、`src/util/device_info.rs`、`src/handle/handle_tcp.rs`。

**改法**：

- 禁止交互式 `sudo`；需要权限时返回可诊断错误。
- 对 `lspci`、`system_profiler`、`sysctl`、`pgrep/pkill`、`ollama`、`docker` 等命令统一封装 timeout 和 stdout/stderr 最大读取。
- 优先使用 `/sys`、`/proc`、Vulkan/ROCm/NVML API；避免 PATH 劫持，使用可信 PATH 或绝对路径。
- 避免 `pkill` 模糊匹配，改为记录 PID/进程组。

**验收**：命令卡住、输出过大、PATH 注入的测试不会阻塞或执行非预期 binary。

## P2 - 下迭代加固

### P2-1 移动 SDK TLS、凭据存储、日志和权限基线

- Android 使用 Keystore，iOS 使用 Keychain；禁止长期 token 落入 SharedPreferences、UserDefaults、明文 TOML/JSON。
- Release 默认启用 TLS 证书校验；自签证书通过 CA bundle 或 pinning。
- logcat/NSLog 默认不输出 token、server addr、prompt、生成内容、设备 ID、模型完整路径。
- Android manifest 和 iOS Info.plist 权限最小化，每个权限有文档说明。

### P2-2 移动 SDK FFI 生命周期测试矩阵

- Android instrumentation/JNI：null string、非法 UTF-8、超长 prompt、重复 init/start/stop/destroy、后台恢复、断网重连。
- iOS simulator/device：C callback 生命周期、Swift/ObjC 对象释放后 native 不再回调。
- 可用平台启用 ASAN/TSAN。
- SDK 示例作为安全默认 golden tests。

### P2-3 全局 state 和 `unsafe` 审计

**范围**：`src/lib.rs`、`src/jni_llama.rs`、`src/jni_remote_worker.rs`、`src/handle/android_sdk.rs`、`src/handle/worker_sdk.rs`。

- `static mut GLOBAL_CONTEXT_POSITION`、全局 pointer、async loading state、static buffers 改为受锁保护的状态结构或 scoped handle。
- 每个 FFI unsafe block 增加 `# Safety` 注释，说明指针、buffer、生命周期、锁条件。
- null pointer、buffer length、CString/UTF-8、callback user_data 的检查进入测试矩阵。

### P2-4 TURN/P2P 凭据内存保护

- TURN password、data-plane secret 使用 `zeroize` 或等效封装；drop 时清零。
- 避免在 `Debug` / `Clone` / error 中泄漏 secret。
- 评估 secret 生命周期，连接结束后立即释放。

### P2-5 控制连接 TLS 迁移

- `gpuf-s` 新增 `--control-tls`，启用后控制端口使用现有 `--proxy-cert-chain-path` / `--proxy-private-key-path` 证书和私钥。
- `gpuf-c` 新增 `--control-tls`、`--control-tls-server-name`，并可通过 `config.toml` 的 `[client].control_tls` / `control_tls_server_name` 配置；`--cert-chain-path` 用作 CA bundle。
- v1.1.0 为兼容保持明文默认；非 loopback 明文控制连接输出 deprecation/security warning。远程生产部署应同时在服务端和客户端启用 TLS。
- 已补本地回归：明文默认连接、TLS listener 握手、同 CA 轮换证书接受、过期证书拒绝均通过。移动 C/JNI 已补 Android 真机 plaintext/TLS Remote Worker 运行证据和 Linux nightly ASAN/TSAN mobile unit sanitizer 证据；仍未完成的 release gate：默认 TLS 切换、iOS raw simulator/device runtime log、Android/iOS runtime sanitizer 或 release-owner substitute approval、生产签名/SBOM。

### P2-6 统一安全配置和回滚策略

- 新增 `SecurityConfig`，集中管理 `require_api_key`、`enforce_model_checksum`、`allow_public_listen`、`enforce_p2p_hmac`、`safe_download_path` 等。
- P0 默认不可关闭；P1 可有临时回滚开关，但 release/mobile SDK 默认必须安全。
- 任何关闭 TLS、跳过 checksum、公开监听、禁用 P2P HMAC 的开关都必须打印高危告警并进入 metrics。

## P3 - 长期治理

### P3-1 cargo deny / cargo audit / license gate 常态化

- 新增 `deny.toml`，禁止 vulnerable/yanked/unsound crates。
- unmaintained advisories 默认为 warn 或 deny，由 release owner 审批例外和到期日。
- CI 缓存工具安装，避免每次联网安装导致不稳定。

### P3-2 SBOM 和发布签名

- release workflow 生成 SBOM，记录 llama.cpp、Rust crate、NDK、Xcode、Cargo feature、Docker image digest。
- release artifact 使用 cosign/minisign/GPG 之一签名。
- 文档提供客户验证命令。

### P3-3 内容安全过滤器作为可选功能

- 支持 prompt/output 长度限制、关键词黑名单、特殊 token 检测。
- 默认不改变模型输出语义，作为 opt-in 插件。
- 环境开关：`GPUF_CONTENT_SAFETY`、`GPUF_BLOCKED_KEYWORDS`、`GPUF_BLOCK_SPECIAL_TOKENS`。

### P3-4 安全回归仪表盘

- 记录 API/P2P 拒绝原因、checksum 失败、auth 失败、public-listen 使用、secret scan 结果。
- standalone API 暴露受认证保护的 `/v1/security/metrics` 快照端点。
- 发布前生成 `security-release-report.md` 和 `scripts/security_release_evidence.sh` 证据包，集中列出证据链接。

## P4 - 算力分享 KV cache / worker 粘性路由实现计划

> 状态：部分实现。`gpuf-s` 已接收 `session_id` / `x-gpuf-session-id` 和 `cache_policy=auto/bypass/reset`，并以内存 `SessionRoute` 表按 bearer token hash 做 owner scope 隔离，将同一 session 粘到同一可用 worker；外部 `session_id` / `client_id` / `cache_status` 已透传到 completion/chat JSON 响应和 `x-gpuf-*` 响应头。route 表已支持 `GPUF_SESSION_ROUTE_MAX_ENTRIES`、`GPUF_SESSION_ROUTE_TTL_SECS`、LRU/TTL 淘汰和受认证 `/api/v1/session/routes/metrics` snapshot。`common::CommandV1::{InferenceTask,ChatInferenceTask}` 已携带 owner-scoped 派生 session key / `cache_policy` 到 `gpuf-c` worker；worker 已通过 `handle::session_cache` 统一解析策略、脱敏记录 session 短 hash，并记录 cold/bypass/reset/unsupported/disabled 决策 metrics；该内部 worker decision snapshot 已纳入 standalone `/v1/security/metrics` 的 `worker_session_cache` 字段，并具备 worker session metadata TTL/LRU 上限。算力分享 TCP worker 路径已接入默认关闭的 llama.cpp session state checkpoint 桥接：启用 `GPUF_ENABLE_SESSION_STATE_CACHE=1` 或 `GPUF_ENABLE_SESSION_KV_CACHE=1` 且请求带 `session_id` 时，可按 scoped session/model/runtime/prompt hash 保存并恢复 prompt prefill state；日志和路径只使用 hash，不写完整 session id 或 prompt；`GPUF_WORKER_SESSION_STATE_MAX_BYTES` 已提供本地 checkpoint 磁盘 quota，超额按 mtime 淘汰旧 `.state` 文件；checkpoint `.meta` sidecar 已记录版本、model/runtime hash、prompt hash 和 state SHA256，load 前校验失败会按 miss/error 回退。`cache_policy=reset/bypass`、跨 owner 复用拒绝、模型切换失效、TTL/allowed worker 校验、容量淘汰、route metrics、协议字段 roundtrip、worker decision scaffold、worker metadata TTL/LRU、worker scoped session key、state checkpoint helper、checkpoint quota 和 sidecar 完整性已有 focused unit tests。常驻内存 per-session `LlamaContext` / 真正 KV context pool、真实 KV byte/hit/miss/eviction resource metrics、Redis 持久化、checkpoint 加密/签名和端到端推理验收仍未落地，本节不能作为 v1.1.0 全部完成项。

### P4-1 会话级 KV cache 与粘性路由

**范围**：`gpuf-s/src/inference/*`、`gpuf-s/src/handle/*`、`gpuf-c/src/llm_engine/llama_engine.rs`、`gpuf-c/src/handle/worker_sdk.rs`、`gpuf-c/src/handle/handle_tcp.rs`、`common/src/lib.rs`、API 文档和 SDK 文档。

**分阶段落地计划**：

| 阶段 | 目标 | 当前状态 | 完成条件 |
|---|---|---|---|
| P4-A 服务端 session 路由 | `gpuf-s` 接收 `session_id` / `cache_policy`，同 owner 的同一 session 粘到同一 worker。 | 已实现第一阶段：header/body 解析、owner scope、route TTL/max/LRU、响应 metadata、route metrics 和 focused tests 已落地。 | `cargo test -p gpuf-s inference::scheduler::tests` 通过；同 session 连续请求返回同 `client_id`；跨 owner 复用被拒绝。 |
| P4-B 协议透传 | `common::CommandV1` 把 session key / `cache_policy` 传到 worker。 | 已实现：`InferenceTask` / `ChatInferenceTask` 已增加字段；`gpuf-s` 发送给 worker 的是 SHA256 派生的 owner-scoped session key，而不是外部原始 `session_id`，API 响应仍返回外部 `session_id`。 | 协议 roundtrip test 通过；旧 worker 字段兼容策略明确；worker scoped session key 单测通过。 |
| P4-C worker 安全接收边界 | worker 对 session/cache policy 做统一解析、日志脱敏和 metrics scaffold，但仍按冷启动上下文执行。 | 已实现第一阶段：TCP worker、Android SDK worker 和 iOS/mobile `worker_sdk` 入口均调用同一 `handle::session_cache` helper，记录短 hash、policy、status 和 `kv_reuse=false`；standalone `/v1/security/metrics` 已返回 `worker_session_cache` snapshot；worker session metadata map 已有 TTL/LRU 上限；state checkpoint hit/miss/save/reset/error/quota eviction/current bytes/max bytes metrics 已加入 snapshot。 | `cargo test -p gpuf-c handle::session_cache::tests util::security_metrics::tests::snapshot_includes_worker_session_cache_metrics` 通过；`rg -n "session_id: _|cache_policy: _" gpuf-c/src/handle common/src gpuf-s/src` 无静默丢弃；worker 不误报 `kv_hit_total`。 |
| P4-D worker 内存 KV cache | `LlamaEngine` 维护 per-session `LlamaContext` / token prefix / last_used / estimated bytes。 | 部分实现：算力分享 TCP worker 路径已接入 `stream_with_session_state_sampling`，默认关闭；启用后用 llama.cpp `state_save_file/state_load_file` 对同 session、同模型/runtime、同 prompt 的 prefill state 做本地 checkpoint，命中前校验 sidecar SHA256，命中后 replay 最后 prompt token 再生成。常驻内存 per-session `LlamaContext` / token-prefix 增量复用仍待实现。 | 已有 helper 测试覆盖默认关闭、路径脱敏、`bypass/unknown` 不读写、`reset` 只清理 hash session dir、sidecar 缺失/篡改/版本不匹配 miss；还需真实模型端到端验证：相同 session/prompt 第二次减少 prefill，prefix mismatch/model hot swap 冷启动并记录 miss reason。 |
| P4-E 资源上限和淘汰 | 增加 worker KV byte/session/token TTL 限制，按 LRU/TTL 淘汰。 | 部分实现：worker session metadata 已支持 `GPUF_WORKER_SESSION_MAX_ENTRIES`、`GPUF_WORKER_SESSION_TTL_SECS`、LRU 淘汰、stale 清理和 reset 删除；checkpoint 文件按 hash session dir 隔离，并支持 `GPUF_WORKER_SESSION_STATE_MAX_BYTES` 本地磁盘 quota（默认 0 表示只统计不淘汰），超额时按 mtime 淘汰旧 `.state` 文件；真实 KV byte/token/context 上限仍待实现。 | metadata 和 checkpoint quota focused tests 通过；真实 KV cache 落地后还需补内存压力、活跃生成保护和 true KV bytes/current metrics。 |
| P4-F 持久化/恢复 | 可选 Redis route 持久化或磁盘 KV checkpoint。 | 部分实现：worker 本地 llama.cpp session state checkpoint 已可 opt-in，默认关闭，已有 hash 路径、reset 清理、quota 淘汰、sidecar SHA256 完整性校验和 metrics；Redis route 持久化、checkpoint 加密/签名、跨版本兼容策略仍待设计。 | 默认关闭；启用时 helper/quota/sidecar integrity 测试通过；后续需补加密、签名、跨版本兼容、清理和回滚方案。 |

**重连语义**：

- 客户端断开后重新请求同一 `session_id`，`gpuf-s` 会优先使用仍有效的 session route，把请求发回原 `client_id`；如果 worker 离线、权限变化、模型变化、route 过期或被 LRU 淘汰，则会清理旧 route 并冷启动选择 worker。
- “重连到同 worker”只保证调度粘性，不等于常驻内存 KV cache 命中。当前 opt-in state checkpoint 只能复用同 session、同模型/runtime、同 prompt 的 prefill state；真正 per-session KV hit 还要求原 worker 进程仍在、同模型的 per-session context 未被淘汰、prompt 是已缓存 token 前缀的安全延展，并且 `cache_policy` 不是 `bypass/reset`。
- worker 进程重启、模型 hot swap、context 被内存压力淘汰或 prefix mismatch 时必须返回 `cold` / `evicted` / miss reason，不能对外报告 `hit`。

**背景事实**：

- KV cache 通常是 GB 级资源，大小随 `layers * kv_heads * head_dim * 2(K+V) * ctx_tokens * bytes_per_elem` 增长；跨 worker 迁移成本高，不适合作为默认调度路径。
- 当前 `gpuf-c` LLAMA 路径缓存 `LlamaBackend` / `LlamaModel` / `GLOBAL_ENGINE`；旧路径每个请求仍新建 `LlamaContext`。算力分享 TCP worker 新路径在显式启用 state checkpoint 时仍会新建 context，但可从本地 llama.cpp state 文件恢复同 prompt 的 prefill state。
- P4 第一阶段实现前，`gpuf-s` 只能通过 `x-target-client-id` 显式指定 worker；默认路由会按授权 client 列表或负载重新选择 worker，没有 session id 到 worker 的粘性映射。

**目标**：

- 为算力分享 API 增加 `session_id` 语义：同一会话默认粘到同一个 `client_id`，从而允许 worker 在内存中复用 per-session KV cache。
- 在 worker 侧维护受控的 `SessionContext` 池，复用 llama context/KV；只有当 prompt 是已缓存 token 前缀的安全延展时才走增量 prefill，否则回退到冷启动 prefill。
- 保持安全默认：不同用户/API key/token 的 session 不得互相复用 KV；断线、取消、超时、模型切换、权限变化和 worker 下线时必须清理或失效相关 session。
- 不做默认跨 worker KV 迁移；后续如需磁盘 checkpoint，必须单独设计加密、完整性校验、版本兼容和磁盘配额。

**协议/API 改法**：

- REST/OpenAI 兼容层接受可选 `session_id` 或 `x-gpuf-session-id`；已实现。响应已返回实际绑定的 `session_id`、`client_id` 和 `cache_status`（`cold` / `hit` / `bypass` / `reset` / `evicted`）。
- `gpuf-s` 新增 `SessionRoute { session_id, owner_scope, client_id, model_id, created_at, last_used, ttl, state }`；内存表已实现核心字段、TTL、容量上限和 LRU 淘汰，`owner_scope` 现绑定 bearer token SHA-256 hash。Redis 同步和显式 `state` 尚未实现。
- `gpuf-s` 调度优先级：显式 `x-target-client-id` > 已存在且有效的 `session_id` 粘性路由 > 当前负载调度；已实现第一阶段。粘性 worker 离线或不可用时当前会清理 route 并冷启动重选 worker，默认不声称 KV cache hit；可配置诊断错误路径仍待实现。
- `common::CommandV1::{InferenceTask,ChatInferenceTask}` 已增加并透传可选 session key、`cache_policy`（`auto` / `bypass` / `reset`）；`gpuf-s` 传给 worker 的 session key 是 `owner_scope + 外部 session_id` 的 SHA256 派生值，API 响应仍使用外部 session id；`CommandV2`、`prompt_hash`、`prompt_tokens` 或前缀校验摘要仍待后续实现。

**worker 侧改法**：

- 已新增默认关闭的 `stream_with_session_state_sampling` 桥接路径：算力分享 TCP worker 将 owner-scoped session key / `cache_policy` 传入该路径；启用 `GPUF_ENABLE_SESSION_STATE_CACHE=1` 或 `GPUF_ENABLE_SESSION_KV_CACHE=1` 后，worker 使用 hash session dir 和 hash state filename 保存 llama.cpp prompt state，不在路径或日志中写完整 session id、prompt 或 token。
- 当前 checkpoint 保存“最后一个 prompt token 前”的 state，保存后写 sidecar meta；恢复时先验证 sidecar 的版本、model/runtime hash、prompt hash 和 state SHA256，再验证 cached token prefix 与当前 prompt token 完全匹配，最后 replay 最后一个 prompt token 产生 logits；`cache_policy=bypass` 和未知策略不读写，`cache_policy=reset` 清理该 session hash dir 后冷启动并重新保存。
- 待新增真正 per-session context map，值包含 `LlamaContext`、token prefix、last_used、model path/hash、n_ctx、estimated_kv_bytes、active flag。
- 将当前每次 `new_context(...)` 的生成路径拆成：
  - `get_or_create_session_context(session_id, model_id, limits)`
  - `sync_prompt_prefix(session, prompt_tokens)`
  - `decode_suffix_or_cold_prefill(session, suffix_tokens)`
  - `generate_from_session(session, sampling, max_tokens)`
- 当 prompt 与缓存 token prefix 不匹配、模型切换、采样路径要求独立上下文、context 剩余空间不足、或 session 被标记 reset 时，清空该 session 并走冷 prefill。
- Android/iOS 移动 worker 首期只支持模型 cache，不启用 KV session cache；当前 worker 已安全接收 `session_id` / `cache_policy`、输出脱敏 decision，并仍走冷启动上下文。后续启用 KV 前 API/worker 必须继续避免误报真实内存 KV hit。

**资源与安全限制**：

- 增加 `SessionCacheLimits`：`max_sessions_per_worker`、`max_total_kv_bytes`、`max_kv_bytes_per_session`、`session_ttl_secs`、`max_prompt_tokens_for_cache`、`max_idle_sessions`。第一阶段已实现 server route 表 `GPUF_SESSION_ROUTE_MAX_ENTRIES` 和 `GPUF_SESSION_ROUTE_TTL_SECS`；worker metadata 层已实现 `GPUF_WORKER_SESSION_MAX_ENTRIES` 和 `GPUF_WORKER_SESSION_TTL_SECS`；state checkpoint 目录可用 `GPUF_WORKER_SESSION_STATE_DIR` 指定，磁盘 quota 可用 `GPUF_WORKER_SESSION_STATE_MAX_BYTES` 指定；真实 worker KV byte/token/context 上限仍待实现。
- LRU/TTL 淘汰必须先取消活跃生成，再释放 context；淘汰、reset、权限失败、prefix mismatch 都进入 metrics。
- session id 必须是不可预测随机值或服务端生成值；禁止客户端用短字符串造成枚举/碰撞。日志只打印短 hash，不打印 prompt、token 内容或完整 session id。
- session route 的 owner_scope 校验失败时返回 403，并强制不复用 KV。
- worker 重连后只有同 `client_id` 且同进程保留了 session context 才能继续 hit；worker 进程重启或模型 hot swap 后必须把相关 route 标记 stale。

**观测与回滚**：

- metrics 增加：`kv_cache_hit_total`、`kv_cache_miss_total`、`kv_cache_eviction_total`、`kv_cache_bytes_current`、`sticky_route_hit_total`、`sticky_route_stale_total`、`session_owner_mismatch_total`。第一阶段已实现受认证 `/api/v1/session/routes/metrics`，返回 route current/max/TTL、hit/miss/bypass/reset/bind/eviction/stale/denied/owner mismatch 计数；worker 侧已新增 decision/metadata/checkpoint metrics scaffold（decisions/session_task/cold/bypass/reset/unsupported/disabled/active_sessions/max_sessions/TTL/metadata_eviction/metadata_stale/kv_hit=0/state_checkpoint_hit/miss/save/reset/error/quota_eviction/bytes_current/max_bytes），并挂到 standalone `/v1/security/metrics` 的 `worker_session_cache` 字段；真实 KV bytes、hit、miss、eviction 仍待 worker KV cache 实现后补齐。
- trace/release report 记录 cache 决策但不记录 prompt 正文。
- 配置开关：`GPUF_ENABLE_SESSION_STATE_CACHE=0/1` 或 `GPUF_ENABLE_SESSION_KV_CACHE=0/1` 启用当前 worker state checkpoint；`GPUF_WORKER_SESSION_STATE_DIR` 指定本地 checkpoint 目录；`GPUF_WORKER_SESSION_STATE_MAX_BYTES` 指定本地 checkpoint quota（默认 0 只统计不淘汰）；`GPUF_SESSION_STICKY_ROUTING=0/1`、`GPUF_SESSION_CACHE_MAX_BYTES` 保留给后续完整 KV cache。默认关闭 worker state/KV cache，只开启安全路由验证与显式 opt-in。

**验收**：

- 单元测试：同一 `session_id` 第二次请求命中同 worker；不同 owner/API key 使用同 `session_id` 被拒绝；worker 离线后 route 变 stale；`cache_policy=reset/bypass` 生效。第一阶段 route 表单元测试已覆盖这些调度语义、容量 LRU 淘汰和 metrics 计数；worker-scoped session key 测试已覆盖不同 owner 派生不同 worker key；worker helper 测试已覆盖 checkpoint 默认关闭、路径脱敏、bypass/unknown 不读写、reset hash dir 清理、quota 淘汰旧 `.state` 文件，以及 sidecar 缺失/篡改/版本不匹配校验失败；真实模型 KV/cache hit 语义仍待后续测试。
- 集成测试：两台 worker 在线时，带同一 `session_id` 的连续请求落到同一 `client_id`；无 `session_id` 请求仍按负载调度。
- 推理测试：相同长 prompt 的第二次请求减少 prefill token/耗时；prefix mismatch 不复用旧 KV；模型 hot swap 后旧 session 全部失效。
- 压力测试：超过 `max_total_kv_bytes` 或 `max_sessions_per_worker` 时按 LRU/TTL 淘汰，内存回落且无活跃请求崩溃。
- 安全测试：session id 枚举、跨 token 复用、日志泄漏、取消/断连后残留 context、worker 重连 stale route 均被覆盖。

## 推荐执行顺序

```text
Day 1: P0-2 -> P0-1 -> P0-5
Day 2: P0-3 -> P0-4
Day 3: P0-6 -> P1-1 targeted design
Day 4-6: P1-2 -> P1-3 -> P1-6
Day 7-10: P1-4 -> P1-5 -> P1-7 -> P1-8
Next iteration: P2 mobile/unsafe/TLS work
Later architecture track: P4 session sticky routing -> P4 worker-side KV session cache -> P4 metrics/resource limits
```

拆 PR 原则：

1. 先改默认值和扫描 gate，再改协议和容器隔离。
2. 每个 PR 都带一个可重复验收命令或测试，不接受“人工检查即可”。
3. 协议变更必须先写兼容策略和版本字段，再落实现。
4. 安全默认值优先于兼容性；兼容开关必须显式、有日志、有到期计划。

## 测试矩阵

| 改动 | 最低测试 |
|---|---|
| P0-1 硬编码地址 | `rg` 无真实公网 IP；未配置 server addr 返回错误；移动示例不自动连接。 |
| P0-2 secret 治理 | `git check-ignore` 命中本地产物；gitleaks/trufflehog 无高危明文；疑似 token 已轮换。 |
| P0-3 模型完整性 | 正确/错误 SHA256、断点续传、中断下载、临时文件清理、路径越界删除保护。 |
| P0-4 发布包校验 | 损坏包安装失败；SHA256 manifest 完整；MD5 不参与信任决策。 |
| P0-5 默认监听 | 默认 localhost；公开监听必须显式开关；安装脚本不再 pipe-to-shell。 |
| P0-6 YAML 依赖 | `cargo audit` / `cargo deny check advisories` 通过；无 `serde_yml`；替代库 round-trip 测试通过。 |
| P1-1 API 认证/配额 | 401/200、超长 prompt、大 max_tokens、超并发、body limit、stream limit。 |
| P1-2 HF token | `ps`、debug log、error log 无 token；临时 secret 文件权限和生命周期正确。 |
| P1-3 Docker hardening | `docker inspect` 校验网络/cap/image digest；GPU smoke test。 |
| P1-4 P2P HMAC | tag 篡改、payload 篡改、replay、跨连接转发均拒绝。 |
| P1-5 UDP 限制 | fragment flood 不超过内存上限；TTL 清理；公网监听默认不可用。 |
| P1-6 路径安全 | traversal、symlink escape、非法扩展、外部路径开关测试。 |
| P1-7 SSE/错误 | stream stop + EOF；断连取消；release 错误脱敏。 |
| P1-8 外部命令 | timeout、输出上限、PATH 注入、无交互 sudo。 |
| P2 mobile/unsafe | Android instrumentation、iOS simulator/device、ASAN/TSAN、callback 生命周期。 |
| P4 算力分享 KV cache | session 粘性路由、owner scope 隔离、同 worker KV hit、prefix mismatch 回退、模型切换失效、LRU/TTL/字节上限淘汰、worker 重连 stale route。 |

## 发布 gate

v1.1.0 发布前至少提供：

```bash
cargo fmt --all --check
cargo test -p gpuf-c
cargo test -p gpuf-c util::security_metrics::tests
cargo audit
cargo deny check advisories
GPUF_REQUIRE_ARTIFACTS=1 GPUF_REQUIRE_SIGNING=1 GPUF_SIGNING_TOOL=<cosign|minisign|gpg> scripts/security_release_evidence.sh security-release-evidence <artifact-dir>
GPUF_REQUIRE_MOBILE_EVIDENCE=1 scripts/mobile_sdk_release_gate.sh security-release-evidence/mobile-sdk <mobile-evidence-dir>
rg -n "8[.]140[.]251[.]142|hf_[[:alnum:]]{20,}|sshpass[[:space:]]+-p|BEGIN[[:space:]].*PRIVATE[[:space:]]KEY|OLLAMA_HOST=0\.0\.0\.0" gpuf-c/src gpuf-c/docs gpuf-c/examples gpuf-c/install.sh gpuf-c/install_client.sh gpuf-c/install_client.ps1 gpuf-c/generate_sdk.sh gpuf-c/generate_ios_sdk.sh .github --glob '!**/target/**' --glob '!**/DerivedData/**'
gitleaks detect --source . --redact
```

如果某个工具在本地不可用，CI 必须执行等效 gate，并把输出附到发布记录。PR/普通 CI 可不传 artifact 或 mobile evidence 目录生成状态说明；正式 release job 必须启用 `GPUF_REQUIRE_ARTIFACTS=1`、`GPUF_REQUIRE_SIGNING=1`，移动 SDK 发布还必须启用 `GPUF_REQUIRE_MOBILE_EVIDENCE=1`。移动 SDK 严格 gate 不只检查文件存在；每个必需证据文件必须带非阻断 `status:`，包含 `REQUIRED`、`REQUIRES`、`PENDING`、`FOLLOWUP`、`ACTION`、`CONDITIONAL`、`NOT_RERUN`、`MISSING`、`INCOMPLETE`、`TODO` 或 `TBD` 等未闭环标记时必须失败。
