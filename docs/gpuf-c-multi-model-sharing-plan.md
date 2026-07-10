# gpuf-c 多模型分享计划

## 背景

当前 `gpuf-c` 更接近“一个进程暴露一个主模型/服务”的模式。后续希望一个 `gpuf-c` 节点可以同时对外分享多种能力，例如：

- `bge-m3-Q8_0`：embedding
- `PaddleOCR-VL-1.6-GGUF`：OCR / vision
- `Qwen` / `Llama`：chat completion

目标是让 `gpuf-s` 能按请求里的 `model` 和能力类型选择合适节点，并让 `gpuf-c` 在节点内部按模型路由到对应 engine。

## 目标

第一版目标不是一次性做完整多模型并发调度，而是先做到：

1. 一个 `gpuf-c` 可以上报多个模型。
2. `gpuf-s` 的 `/v1/models`、chat、embeddings、OCR/P2P 调度能识别一个节点上的多个模型。
3. `gpuf-c` 收到任务后可以按 `model` 路由到对应 handler。
4. embedding、OCR、chat 可以在同一个节点上被“分享”，但是否常驻加载由配置决定。

非第一版目标：

- 多个大模型同时高并发推理。
- 完整显存预算调度。
- 跨模型抢占、优先级、自动卸载策略。

这些放到后续阶段。

## 阶段规划

### 阶段 1：多模型上报

范围较小，优先做。

改动点：

- 扩展 `gpuf-c` 模型配置，支持多个 model entry。
- 每个模型上报以下基础字段：
  - `id`
  - `object`
  - `owned_by`
  - `capabilities`
  - `engine_type`
  - `status`
  - `loaded`
  - `context_length`
  - `input_modalities`
  - `output_modalities`
- `gpuf-c -> gpuf-s` 的 model status 上报保留兼容旧 `Vec<Model>`，新增能力字段时优先追加新结构或新版本命令，避免破坏旧 worker。
- `gpuf-s` Redis / active_clients 中保留每个 client 的多个模型。

验收标准：

- 一个 `gpuf-c` 启动后能在 Redis 中看到多个模型。
- `/v1/models` 能返回同一个 client 提供的多个模型。
- 旧版单模型 `gpuf-c` 仍可登录、上报和被调度。

### 阶段 2：单进程多模型路由

范围中等，是第一版可用能力的核心。

改动点：

- `gpuf-c` 内部增加 `ModelRouter` 或等价结构。
- 按请求 `model` 路由到：
  - embedding handler
  - llama chat handler
  - multimodal/OCR handler
  - 外部 OpenAI/Ollama/vLLM proxy handler
- 支持两种加载策略：
  - `resident`：启动时加载并常驻。
  - `lazy`：首次请求时加载，空闲后可卸载。
- 对不支持的输入类型返回明确错误，例如 OCR 模型收到 embeddings 请求应返回 model capability mismatch。

验收标准：

- 同一个 `gpuf-c` 能同时对外提供 `/v1/embeddings` 和 `/v1/chat/completions`。
- `model=bge-m3-Q8_0` 走 embedding。
- `model=PaddleOCR-VL-1.6-GGUF` 走 OCR/multimodal。
- 请求不存在的模型时返回 OpenAI 兼容错误。

### 阶段 3：调度和策略增强

范围中到大，放在第一版稳定后。

改动点：

- `gpuf-s` scheduler 按模型能力、在线状态、负载、显存估算选择节点。
- 增加模型级健康状态：
  - `loading`
  - `loaded`
  - `failed`
  - `evicted`
- 增加模型级并发限制：
  - per-model queue
  - per-client queue
  - max concurrent requests
- 增加节点侧 backpressure，避免多个模型同时抢显存或 KV cache。

验收标准：

- 多个节点提供同一模型时，`gpuf-s` 能选择健康节点。
- 一个节点提供多个模型时，错误不会污染其他模型。
- 模型加载失败后能上报失败状态，并在调度中避开。

### 阶段 4：多模型常驻并发

范围大，属于生产级调度能力。

改动点：

- 多模型同时加载。
- 模型级队列和资源隔离。
- 显存预算和驱逐策略。
- 优先级和限流。
- 统计每个模型的吞吐、错误率、token usage、P2P 文件传输量。

验收标准：

- embedding 小模型常驻。
- OCR 模型可常驻或 lazy。
- 大语言模型按显存策略常驻或卸载。
- 并发请求下不会互相阻塞到不可用。

## 配置设计草案

建议新增一个多模型配置文件，同时保留现有 CLI 参数作为单模型兼容入口。

```toml
[[models]]
id = "bge-m3-Q8_0"
kind = "embedding"
engine = "llama"
model_path = "/models/bge-m3-Q8_0.gguf"
load_policy = "resident"
normalize_default = true

[[models]]
id = "PaddleOCR-VL-1.6-GGUF"
kind = "vision_ocr"
engine = "llama"
model_path = "/models/PaddleOCR-VL-1.6-GGUF.gguf"
mmproj_path = "/models/PaddleOCR-VL-1.6-GGUF-mmproj.gguf"
load_policy = "lazy"
input_modalities = ["text", "image"]
output_modalities = ["text"]

[[models]]
id = "Qwen3-8B"
kind = "chat"
engine = "llama"
model_path = "/models/qwen3-8b.gguf"
load_policy = "lazy"
context_length = 8192
```

兼容策略：

- 如果没有 `[[models]]`，继续使用现有 `--llama-model-path`、`--llama-mmproj-path`、`--engine-type` 等参数。
- 如果同时提供旧参数和 `[[models]]`，优先使用 `[[models]]`，并输出 warning。

## 协议设计草案

现有基础：

- `common::Model`
- `CommandV1::ModelsStatus`
- `active_clients.models`

建议新增：

```rust
pub struct ModelCapability {
    pub id: String,
    pub kind: ModelKind,
    pub engine_type: EngineType,
    pub status: ModelRuntimeStatus,
    pub loaded: bool,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub context_length: Option<u32>,
    pub max_batch_size: Option<u32>,
}
```

兼容方案：

- 短期：继续把 `id/object/owned_by` 写入旧 `Model`，能力字段存 Redis sidecar 或新增 V2 status。
- 中期：新增 `CommandV2::ModelCapabilitiesStatus`。
- 长期：调度器只依赖 capability，而不是只看 model id。

## 与 OCR P2P 的关系

OCR P2P 文件传输是数据面能力；多模型分享是模型注册和路由能力。

建议顺序：

1. 先完成 `P2PFileStart / Chunk / Done` 的 gpuf-s/gpuf-c 运行链路。
2. 同步补模型 capability 字段，至少区分 `embedding`、`chat`、`vision_ocr`。
3. OCR P2P 调度时要求目标模型具备：
   - `kind = vision_ocr`
   - `input_modalities` 包含 `image`
   - `status = loaded` 或可 lazy load

## 测试计划

单元测试：

- 多模型配置解析。
- capability 序列化 roundtrip。
- model kind 和 endpoint 的匹配校验。
- 不支持的 endpoint 返回明确错误。

集成测试：

- 单 `gpuf-c` 上报 embedding + OCR 两个模型。
- `/v1/models` 返回两个模型。
- `/v1/embeddings model=bge-m3-Q8_0` 成功。
- `/v1/chat/completions model=PaddleOCR-VL-1.6-GGUF` 图片 OCR 成功。
- P2P OCR 文件传输只路由到支持 `vision_ocr` 的 worker。

跨平台测试：

- Linux CUDA
- Linux Vulkan/OpenCL/CPU
- macOS Metal
- Windows CUDA/Vulkan

## 风险

- 大模型同时常驻会导致显存不足。
- lazy load 首次请求延迟明显。
- 旧 worker 不认识新协议字段时可能断开，需要版本协商或追加新命令。
- 一个进程内多个 llama context 的线程安全和内存释放要谨慎验证。
- 多模型请求同时进入时，需要清晰的排队和取消语义。

## 当前状态

- 已验证现有 P2P inference 数据面可以本地 SDK 到 z370 Linux CUDA `gpuf-c` 跑通。
- 已验证 PaddleOCR-VL 通过现有 HTTP 多模态链路可返回 OCR 结果。
- 当前分支已新增 OCR P2P 文件传输公共协议类型：
  - `P2PFileStart`
  - `P2PFileChunk`
  - `P2PFileDone`
  - `P2PFileAck`
  - `P2PFileCancel`
- 多模型分享尚未进入实现阶段，本计划作为后续 roadmap。
