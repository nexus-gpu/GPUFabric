# gpuf-p2p-proxy 轻量 P2P API 转发客户端

`gpuf-p2p-proxy` 是一个本地轻量转发器，用来把 OpenAI-compatible 分享 API 请求自动尝试转成 GPUFabric P2P 数据面请求。它不链接 llama/cuda/vulkan，不负责本地推理，只负责：

- 在本地监听 HTTP API；
- 以 consumer-only 身份连接 `gpuf-s` 控制面；
- 向目标 `gpuf-c` 发起 P2P 信令；
- 将文本 chat 请求转为 `P2PInferenceRequest`；
- 收集 `P2PInferenceChunk` / `P2PInferenceDone` 后返回 OpenAI-compatible JSON；
- P2P 不可用时自动 fallback 到现有 `gpuf-s` 分享 API。

## 当前 MVP 能力

已支持：

- `POST /v1/chat/completions`
- 非流式文本请求：`stream` 不传或 `false`
- 通过 `--target-client-id` 或请求头 `x-target-client-id` 指定目标 `gpuf-c`
- direct UDP P2P 数据面
- P2P usage 安全上报：consumer report 必须和目标 `gpuf-c` receipt 匹配后才落库
- P2P 失败后转发到 `--fallback-base-url`
- `GET /healthz`

当前 fallback：

- `stream=true`
- OCR / vision / 多模态图片请求
- `/v1/embeddings`
- `/v1/models`
- `/v1/completions`
- Sophnet embeddings adapter

下一阶段要接入：

- P2P SSE streaming；
- `P2PFileStart / P2PFileChunk / P2PFileDone / P2PFileAck / P2PFileCancel` OCR 文件传输；
- TURN/UDP fallback；
- 基于模型 capability 的自动目标选择。

## 构建

从仓库根目录执行：

```bash
cargo build -p gpuf-p2p-proxy --release
```

开发检查：

```bash
cargo check -p gpuf-p2p-proxy
cargo test -p gpuf-p2p-proxy
```

## 启动

本地开发示例：

```bash
target/release/gpuf-p2p-proxy \
  --listen 127.0.0.1:18088 \
  --server-addr <gpuf-s-host> \
  --control-port 17000 \
  --consumer-id <consumer-id-32-hex> \
  --target-client-id <target-gpuf-c-client-id-32-hex> \
  --fallback-base-url http://<gpuf-s-host>:18080
```

如果 gpuf-s 控制面使用 TLS：

```bash
target/release/gpuf-p2p-proxy \
  --listen 127.0.0.1:18088 \
  --server-addr <gpuf-s-host> \
  --control-port 17000 \
  --control-tls \
  --control-tls-server-name <tls-server-name> \
  --cert-chain-path ca-cert.pem \
  --consumer-id <consumer-id-32-hex> \
  --target-client-id <target-gpuf-c-client-id-32-hex> \
  --fallback-base-url https://<gpuf-s-api-host>
```

`consumer-id` 是本地 proxy/插件/SDK 的 consumer 会话 ID，不需要存在于 `gpu_assets`，也不会把 proxy 标记为在线算力设备。建议使用随机 32 位 hex ID，不能和在线 `gpuf-c` 的 client id 冲突。实际鉴权使用每个 API 请求里的 `Authorization: Bearer <token>`，gpuf-s 会按 token 可访问的 client 列表校验目标 `gpuf-c`。

## 请求方式

用户侧把 OpenAI SDK 的 `base_url` 改成本地 proxy：

```text
http://127.0.0.1:18088/v1
```

curl 示例：

```bash
curl -s http://127.0.0.1:18088/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer <api-token>' \
  -H 'x-target-client-id: <target-gpuf-c-client-id-32-hex>' \
  -d '{
    "model": "gpuf",
    "messages": [
      {"role": "user", "content": "只回复 ok"}
    ],
    "max_tokens": 32,
    "temperature": 0
  }'
```

如果 P2P 成功，响应会带：

```text
x-gpuf-p2p: direct
```

响应体会包含：

```json
{
  "object": "chat.completion",
  "client_id": "<target-client-id>",
  "p2p": {
    "enabled": true,
    "transport": "udp",
    "fallback": false
  }
}
```

## Fallback 行为

配置了 `--fallback-base-url` 后，以下情况会转发到现有 gpuf-s 分享 API：

- 未指定目标 client id；
- 缺少 `Authorization: Bearer <token>`；
- consumer token 无权访问目标 client；
- P2P 建连失败；
- P2P UDP 超时；
- 目标节点不在线；
- `stream=true`；
- 请求包含图片、多模态内容；
- 请求的是 embeddings/models/completions 等 MVP 尚未 P2P 化的路径。

fallback 会保留原始请求体和大部分请求头，包括 `Authorization`、`x-target-client-id`、`x-request-id`。

## 统计与防伪造口径

P2P usage 统计遵循“只统计真实完成的 P2P，不让 proxy 单边伪造”的规则：

- fallback 请求不走 P2P usage report，继续由现有 gpuf-s HTTP gateway 写入 `inference_token_usage`；
- proxy 只有在收到 `P2PInferenceDone` 后才发送 `P2PUsageReport`；
- gpuf-s 不信任 proxy 上传的 token_hash 或任意 client 归属，token_hash 由 consumer 登录时的 Bearer token 在 gpuf-s 端计算并绑定；
- gpuf-s 会校验 consumer session、`consumer_id`、目标 `target_client_id`、token 可访问 client 列表，以及 gpuf-s 曾签发过的 P2P `connection_id`；
- 目标 `gpuf-c` 会在发送 Done 前通过控制面发送 `P2PUsageReceipt`；
- gpuf-s 只有在 consumer report 和 target receipt 的 `task_id`、输出 `sha256`、prompt/completion/total/analysis/final token 计数一致时，才写入成功 usage；
- `request-id` 和 `x-request-id` 都会作为去重键来源，最终仍使用 `inference_token_usage` 现有的 `(request_id, token_hash, endpoint)` 去重；
- 如果 receipt 缺失、ownership 不匹配、token 绑定不匹配、输出 hash 不一致或 token 计数不一致，gpuf-s 不写成功 token 统计。

当前文本 P2P 路径使用轻量 token 估算作为过渡口径；proxy 和 `gpuf-c` 使用同一规则，所以可以做双端一致性校验。后续如果底层推理引擎返回更精确 usage，应让 `gpuf-c` 的 receipt 和 proxy report 同步切换到同一精确口径。

### stream / OCR / 图片请求

当前 `stream=true`、OCR、vision、多模态图片请求仍 fallback 到 gpuf-s HTTP gateway，因此统计仍由 HTTP gateway 负责，不会额外产生 P2P usage report。

后续真正启用 P2P stream 或 OCR 文件传输时，必须复用同一套规则：

- stream：只在收到最终 Done/Finish 后落一次 usage，不能按 chunk 重复落库；
- OCR/图片：文件传输阶段只记录传输审计指标，模型结果完成并通过 report/receipt 匹配后才记录 token usage；
- fallback：只要最后走了 HTTP fallback，就不能发送 P2P token usage report，避免双记；
- 中途失败/取消：不得写成功 usage；如要审计失败，应写失败事件或 success=false 记录，不进入成功 token 汇总。

## 设计边界

第一版是“轻量转发客户端”，不是模型 SDK，也不是算力节点：

- 不加载模型；
- 不做本地推理；
- 不暴露公网监听，默认只建议监听 `127.0.0.1`；
- 不绕过 gpuf-s 鉴权和计费，fallback 仍走现有分享 API；
- P2P 只作为数据面优化，控制面仍由 gpuf-s 负责。

## 后续实现计划

1. 把 P2P UDP/TURN 公共逻辑从 `gpuf-p2p-proxy` 和 `gpuf-c` 抽到 common，减少重复。
2. 实现 P2P streaming，将 chunk 直接转成 OpenAI SSE。
3. 实现 OCR 文件 P2P，将 OpenAI multimodal content 中的 image/data URL 转为 `P2PFile*` 文件块。
4. 根据模型 capability 自动选择目标节点，减少必须传 `x-target-client-id` 的场景。
