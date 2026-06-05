# GPUFabric SDK Compute Sharing Flow Diagrams Documentation

## Overview

This document collection contains complete flow diagrams of GPUFabric SDK compute sharing functionality, showcasing the dual architecture design of local inference and compute sharing.

## 📋 Diagram List

### 1. Main Flow Diagrams

| Filename | Description | Type | Focus |
|----------|-------------|------|-------|
| `sdk-compute-sharing-flow.mmd` | Complete compute sharing flow diagram | Flowchart | Dual architecture, component interaction |
| `sdk-compute-sharing-sequence.mmd` | Compute sharing sequence diagram | Sequence diagram | Sequential interaction, message passing |
| `sdk-compute-sharing-architecture.mmd` | Compute sharing architecture diagram | Architecture diagram | System components, protocol layers |

### 2. Basic Flow Diagrams (Original)

| Filename | Description | Type | Focus |
|----------|-------------|------|-------|
| `sdk-basic-flow.mmd` | Basic local inference flow | Flowchart | Pure local architecture |
| `sdk-basic-sequence.mmd` | Basic local inference sequence | Sequence diagram | JNI call flow |
| `sdk-interaction-flow-fixed.mmd` | Detailed interaction flow diagram | Flowchart | Complete interaction logic |

## 🏗️ Architecture Features

### Dual Architecture Design
```
Local Inference Path:          Compute Sharing Path:
Android App            Android App
    ↓                       ↓
JNI Layer            JNI Layer  
    ↓                       ↓
LLM Engine ←→ ComputeProxy ←→ Remote Servers
(Direct call)        (Compatible WorkerHandle)  (TCP/WS + HTTP)
```

### Core Components
- **Android App**: User interface layer
- **JNI Layer**: Native interface bridge
- **Local LLM Engine**: Zero-latency local inference
- **ComputeProxy**: Compute monitoring and sharing coordinator
- **WorkerHandle**: Compatible with existing communication architecture
- **Remote Servers**: gpuf-s and GPUFabric servers

### Communication Protocols
- **TCP/WS**: Existing CommandV1 protocol compatibility
- **HTTP**: Enhanced monitoring data reporting
- **JSON**: Unified message format

## 📊 Functional Modules

### 1. Local Inference Module
- ✅ Zero-latency direct calls
- ✅ Model loading and management
- ✅ Health status checking
- ✅ Engine lifecycle management

### 2. Compute Monitoring Module
- ✅ Compatible with existing WorkerHandle
- ✅ TCP/WS dual protocol support
- ✅ 120-second heartbeat mechanism
- ✅ Device registration and authentication

### 3. Compute Sharing Module
- ✅ Task reception and distribution
- ✅ Proxy connection management
- ✅ Load balancing support
- ✅ Error recovery mechanism

### 4. Enhanced Monitoring Module
- ✅ HTTP additional monitoring reporting
- ✅ GPU utilization monitoring
- ✅ Memory efficiency statistics
- ✅ Inference performance analysis

## 🔄 Usage Flow

### Initialization Phase
1. Android App startup
2. JNI loads library files
3. Initialize local LLM engine
4. Start compute monitoring and sharing

### Runtime Phase
1. **Local Inference**: Direct calls, zero latency
2. **Compute Sharing**: Receive remote tasks, execute inference
3. **Status Reporting**: Dual monitoring, TCP + HTTP
4. **Task Processing**: Compatible with existing CommandV1 protocol

### Shutdown Phase
1. Stop compute monitoring and sharing
2. Disconnect from remote servers
3. Clean up local LLM engine
4. Release system resources

## 🎯 Technical Advantages

### Compatibility
- 100% compatible with existing WorkerHandle architecture
- Supports both TCP and WebSocket dual protocols
- Complete CommandV1 protocol support

### Performance
- Zero latency for local inference
- Asynchronous monitoring doesn't block main flow
- Intelligent task scheduling and load balancing

### Reliability
- Automatic reconnection mechanism
- Error recovery and retry
- Dual monitoring guarantee

### Extensibility
- Modular design
- Optional enhanced monitoring
- Supports multiple engine types

## 📱 Android Integration Example

```java
// 1. Start local inference
GpufNative.startInferenceService(modelPath, 8082);

// 2. Start compute monitoring and sharing
GpufNative.startComputeMonitoring(
    "https://<your-gpufabric-api>",  // HTTP monitoring server
    "gpufs.example.com",          // TCP/WS server address
    8081,                         // Control port
    8083,                         // Proxy port
    0,                            // WorkerType: TCP
    2                             // EngineType: LLAMA
);

// 3. Local inference (zero latency)
String result = GpufNative.generateText("Hello", 100);

// 4. Background automatic compute sharing and monitoring
```

## 🔧 Configuration Parameters

### WorkerType
- `0`: TCP Worker
- `1`: WebSocket Worker

### EngineType
- `0`: VLLM Engine
- `1`: Ollama Engine
- `2`: LLAMA Engine

### Monitoring Configuration
- Heartbeat interval: 120 seconds
- HTTP monitoring interval: 10 seconds
- Reconnection interval: 5 seconds

## 📈 Monitoring Metrics

### Basic Metrics (via TCP/WS)
- CPU usage rate
- Memory usage status
- Disk usage status
- Network traffic statistics

### Enhanced Metrics (via HTTP)
- GPU utilization
- Memory efficiency
- Thermal status monitoring
- Inference performance statistics

## 🚀 Future Extensions

1. **Multi-device Collaboration**: Support direct device-to-device communication
2. **Intelligent Scheduling**: AI-driven task allocation
3. **Edge Computing**: Local model optimization
4. **Security Enhancement**: End-to-end encrypted communication

---

*Last updated: November 25, 2025*
*Version: v1.0.0*
*Architecture: Dual compute sharing architecture compatible with existing WorkerHandle*
