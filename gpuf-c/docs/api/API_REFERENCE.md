# 📖 API Reference

## 🎯 Overview

GPUFabric provides multi-language APIs supporting Rust, Java, and other language integrations. This document details all available API interfaces.

## Security Compatibility Note

The 2026-06-04 remediation pass does not remove or rename existing public Rust/C/JNI APIs documented here. Native and mobile wrappers remain source/ABI compatible at the existing function-signature level. Integrations should update configuration, not old method calls: pass explicit remote server addresses, verify SDK artifacts with SHA256, and avoid relying on deprecated insecure defaults. Mobile control TLS is exposed through additive APIs (`start_remote_worker_with_tls` / `startRemoteWorkerWithTls`) so existing plaintext integrations continue to build while production deployments can opt into TLS.

## 🦀 Rust API

### Core Initialization

#### `init()`
```rust
pub fn init() -> Result<(), GpuFabricError>
```
Initializes the GPUFabric library.

**Return Values:**
- `Ok(())`: Initialization successful
- `Err(GpuFabricError)`: Initialization failed

**Example:**
```rust
use gpuf_c::init;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init()?;
    println!("GPUFabric initialized successfully");
    Ok(())
}
```

#### `cleanup()`
```rust
pub fn cleanup() -> Result<(), GpuFabricError>
```
Cleans up GPUFabric resources.

**Example:**
```rust
use gpuf_c::{init, cleanup};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init()?;
    // ... use GPUFabric ...
    cleanup()?;
    Ok(())
}
```

### Device Information

#### `collect_device_info()`
```rust
pub async fn collect_device_info() -> Result<DeviceInfo, GpuFabricError>
```
Collects comprehensive device information.

**Return Values:**
- `Ok(DeviceInfo)`: Device information structure
- `Err(GpuFabricError)`: Collection failed

**Example:**
```rust
use gpuf_c::collect_device_info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device_info = collect_device_info().await?;
    println!("Device: {}", device_info.device_name);
    Ok(())
}
```

#### `get_device_info_cached()`
```rust
pub fn get_device_info_cached() -> Result<DeviceInfo, GpuFabricError>
```
Gets cached device information (5-minute cache).

**Example:**
```rust
use gpuf_c::get_device_info_cached;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device_info = get_device_info_cached()?;
    println!("Cached device info available");
    Ok(())
}
```

### LLM Functions

#### `gpuf_llm_init()`
```rust
pub unsafe fn gpuf_llm_init(
    model_path: *const c_char,
    context_size: i32,
    gpu_layers: i32
) -> i32
```
Initializes LLM model.

**Parameters:**
- `model_path`: Path to model file
- `context_size`: Context window size
- `gpu_layers`: Number of GPU layers

**Return Values:**
- `0`: Success
- `non-zero`: Error code

**Example:**
```rust
use gpuf_c::gpuf_llm_init;
use std::ffi::CString;

fn main() {
    let model_path = CString::new("model.gguf").unwrap();
    unsafe {
        let result = gpuf_llm_init(model_path.as_ptr(), 2048, 32);
        if result == 0 {
            println!("Model initialized successfully");
        }
    }
}
```

#### `gpuf_llm_generate()`
```rust
pub unsafe fn gpuf_llm_generate(
    prompt: *const c_char,
    max_tokens: i32
) -> *mut c_char
```
Generates text from prompt.

**Parameters:**
- `prompt`: Input prompt
- `max_tokens`: Maximum tokens to generate

**Return Values:**
- `*mut c_char`: Generated text (must be freed)
- `null`: Generation failed

**Example:**
```rust
use gpuf_c::{gpuf_llm_init, gpuf_llm_generate, gpuf_llm_free_string};
use std::ffi::CString;

fn main() {
    let model_path = CString::new("model.gguf").unwrap();
    let prompt = CString::new("Hello, world!").unwrap();
    
    unsafe {
        gpuf_llm_init(model_path.as_ptr(), 2048, 32);
        let result = gpuf_llm_generate(prompt.as_ptr(), 100);
        if !result.is_null() {
            let response = CString::from_raw(result);
            println!("Generated: {}", response.to_string_lossy());
        }
    }
}
```

#### `gpuf_llm_free_string()`
```rust
pub unsafe fn gpuf_llm_free_string(ptr: *mut c_char)
```
Frees string allocated by LLM functions.

**Example:**
```rust
use gpuf_c::{gpuf_llm_generate, gpuf_llm_free_string};

unsafe {
    let result = gpuf_llm_generate(prompt.as_ptr(), 100);
    if !result.is_null() {
        // Use result...
        gpuf_llm_free_string(result);
    }
}
```

## ☕ Java API

### Core Classes

#### `GPUFabricClientSDK`
```java
public class GPUFabricClientSDK {
    public boolean init();
    public boolean cleanup();
    public boolean registerDevice();
    public DeviceInfo getDeviceInfo();
    // ... LLM methods
}
```

**Example:**
```java
import com.gpufabric.GPUFabricClientSDK;

public class Example {
    public static void main(String[] args) {
        GPUFabricClientSDK sdk = new GPUFabricClientSDK();
        
        if (sdk.init()) {
            System.out.println("SDK initialized successfully");
            
            DeviceInfo info = sdk.getDeviceInfo();
            System.out.println("Device: " + info.getDeviceName());
            
            sdk.cleanup();
        }
    }
}
```

### LLM Methods

#### `initializeModel()`
```java
public boolean initializeModel(String modelPath)
public boolean initializeModel(String modelPath, int contextSize, int gpuLayers)
```
Initializes LLM model.

**Parameters:**
- `modelPath`: Path to model file
- `contextSize`: Context window size (default: 2048)
- `gpuLayers`: Number of GPU layers (default: 0)

**Example:**
```java
GPUFabricClientSDK sdk = new GPUFabricClientSDK();

// Basic initialization
if (sdk.initializeModel("/path/to/model.gguf")) {
    System.out.println("Model loaded");
}

// Advanced initialization
if (sdk.initializeModel("/path/to/model.gguf", 4096, 64)) {
    System.out.println("Model loaded with custom settings");
}
```

#### `generateResponse()`
```java
public String generateResponse(String prompt)
public String generateResponse(String prompt, int maxTokens)
```
Generates text response.

**Example:**
```java
String prompt = "Tell me about artificial intelligence";
String response = sdk.generateResponse(prompt, 150);
System.out.println("AI Response: " + response);
```

#### `generateResponseStream()`
```java
public void generateResponseStream(String prompt, ResponseCallback callback)
```
Generates streaming response.

**Example:**
```java
sdk.generateResponseStream("Hello", new ResponseCallback() {
    @Override
    public void onToken(String token) {
        System.out.print(token);
    }
    
    @Override
    public void onComplete(String fullResponse) {
        System.out.println("\nComplete: " + fullResponse);
    }
    
    @Override
    public void onError(Exception error) {
        System.err.println("Error: " + error.getMessage());
    }
});
```

### Device Management

#### `DeviceInfo`
```java
public class DeviceInfo {
    public String getDeviceName();
    public String getDeviceId();
    public long getTotalMemory();
    public long getAvailableMemory();
    public int getCpuCores();
    public String getGpuName();
    public double getGpuUsage();
    public double getTemperature();
}
```

**Example:**
```java
DeviceInfo info = sdk.getDeviceInfo();
System.out.println("Device: " + info.getDeviceName());
System.out.println("CPU Cores: " + info.getCpuCores());
System.out.println("GPU Usage: " + info.getGpuUsage() + "%");
System.out.println("Temperature: " + info.getTemperature() + "°C");
```

## 📊 Data Structures

### Rust Structures

#### `DeviceInfo`
```rust
pub struct DeviceInfo {
    pub device_name: String,
    pub device_id: String,
    pub total_memory: u64,
    pub available_memory: u64,
    pub cpu_cores: u32,
    pub gpu_name: Option<String>,
    pub gpu_usage: Option<f64>,
    pub temperature: Option<f64>,
    pub platform: Platform,
}

pub enum Platform {
    Windows,
    Linux,
    macOS,
    Android,
}
```

#### `GpuFabricError`
```rust
pub enum GpuFabricError {
    InitializationError(String),
    DeviceInfoError(String),
    LlmError(String),
    NetworkError(String),
    IoError(std::io::Error),
}
```

### Java Classes

#### `ClientConfig`
```java
public class ClientConfig {
    public String serverAddr;
    public int controlPort;
    public int proxyPort;
    public String clientId;
    public String deviceName;
    public boolean autoRegister;
    public int heartbeatIntervalSecs;
    
    // Builder pattern
    public static ClientConfig builder() { ... }
    public ClientConfig serverAddr(String addr) { ... }
    public ClientConfig controlPort(int port) { ... }
    // ... other builders
    public GPUFabricClientSDK build() { ... }
}
```

**Example:**
```java
GPUFabricClientSDK sdk = ClientConfig.builder()
    .serverAddr("<gpuf-s-host>")
    .controlPort(17000)
    .proxyPort(17001)
    .clientId("<client-id-32-hex>")
    .deviceName("My Android Device")
    .autoRegister(true)
    .heartbeatIntervalSecs(30)
    .build();
```

Control connection TLS note: CLI/config deployments can enable `[client].control_tls = true` and `control_tls_server_name`. The public Java/C mobile remote worker plaintext signatures shown here remain unchanged for compatibility. Mobile wrappers can preflight CA/SNI/pin inputs with the policy helper below and can opt into the TLS-wrapped control protocol with the additive `start_remote_worker_with_tls` / `startRemoteWorkerWithTls` APIs. Android arm64 target compile and packaged SDK real-device inference now pass locally with NDK 25.1.8937393; iOS target builds and Android/iOS TLS/pinning handshake logs are still required release evidence.

### Mobile TLS Policy Helper

C API:

```c
int gpuf_validate_mobile_tls_policy(const char *ca_cert_path,
                                    const char *server_name,
                                    const char *cert_sha256_pin);
```

JNI API:

```java
public static native int validateMobileTlsPolicy(
    String caCertPath,
    String serverName,
    String certSha256Pin
);
```

Pass an empty string or `NULL` for `ca_cert_path` / `cert_sha256_pin` when unused. At least one of CA bundle or SHA256 certificate pin must be provided. Return codes: `0` valid, `-1` missing/invalid server name, `-2` missing trust material, `-3` invalid CA bundle, `-4` invalid SHA256 pin, `-5` invalid UTF-8. This helper validates configuration before calling the TLS start API.

### Mobile Remote Worker TLS Start API

C API:

```c
int start_remote_worker_with_tls(const char *server_addr,
                                 int control_port,
                                 int proxy_port,
                                 const char *worker_type,
                                 const char *client_id,
                                 const char *ca_cert_path,
                                 const char *control_tls_server_name,
                                 const char *cert_sha256_pin);
```

JNI API:

```java
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
```

The first five parameters match `start_remote_worker` / `startRemoteWorker`; the final three parameters are TLS trust configuration. Pass an empty CA path when using pin-only trust, or an empty pin when using a CA bundle only. `control_tls_server_name` may be empty to use `server_addr` as SNI, but production wrappers should pass the expected DNS name explicitly. Return codes: `0` success, `-1` required argument/connection/login failure, `-2` invalid TLS policy.

## 🔌 Error Handling

### Rust Error Types
```rust
use gpuf_c::GpuFabricError;

match collect_device_info().await {
    Ok(info) => println!("Device: {}", info.device_name),
    Err(GpuFabricError::InitializationError(msg)) => {
        eprintln!("Initialization failed: {}", msg);
    },
    Err(GpuFabricError::DeviceInfoError(msg)) => {
        eprintln!("Device info collection failed: {}", msg);
    },
    Err(err) => eprintln!("Other error: {:?}", err),
}
```

### Java Exception Handling
```java
try {
    GPUFabricClientSDK sdk = new GPUFabricClientSDK();
    if (sdk.init()) {
        String response = sdk.generateResponse("Hello");
        System.out.println(response);
    }
} catch (GPUFabricException e) {
    System.err.println("GPUFabric error: " + e.getMessage());
} catch (Exception e) {
    System.err.println("General error: " + e.getMessage());
}
```

## 🧪 Testing API

### Test Functions
```rust
// Rust test functions
pub async fn test_device_info_collection() -> Result<(), GpuFabricError>
pub async fn test_llm_inference() -> Result<(), GpuFabricError>
pub async fn test_network_connectivity() -> Result<(), GpuFabricError>
```

### Java Test Methods
```java
// Java test methods
public boolean testDeviceInfo();
public boolean testLlmInference();
public boolean testNetworkConnectivity();
```

**Example:**
```java
// Run all tests
boolean allPassed = true;
allPassed &= sdk.testDeviceInfo();
allPassed &= sdk.testLlmInference();
allPassed &= sdk.testNetworkConnectivity();

if (allPassed) {
    System.out.println("All tests passed!");
} else {
    System.out.println("Some tests failed!");
}
```

## 📝 Best Practices

### Performance Tips
1. **Use cached device info** for frequent calls
2. **Initialize models once** and reuse
3. **Use streaming generation** for long responses
4. **Set appropriate GPU layers** based on device memory

### Memory Management
1. **Call cleanup()** when done
2. **Free strings** allocated by C functions
3. **Monitor memory usage** during inference
4. **Use context size limits** to prevent OOM

### Error Handling
1. **Always check return values**
2. **Handle network timeouts gracefully**
3. **Provide fallback options** for GPU failures
4. **Log errors for debugging**

---

*Last updated: 2025-11-21*
