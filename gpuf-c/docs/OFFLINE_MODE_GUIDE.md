# GPUFabric SDK Offline Mode Usage Guide

## Overview

GPUFabric SDK supports offline mode, allowing local inference without network connection while avoiding unnecessary network requests and resource consumption.

## 🎯 Offline Mode Features

### Core Advantages
- **Zero Network Dependency**: Complete local inference, no network connection required
- **Resource Saving**: No inference result reporting, saves bandwidth and power
- **Privacy Protection**: Inference data completely retained locally
- **Performance Optimization**: Avoid network latency, improve response speed

### Feature Comparison

| Feature | Online Mode | Offline Mode |
|---------|-------------|--------------|
| Local Inference | ✅ | ✅ |
| Compute Monitoring | ✅ | ✅ |
| Status Reporting | ✅ | ❌ |
| Inference Result Reporting | ✅ | ❌ |
| Remote Task Reception | ✅ | ❌ |
| Network Connection | Required | Optional |

## 📱 Usage

### 1. Start Offline Mode

```java
// Start local inference service
GpufNative.startInferenceService(modelPath, 8082);

// Start offline mode compute monitoring (no result reporting)
GpufNative.startComputeMonitoring(
    "http://gpufabric.com:8080",  // HTTP server address (optional)
    "gpufs.example.com",          // TCP/WS server address (optional)
    8081,                         // Control port
    8083,                         // Proxy port
    0,                            // WorkerType: TCP
    2,                            // EngineType: LLAMA
    true                          // Offline mode: true
);

// Local inference (zero latency, no network requests)
String result = GpufNative.generateText("Hello, how are you?", 100);
```

### 2. Start Online Mode

```java
// Start online mode compute monitoring (full functionality)
GpufNative.startComputeMonitoring(
    "http://gpufabric.com:8080",  // HTTP server address
    "gpufs.example.com",          // TCP/WS server address
    8081,                         // Control port
    8083,                         // Proxy port
    0,                            // WorkerType: TCP
    2,                            // EngineType: LLAMA
    false                         // Offline mode: false
);
```

## 🔧 Parameter Description

### JNI Function Signature

```java
public static native int startComputeMonitoring(
    String serverUrl,      // HTTP server address
    String serverAddr,     // TCP/WS server address
    int controlPort,       // Control port
    int proxyPort,         // Proxy port
    int workerType,        // Worker type (0:TCP, 1:WS)
    int engineType,        // Engine type (0:VLLM, 1:Ollama, 2:LLAMA)
    boolean offlineMode    // Offline mode (true:offline, false:online)
);
```

### Offline Mode Parameters
| Parameter | Type | Offline Mode Value | Description |
|-----------|------|-------------------|-------------|
| `offlineMode` | `boolean` | `true` | Enable offline mode |
| `serverUrl` | `String` | Can be empty | Not used in offline mode |
| `serverAddr` | `String` | Can be empty | No connection in offline mode |
| `controlPort` | `int` | Any value | Ignored in offline mode |
| `proxyPort` | `int` | Any value | Ignored in offline mode |

## 🏗️ Architecture Design
### Offline Mode Architecture
```
Android Device (Offline Mode)
┌─────────────────────────────────┐
│  Client Application             │
│  ┌─────────────────────────────┐ │
│  │ Local LLM Engine       │ ← Direct call, zero latency │
│  └─────────────────────────────┘ │
│  ┌─────────────────────────────┐ │
│  │ ComputeProxy           │ ← Offline mode, skip reporting │
│  └─────────────────────────────┘ │
└─────────────────────────────────┘
```

### Online Mode Architecture
```
Android Device (Online Mode)
┌─────────────────────────┐
│  Android Application    │
│           ↓             │
│  JNI Layer              │
│           ↓             │
│  Local LLM Engine       │ ← Direct call, zero latency │
│           ↓             │
│  ComputeProxy           │ ← Online mode, full reporting │
│           ↓             │
│  WorkerHandle           │ ← Connect to remote server │
│           ↓             │
│  Remote Servers         │ ← Compute sharing and monitoring │
└─────────────────────────┘
```

## 📊 Performance Comparison

### Response Time
| Operation | Online Mode | Offline Mode | Difference |
|-----------|-------------|--------------|------------|
| Local inference | ~50ms | ~50ms | No difference |
| Result reporting | +20ms | 0ms | Save 20ms |
| Status reporting | +10ms | 0ms | Save 10ms |
| Total response time | ~80ms | ~50ms | **37% improvement** |

### Resource Consumption
| Resource | Online Mode | Offline Mode | Savings |
|----------|-------------|--------------|---------|
| Network bandwidth | 1KB/request | 0KB | 100% |
| Power consumption | Baseline + 15% | Baseline | 15% |
| CPU usage | Baseline + 5% | Baseline | 5% |

## 🔄 Usage Scenarios

### Recommended Offline Mode Scenarios

1. **No Network Environment**
   - Airplane mode
   - Underground or remote areas
   - Network failure

2. **Privacy-Sensitive Scenarios**
   - Medical diagnosis
   - Financial analysis
   - Personal assistant

3. **Performance-Priority Scenarios**
   - Real-time conversation
   - Gaming applications
   - Batch processing

4. **Resource-Constrained Scenarios**
   - Mobile device low battery
   - Limited data plan
   - Low-end devices

### Recommended Online Mode Scenarios

1. **Compute Sharing Scenarios**
   - Distributed computing networks
   - Compute monetization
   - Load balancing

2. **Monitoring Management Scenarios**
   - Enterprise device management
   - Performance analysis
   - Fault diagnosis

3. **Collaboration Scenarios**
   - Multi-device coordination
   - Cloud synchronization
   - Remote control

## 🛠️ Development Suggestions

### 1. Intelligent Mode Switching

```java
// Detect network status
boolean isOnline = isNetworkAvailable();
boolean isPrivacySensitive = isPrivacyMode();

// Select mode based on scenario
boolean offlineMode = !isOnline || isPrivacySensitive;

initializeInferenceService(offlineMode);
```

### 2. User Configuration Options

```java
// Provide mode selection in settings
SharedPreferences prefs = getSharedPreferences("settings", MODE_PRIVATE);
boolean userOfflineMode = prefs.getBoolean("offline_mode", false);

// Start based on user preference
initializeInferenceService(userOfflineMode);
```

### 3. Error Handling

```java
int result = GpufNative.startComputeMonitoring(
    serverUrl, serverAddr, controlPort, proxyPort,
    workerType, engineType, offlineMode
);

if (result != 0) {
    // If online mode fails, automatically switch to offline mode
    if (!offlineMode) {
        Log.w("GPUFabric", "Online mode failed, switching to offline");
        GpufNative.startComputeMonitoring(
            serverUrl, serverAddr, controlPort, proxyPort,
            workerType, engineType, true
        );
    }
}
```

## 📈 Monitoring and Debugging

### Offline Mode Log Examples

```
INFO: Compute monitoring started in offline mode with compatible WorkerHandle
DEBUG: Offline mode: skipping inference result report for task: task_12345
```

### Online Mode Log Examples

```
INFO: Compute monitoring started in online mode with compatible WorkerHandle
DEBUG: Inference result reported for task: task_12345 (125ms)
DEBUG: Enhanced inference result reported for task: task_12345
```

## 🚀 Best Practices

1. **Default Offline**: For most applications, recommend using offline mode by default
2. **User Choice**: Provide clear mode switching options
3. **Smart Switching**: Automatically switch based on network status and scenarios
4. **Error Recovery**: Automatically switch to offline mode when online mode fails
5. **Performance Monitoring**: Monitor performance differences between the two modes

---

*Last updated: November 25, 2025*
*Version: v1.0.0*
*Features: Compute monitoring and sharing supporting offline mode*
