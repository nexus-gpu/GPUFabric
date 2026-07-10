# Linux 本机构建 GPUFabric gpuf-c 流程

本文档记录在 Linux 环境构建 GPUFabric `gpuf-c` CUDA 13 / Vulkan 版本的流程。它对应 Windows 10 KVM 构建文档 `/home/jack/桌面/working/win10-gpuf-c-build-flow.md`，用于以后更新 Linux 客户端包和上传 OSS 前复现构建、测试、打包。

## 概览

- 代码库路径：`/home/jack/codedir/GPUFabric`
- Cargo package：`gpuf-c`
- 当前版本：`gpuf-c/Cargo.toml` 中 `version = "1.0.4"`
- Linux 构建目标：`x86_64-unknown-linux-gnu`
- CUDA Toolkit：`/usr/local/cuda-13.0`
- CUDA 产物：`target/release/gpuf-c-cuda`
- Vulkan 产物：`target/release/gpuf-c-vulkan`
- 建议 CUDA target 目录：`/tmp/gpuf-target-linux-cuda13`
- 建议 Vulkan target 目录：`/tmp/gpuf-target-linux-vulkan`
- Linux 线上包目录：`/home/jack/桌面/working/client-pack/v1.0.4-linux-gpuf-c`

`gpuf-c` 的 `cuda` 和 `vulkan` feature 当前都会自动启用 `multimodal`，所以 CUDA/Vulkan 产物都支持 PaddleOCR-VL 这类需要 `--llama-mmproj-path` 的 OCR 多模态模型。

## 为什么分开 target 目录

CUDA 和 Vulkan 都会触发 `llama.cpp`/`llama-cpp-2` 的 native 构建。如果共用默认 `target/`，容易出现 feature 缓存、增量构建残留或排查困难的问题。实际构建时建议每个后端使用独立的 `CARGO_TARGET_DIR`：

```bash
/tmp/gpuf-target-linux-cuda13
/tmp/gpuf-target-linux-vulkan
```

构建完成后，再把最终二进制复制到仓库统一输出目录：

```bash
/home/jack/codedir/GPUFabric/target/release/gpuf-c-cuda
/home/jack/codedir/GPUFabric/target/release/gpuf-c-vulkan
```

## Linux 构建依赖

基础依赖：

```bash
sudo apt-get update
sudo apt-get install -y build-essential clang cmake ninja-build pkg-config libssl-dev git curl
```

Vulkan 构建和运行依赖：

```bash
sudo apt-get install -y libvulkan-dev vulkan-tools
```

CUDA 构建和运行依赖：

```text
NVIDIA driver：运行时需要 libcuda.so.1 和 libnvidia-ml.so.1
CUDA Toolkit：构建时需要 CUDA 13.0 的 nvcc、headers 和 runtime
```

检查环境：

```bash
rustc -V
cargo -V
cmake --version
ninja --version
pkg-config --version

nvidia-smi
/usr/local/cuda-13.0/bin/nvcc --version
vulkaninfo --summary
```

如果构建机需要代理，先设置代理：

```bash
export https_proxy=http://127.0.0.1:7897
export http_proxy=http://127.0.0.1:7897
export all_proxy=socks5://127.0.0.1:7897
```

## 构建前检查

进入仓库根目录：

```bash
cd /home/jack/codedir/GPUFabric
```

确认版本号：

```bash
rg -n '^version = ' gpuf-c/Cargo.toml
```

确认 feature：

```bash
rg -n '^(multimodal|cuda|vulkan) =' gpuf-c/Cargo.toml
```

确认工作区当前分支和变更：

```bash
git status --short --branch
```

## 构建 CUDA 13 版本

设置 CUDA 13 环境变量：

```bash
export CUDA_HOME=/usr/local/cuda-13.0
export CUDA_PATH=/usr/local/cuda-13.0
export CUDACXX=/usr/local/cuda-13.0/bin/nvcc
export CMAKE_CUDA_COMPILER=/usr/local/cuda-13.0/bin/nvcc
export PATH=/usr/local/cuda-13.0/bin:$PATH
export LD_LIBRARY_PATH=/usr/local/cuda-13.0/lib64:${LD_LIBRARY_PATH:-}
```

CUDA 13 不再接受一些旧默认架构，例如 `sm_52`。在 z370 / RTX 4090 这类 Ada 机器上，显式指定：

```bash
export CMAKE_CUDA_ARCHITECTURES=89
```

如果是 H100/H800 这类 Hopper 机器，通常改为：

```bash
export CMAKE_CUDA_ARCHITECTURES=90
```

开始构建：

```bash
CARGO_TARGET_DIR=/tmp/gpuf-target-linux-cuda13 \
cargo build -j "$(nproc)" -p gpuf-c --bin gpuf-c --release --no-default-features --features cuda
```

复制最终产物：

```bash
install -m 0755 /tmp/gpuf-target-linux-cuda13/release/gpuf-c \
  /home/jack/codedir/GPUFabric/target/release/gpuf-c-cuda
```

基础验收：

```bash
target/release/gpuf-c-cuda --version
file target/release/gpuf-c-cuda
ldd target/release/gpuf-c-cuda
sha256sum target/release/gpuf-c-cuda
```

运行 CUDA 版时，机器上必须有 NVIDIA driver 提供的 `libcuda.so.1`、`libnvidia-ml.so.1`，以及 CUDA 13 runtime `libcudart.so.13`。如果 `ldd` 显示 `libcudart.so.13 => not found`，需要设置：

```bash
export LD_LIBRARY_PATH=/usr/local/cuda-13.0/lib64:${LD_LIBRARY_PATH:-}
```

## 构建 Vulkan 版本

Vulkan 构建不需要 CUDA 环境变量。为了避免 CUDA 变量影响排查，可以新开 shell，或手动清理：

```bash
unset CUDA_HOME
unset CUDA_PATH
unset CUDACXX
unset CMAKE_CUDA_COMPILER
unset CMAKE_CUDA_ARCHITECTURES
```

确认 Vulkan loader 可用：

```bash
vulkaninfo --summary
```

开始构建：

```bash
CARGO_TARGET_DIR=/tmp/gpuf-target-linux-vulkan \
cargo build -j "$(nproc)" -p gpuf-c --bin gpuf-c --release --no-default-features --features vulkan
```

复制最终产物：

```bash
install -m 0755 /tmp/gpuf-target-linux-vulkan/release/gpuf-c \
  /home/jack/codedir/GPUFabric/target/release/gpuf-c-vulkan
```

基础验收：

```bash
target/release/gpuf-c-vulkan --version
file target/release/gpuf-c-vulkan
ldd target/release/gpuf-c-vulkan
sha256sum target/release/gpuf-c-vulkan
```

Vulkan 运行时需要系统有 `libvulkan.so.1` 和可用的 GPU ICD。如果 `vulkaninfo --summary` 失败，先修复驱动/ICD，再测试 `gpuf-c-vulkan`。

## 本地 standalone smoke test

Embedding 模型快速启动示例：

```bash
GPUF_MAX_MAX_TOKENS=4096 \
target/release/gpuf-c-vulkan \
  --standalone-llama \
  --engine-type llama \
  --llama-model-path /home/jack/下载/bge-m3-Q8_0.gguf \
  --local-addr 127.0.0.1 \
  --local-port 18080 \
  --n-gpu-layers 99 \
  --n-ctx 8192 \
  --n-batch 4096
```

另一个终端检查 OpenAI-compatible 路由：

```bash
curl -s http://127.0.0.1:18080/v1/models
```

PaddleOCR-VL OCR 多模态启动示例：

```bash
GPUF_MAX_MAX_TOKENS=4096 \
target/release/gpuf-c-cuda \
  --standalone-llama \
  --engine-type llama \
  --llama-model-path /tmp/paddleocr-vl-gguf/PaddleOCR-VL-1.6-GGUF.gguf \
  --llama-mmproj-path /tmp/paddleocr-vl-gguf/PaddleOCR-VL-1.6-GGUF-mmproj.gguf \
  --local-addr 127.0.0.1 \
  --local-port 18080 \
  --n-gpu-layers 99 \
  --n-ctx 8192 \
  --n-batch 4096
```

OCR 请求使用 OpenAI Vision 的多模态数组格式：

```bash
curl -s http://127.0.0.1:18080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "PaddleOCR-VL-1.6-GGUF",
    "messages": [
      {
        "role": "user",
        "content": [
          {"type": "text", "text": "OCR this image. Return the visible text and layout hints."},
          {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
        ]
      }
    ],
    "max_tokens": 1024,
    "temperature": 0
  }'
```

## z370 测试流程

把本地 CUDA 产物拉到 z370 测试：

```bash
ssh z370 'mkdir -p /tmp/gpuf-c-linux-1.0.4'
scp target/release/gpuf-c-cuda z370:/tmp/gpuf-c-linux-1.0.4/gpuf-c-cuda
ssh z370 'chmod +x /tmp/gpuf-c-linux-1.0.4/gpuf-c-cuda'
```

在 z370 检查 CUDA 运行环境：

```bash
ssh z370 'nvidia-smi'
ssh z370 '/usr/local/cuda-13.0/bin/nvcc --version'
ssh z370 'LD_LIBRARY_PATH=/usr/local/cuda-13.0/lib64:${LD_LIBRARY_PATH:-} /tmp/gpuf-c-linux-1.0.4/gpuf-c-cuda --version'
```

在 z370 启动 PaddleOCR-VL：

```bash
ssh z370 'CUDA_HOME=/usr/local/cuda-13.0 CUDA_PATH=/usr/local/cuda-13.0 LD_LIBRARY_PATH=/usr/local/cuda-13.0/lib64:${LD_LIBRARY_PATH:-} GPUF_MAX_MAX_TOKENS=4096 /tmp/gpuf-c-linux-1.0.4/gpuf-c-cuda --standalone-llama --engine-type llama --llama-model-path /tmp/paddleocr-vl-gguf/PaddleOCR-VL-1.6-GGUF.gguf --llama-mmproj-path /tmp/paddleocr-vl-gguf/PaddleOCR-VL-1.6-GGUF-mmproj.gguf --local-addr 127.0.0.1 --local-port 18080 --n-gpu-layers 99 --n-ctx 8192 --n-batch 4096'
```

如果要让本地访问 z370 的 standalone 服务，可以在本地开 SSH 隧道：

```bash
ssh -L 18080:127.0.0.1:18080 z370
```

然后本地请求：

```bash
curl -s http://127.0.0.1:18080/v1/models
```

## GPUFabric TLS worker 启动

线上包里的脚本使用环境变量启动，不在包里保存 token、私钥或 client credential。

CUDA：

```bash
export GPUF_SERVER_ADDR=agent.gpunexus.com
export GPUF_CLIENT_ID=<client-id-32-hex>
export GPUF_CONTROL_TLS_SERVER_NAME=agent.gpunexus.com
export GPUF_MODEL_PATH=/models/model.gguf

./start-gpuf-c-cuda-tls.sh
```

Vulkan：

```bash
export GPUF_SERVER_ADDR=agent.gpunexus.com
export GPUF_CLIENT_ID=<client-id-32-hex>
export GPUF_CONTROL_TLS_SERVER_NAME=agent.gpunexus.com
export GPUF_MODEL_PATH=/models/model.gguf

./start-gpuf-c-vulkan-tls.sh
```

如需 OCR，多模态 projector 通过额外参数传入：

```bash
./start-gpuf-c-cuda-tls.sh \
  --engine-type llama \
  --llama-mmproj-path /models/PaddleOCR-VL-1.6-GGUF-mmproj.gguf \
  --n-gpu-layers 99 \
  --n-ctx 8192 \
  --n-batch 4096
```

## 更新 Linux client-pack

准备包目录：

```bash
PACK=/home/jack/桌面/working/client-pack/v1.0.4-linux-gpuf-c
mkdir -p "$PACK"
```

用当前提交短 SHA 命名二进制，和现有线上包保持一致：

```bash
SHORT_SHA="$(git rev-parse --short=6 HEAD)"
install -m 0755 target/release/gpuf-c-cuda "$PACK/${SHORT_SHA}-cuda-gpuf-c"
install -m 0755 target/release/gpuf-c-vulkan "$PACK/${SHORT_SHA}-vulkan-gpuf-c"
```

确认启动脚本和 CA：

```bash
test -f "$PACK/start-gpuf-c-cuda-tls.sh"
test -f "$PACK/start-gpuf-c-vulkan-tls.sh"
test -f "$PACK/ca-cert.pem"
```

重新生成校验文件：

```bash
cd "$PACK"
sha256sum * > SHA256SUMS
```

打包：

```bash
cd /home/jack/桌面/working/client-pack
tar -czf v1.0.4-linux-gpuf-c.tar.gz v1.0.4-linux-gpuf-c
sha256sum v1.0.4-linux-gpuf-c.tar.gz
```

上传 OSS 前检查：

```bash
tar -tf /home/jack/桌面/working/client-pack/v1.0.4-linux-gpuf-c.tar.gz
cat /home/jack/桌面/working/client-pack/v1.0.4-linux-gpuf-c/SHA256SUMS
```

## 常见问题

### CUDA 13 报不支持旧架构

现象类似 `Unsupported gpu architecture 'compute_52'`。处理方式是显式设置：

```bash
export CMAKE_CUDA_ARCHITECTURES=89
```

不同 GPU 需要不同架构值，Ada/RTX 4090 常用 `89`，Hopper/H100 常用 `90`。

### 找不到 CUDA runtime

如果 `ldd gpuf-c-cuda` 显示：

```text
libcudart.so.13 => not found
```

设置：

```bash
export LD_LIBRARY_PATH=/usr/local/cuda-13.0/lib64:${LD_LIBRARY_PATH:-}
```

如果缺少 `libcuda.so.1` 或 `libnvidia-ml.so.1`，说明 NVIDIA driver 侧不完整，不能只靠 CUDA Toolkit 解决。

### Vulkan loader 或 ICD 不可用

如果 `vulkaninfo --summary` 失败，先检查：

```bash
ldconfig -p | rg 'libvulkan.so.1'
ls /usr/share/vulkan/icd.d
```

需要安装 Vulkan loader、对应 GPU 驱动和 ICD 文件。

### OCR 请求提示需要 mmproj

PaddleOCR-VL 等视觉模型必须在启动 `gpuf-c` 时传入匹配的：

```bash
--llama-mmproj-path /models/PaddleOCR-VL-1.6-GGUF-mmproj.gguf
```

只传主模型 `--llama-model-path` 会导致图片请求失败。

### `UnexpectedVariant found: 11`

这是旧版 worker 二进制解析当前多模态任务结构时可能出现的问题。处理方式是使用当前分支重新构建的 `gpuf-c`，并确认版本为 `1.0.4`。

### 文本 prompt 返回 `okok...`

之前 P2P OCR 测试里出现过类似输出。这通常表示请求走通了 P2P/推理链路，但输入不是有效 OCR 图片，不能代表 OCR 识别质量。OCR 验收需要使用 OpenAI Vision 多模态数组，并传入真实图片。

## 当前线上包参考

当前已有 Linux 包目录：

```text
/home/jack/桌面/working/client-pack/v1.0.4-linux-gpuf-c
```

包内当前形态：

```text
*-cuda-gpuf-c
*-vulkan-gpuf-c
ca-cert.pem
start-gpuf-c-cuda-tls.sh
start-gpuf-c-vulkan-tls.sh
read.txt
SHA256SUMS
```

发布前需要确认 `read.txt` 中的 Package/version 文案和目录版本一致，避免目录是 `v1.0.4` 但文案仍写旧版本。
