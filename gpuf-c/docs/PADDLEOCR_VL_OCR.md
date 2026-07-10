# PaddleOCR-VL OCR Multimodal

`gpuf-c` can run PaddleOCR-VL GGUF models in standalone OpenAI-compatible mode through llama.cpp mtmd.

## Model Files

Download both files from ModelScope:

- `PaddleOCR-VL-1.6-GGUF.gguf`
- `PaddleOCR-VL-1.6-GGUF-mmproj.gguf`

The main GGUF is passed with `--llama-model-path`; the projector GGUF is passed with
`--llama-mmproj-path`.

## Start gpuf-c

### Linux CUDA 13

CUDA 13 no longer accepts old default architectures such as `sm_52`. Set the
CUDA compiler and architecture explicitly when building on Ada GPUs:

```bash
export CUDA_HOME=/usr/local/cuda-13.0
export CUDA_PATH=/usr/local/cuda-13.0
export CUDACXX=/usr/local/cuda-13.0/bin/nvcc
export CMAKE_CUDA_COMPILER=/usr/local/cuda-13.0/bin/nvcc
export CMAKE_CUDA_ARCHITECTURES=89
export PATH=/usr/local/cuda-13.0/bin:$PATH

cargo build -p gpuf-c --no-default-features --features cuda --release
```

```bash
./target/release/gpuf-c \
  --standalone-llama \
  --engine-type llama \
  --llama-model-path /models/PaddleOCR-VL-1.6-GGUF.gguf \
  --llama-mmproj-path /models/PaddleOCR-VL-1.6-GGUF-mmproj.gguf \
  --local-addr 127.0.0.1 \
  --local-port 8080 \
  --n-gpu-layers 99 \
  --n-ctx 8192 \
  --n-batch 4096
```

### macOS Metal

Build with Metal support on macOS:

```bash
cargo build -p gpuf-c --no-default-features --features metal --release
```

Use the same runtime arguments as above. Metal, CUDA, CPU, and Vulkan all use the
same `llama.cpp` mtmd path in `gpuf-c`; only the build feature changes.

### Linux Vulkan

```bash
cargo build -p gpuf-c --no-default-features --features vulkan --release
```

There is currently no `opencl` feature in the `llama-cpp-2` version used by
`gpuf-c`; Linux OpenCL is therefore not a supported PaddleOCR-VL target in this
branch. Use CUDA for NVIDIA or Vulkan for cross-vendor Linux builds.

For OCR, use deterministic sampling:

```bash
export GPUF_MAX_MAX_TOKENS=4096
```

Then send requests with `temperature: 0`.

## PaddleOCR CLI

```bash
paddleocr doc_parser \
  -i https://paddle-model-ecology.bj.bcebos.com/paddlex/imgs/demo_image/paddleocr_vl_demo.png \
  --pipeline_version v1.6 \
  --vl_rec_backend llama-cpp-server \
  --vl_rec_server_url http://127.0.0.1:8080/v1
```

## OpenAI Vision Request

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "PaddleOCR-VL-1.6-GGUF",
    "messages": [
      {
        "role": "user",
        "content": [
          {"type": "text", "text": "OCR:"},
          {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
        ]
      }
    ],
    "max_tokens": 1024,
    "temperature": 0
  }'
```

Supported image sources:

- `data:` URLs
- `http://` and `https://` URLs
- `file://` URLs only when `GPUF_ALLOW_FILE_IMAGE_URLS=1`

Useful limits:

- `GPUF_REQUEST_BODY_LIMIT_BYTES`, default `33554432`
- `GPUF_MAX_IMAGE_BYTES`, default `33554432`
- `GPUF_MAX_IMAGES_PER_REQUEST`, default `8`

## Notes

- The ModelScope files verified for this flow are:
  - `PaddleOCR-VL-1.6-GGUF.gguf`
  - `PaddleOCR-VL-1.6-GGUF-mmproj.gguf`
- PaddleOCR-VL may log a chat-template fallback such as `FfiError(-1)`. The
  fallback prompt still works with mtmd; a successful run should show
  `projector: paddleocr` and return OCR text plus `<|LOC_...|>` location tokens.
