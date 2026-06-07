# GPUFabric iOS SDK Integration

This document describes the iOS C API and the expected integration flow for
`gpuf_c_sdk.xcframework`.

## Build

Build llama.cpp iOS libraries first, then build the SDK:

```bash
bash generate_llama_ios.sh
bash generate_ios_sdk.sh
```

The SDK is generated at:

```text
build_ios/dist/gpuf_c_sdk.xcframework
```

Expected slices:

```text
ios-arm64/libgpuf_c_sdk.a
ios-arm64-simulator/libgpuf_c_sdk.a
```

Both slices intentionally use the same static library name so Xcode, CocoaPods,
and Swift package consumers can select the right slice without changing linker
settings between device and simulator builds.

## Linker Requirements

Add `gpuf_c_sdk.xcframework` to the iOS target and link these Apple frameworks:

```text
Metal.framework
Accelerate.framework
```

Use the shipped `Headers/gpuf_c.h` from the selected XCFramework slice. The
iOS header is pure C and does not expose Android/JNI types such as `JNIEnv`,
`jstring`, `jobject`, or `jclass`.

## Local Inference API

The local LLM API is a plain C API and does not require Android or JNI types.

```c
int gpuf_init(void);
int gpuf_cleanup(void);
const char *gpuf_version(void);
const char *gpuf_system_info(void);

struct llama_model *gpuf_load_model(const char *model_path);
struct llama_context *gpuf_create_context(struct llama_model *model);

int gpuf_generate_final_solution_text(
    const struct llama_model *model,
    struct llama_context *context,
    const char *prompt,
    int max_tokens,
    char *output_buffer,
    int output_buffer_size
);

void llama_model_free(struct llama_model *model);
void llama_free(struct llama_context *context);
```

Recommended order:

```c
gpuf_init();
struct llama_model *model = gpuf_load_model(model_path);
struct llama_context *ctx = gpuf_create_context(model);
gpuf_generate_final_solution_text(model, ctx, prompt, max_tokens, output, output_size);
llama_free(ctx);
llama_model_free(model);
gpuf_cleanup();
```

## Remote Worker API

The legacy Remote Worker API remains compatible:

```c
int set_remote_worker_model(const char *model_path);
int start_remote_worker(
    const char *server_addr,
    int control_port,
    int proxy_port,
    const char *worker_type,
    const char *client_id
);
int start_remote_worker_tasks(void);
int start_remote_worker_tasks_with_callback_ptr(void (*callback)(const char *, void *));
int get_remote_worker_status(char *buffer, size_t buffer_size);
int stop_remote_worker(void);
```

The preferred iOS callback API is:

```c
typedef void (*gpuf_status_callback)(const char *message, void *user_data);

int gpuf_register_remote_worker_callback(
    gpuf_status_callback callback,
    void *user_data
);
```

Recommended iOS Remote Worker order:

```c
set_remote_worker_model(model_path);
start_remote_worker(server_addr, control_port, proxy_port, "TCP", client_id);
gpuf_register_remote_worker_callback(callback, user_data);
start_remote_worker_tasks();
```

`start_remote_worker_tasks_with_callback_ptr` is kept for backward
compatibility, but iOS and Objective-C++ callers should prefer
`gpuf_register_remote_worker_callback`.

## TLS Optional API

Plaintext control connections can continue using `start_remote_worker`.
TLS is opt-in:

```c
int gpuf_validate_mobile_tls_policy(
    const char *ca_cert_path,
    const char *control_tls_server_name,
    const char *cert_sha256_pin
);

int start_remote_worker_with_tls(
    const char *server_addr,
    int control_port,
    int proxy_port,
    const char *worker_type,
    const char *client_id,
    const char *ca_cert_path,
    const char *control_tls_server_name,
    const char *cert_sha256_pin
);
```

If the test or production server still uses plaintext, frontend code does not
need to call TLS APIs.

## Swift Callback Example

```swift
private func remoteWorkerCallback(
    _ message: UnsafePointer<CChar>?,
    _ userData: UnsafeMutableRawPointer?
) {
    guard let message else { return }
    let text = String(cString: message)
    print(text)
}

let cb: (@convention(c) (UnsafePointer<CChar>?, UnsafeMutableRawPointer?) -> Void) =
    remoteWorkerCallback

gpuf_register_remote_worker_callback(cb, nil)
start_remote_worker_tasks()
```

## Compatibility Notes

## Multimodal Status

The iOS archive may include `libmtmd.a` when llama.cpp was built with mtmd, but
the current iOS C implementation still returns unsupported results for
multimodal entry points:

```c
gpuf_load_multimodal_model(...)        // returns NULL on iOS
gpuf_generate_multimodal(...)          // returns -1 on iOS
gpuf_multimodal_supports_vision(...)   // returns false on iOS
```

Use the current iOS SDK for text local inference and Remote Worker compute
sharing. Treat iOS multimodal as not implemented until those C functions return
real model/context handles.

## Compatibility Notes

Existing local inference calls remain source compatible.
Existing Remote Worker calls remain source compatible.
`start_remote_worker_tasks_with_callback_ptr` remains available, now declared as
a typed C callback in iOS headers.
TLS APIs are additive.
`gpuf_register_remote_worker_callback` is additive and replaces the need for
Swift-side function-pointer integer casts.
