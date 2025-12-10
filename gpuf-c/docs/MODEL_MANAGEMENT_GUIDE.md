# GPUFabric SDK Model Management Usage Guide

## Overview

GPUFabric SDK provides complete model management functionality, supporting dynamic model loading, model status querying, and notifying the server of current model information. These features are particularly useful when the SDK runs background services.

## 🔧 New Model Management Functions

### 1. Dynamic Model Loading

```java
/**
 * Dynamically load the specified model
 * @param modelPath Model file path
 * @return 0 for success, -1 for failure
 */
public static native int loadModel(String modelPath);
```

**Features:**
- ✅ Supports runtime dynamic loading of new models
- ✅ Automatically unloads current model and loads new model
- ✅ Automatically notifies server after successful loading (non-offline mode)
- ✅ Asynchronous loading, does not block main thread

### 2. Query Current Model

```java
/**
 * Get the path of the currently loaded model
 * @return Current model path, returns null on failure
 */
public static native String getCurrentModel();
```

**Features:**
- ✅ Returns the path of the currently used model
- ✅ Returns empty string if no model is loaded
- ✅ Thread-safe query operation

### 3. Check Model Loading Status

```java
/**
 * Check if any model is loaded
 * @return 1 for loaded, 0 for not loaded, -1 for error
 */
public static native int isModelLoaded();
```

**Features:**
- ✅ Quick check of model loading status
- ✅ Suitable for conditional judgment and status checking
- ✅ Returns clear boolean value result

### 4. Get Detailed Loading Status

```java
/**
 * Get detailed status information of model loading
 * @return Status string, returns null on failure
 */
public static native String getModelLoadingStatus();
```

**Features:**
- ✅ Returns detailed loading status information
- ✅ Includes loading progress, error information, etc.
- ✅ Suitable for debugging and user interface display

## 📱 Usage Examples

### Basic Usage Flow

```java
public class ModelManager {
    private static final String TAG = "ModelManager";
    
    // 1. Start inference service
    public void startService() {
        String initialModel = "/path/to/initial/model.gguf";
        int result = GpufNative.startInferenceService(initialModel, 8082);
        
        if (result == 0) {
            Log.i(TAG, "Inference service started successfully");
            
            // Start compute monitoring (offline mode)
            GpufNative.startComputeMonitoring(
                "http://gpufabric.com:8080", 
                "gpufs.example.com", 
                8081, 8083, 0, 2, true
            );
        }
    }
    
    // 2. Dynamic model switching
    public boolean switchModel(String newModelPath) {
        Log.i(TAG, "Switching to model: " + newModelPath);
        
        int result = GpufNative.loadModel(newModelPath);
        if (result == 0) {
            Log.i(TAG, "Model switched successfully");
            return true;
        } else {
            String error = GpufNative.getLastError();
            Log.e(TAG, "Failed to switch model: " + error);
            return false;
        }
    }
    
    // 3. Query model status
    public void checkModelStatus() {
        // Check if any model is loaded
        int isLoaded = GpufNative.isModelLoaded();
        if (isLoaded == 1) {
            Log.i(TAG, "Model is loaded");
            
            // Get current model path
            String currentModel = GpufNative.getCurrentModel();
            Log.i(TAG, "Current model: " + currentModel);
            
            // Get detailed status
            String status = GpufNative.getModelLoadingStatus();
            Log.i(TAG, "Model status: " + status);
        } else if (isLoaded == 0) {
            Log.w(TAG, "No model is loaded");
        } else {
            String error = GpufNative.getLastError();
            Log.e(TAG, "Error checking model status: " + error);
        }
    }
}
```

### Advanced Usage Scenarios

#### 1. Smart Model Switching

```java
public class SmartModelSwitcher {
    private Map<String, ModelInfo> availableModels = new HashMap<>();
    
    public void initializeModels() {
        // Predefined available models
        availableModels.put("chat", new ModelInfo("/models/chat.gguf", "Chat model"));
        availableModels.put("code", new ModelInfo("/models/code.gguf", "Code model"));
        availableModels.put("translate", new ModelInfo("/models/translate.gguf", "Translation model"));
    }
    
    public boolean switchToOptimalModel(String taskType) {
        ModelInfo modelInfo = availableModels.get(taskType);
        if (modelInfo == null) {
            Log.e(TAG, "Unknown task type: " + taskType);
            return false;
        }
        
        // Check current model
        String currentModel = GpufNative.getCurrentModel();
        if (modelInfo.path.equals(currentModel)) {
            Log.i(TAG, "Model already loaded: " + taskType);
            return true;
        }
        
        // Switch model
        return switchModel(modelInfo.path);
    }
    
    private static class ModelInfo {
        String path;
        String description;
        
        ModelInfo(String path, String description) {
            this.path = path;
            this.description = description;
        }
    }
}
```

#### 2. Model Loading Monitoring

```java
public class ModelLoadingMonitor {
    private Handler mainHandler = new Handler(Looper.getMainLooper());
    
    public void monitorLoading() {
        new Thread(() -> {
            while (true) {
                String status = GpufNative.getModelLoadingStatus();
                
                mainHandler.post(() -> {
                    updateUI(status);
                });
                
                try {
                    Thread.sleep(1000); // Check every second
                } catch (InterruptedException e) {
                    break;
                }
            }
        }).start();
    }
    
    private void updateUI(String status) {
        // Update user interface to show loading status
        if (status.contains("loading")) {
            showProgressBar();
        } else if (status.contains("ready")) {
            hideProgressBar();
        } else if (status.contains("error")) {
            showError(status);
        }
    }
}
```

#### 3. Offline Mode Model Management

```java
public class OfflineModelManager {
    private boolean isOfflineMode = true;
    
    public void initializeOfflineMode() {
        // Start offline mode
        GpufNative.startComputeMonitoring(
            "", "", 0, 0, 0, 2, true  // Offline mode
        );
        
        // Load local model
        String localModel = getLocalModelPath();
        if (GpufNative.loadModel(localModel) == 0) {
            Log.i(TAG, "Local model loaded successfully");
        }
    }
    
    public String getLocalModelPath() {
        // Return locally stored model path
        return "/storage/emulated/0/models/default.gguf";
    }
    
    public void switchToModel(String modelName) {
        String modelPath = getLocalModelPath(modelName);
        if (new File(modelPath).exists()) {
            GpufNative.loadModel(modelPath);
        } else {
            Log.e(TAG, "Model not found: " + modelPath);
        }
    }
}
```

## 🔄 Server Notification Mechanism

### Automatic Notification

When a model is successfully loaded, the SDK automatically notifies the server of the current model information:

```json
{
  "model_path": "/path/to/model.gguf",
  "timestamp": 1701234567,
  "device_id": "android-device-001",
  "status": "loaded"
}
```

### Notification Conditions

- ✅ **Online Mode**: Automatically send notification to server
- ❌ **Offline Mode**: Skip notification to protect privacy
- ✅ **Network Available**: Only send when network is connected
- ✅ **Load Successful**: Only notify after model is successfully loaded

### Notification Endpoint

```
POST /api/models/current
Content-Type: application/json
Authorization: Bearer <device_token>
```

## 📊 Status Information Description

### Model Loading Status

| Status Value | Description | Applicable Scenarios |
|--------|------|----------|
| `"not_loaded"` | No model loaded | Initial state |
| `"loading"` | Model is loading | During loading process |
| `"ready"` | Model loaded, ready for inference | Normal usage state |
| `"error"` | Loading failed | Error handling |
| `"switching"` | Model switching in progress | Model switching |

### Error Handling

```java
public void handleModelError() {
    int result = GpufNative.loadModel("/path/to/model.gguf");
    
    if (result != 0) {
        String error = GpufNative.getLastError();
        
        switch (error) {
            case "Model file not found":
                // Handle file not found
                downloadModel();
                break;
                
            case "Insufficient memory":
                // Handle insufficient memory
                freeMemory();
                break;
                
            case "Invalid model format":
                // Handle format error
                showFormatError();
                break;
                
            default:
                // Generic error handling
                Log.e(TAG, "Unknown error: " + error);
                break;
        }
    }
}
```

## 🎯 Best Practices

### 1. Model Preloading

```java
public class ModelPreloader {
    public void preloadCommonModels() {
        // Preload commonly used models when app starts
        String[] commonModels = {
            "/models/chat.gguf",
            "/models/qa.gguf"
        };
        
        for (String model : commonModels) {
            if (new File(model).exists()) {
                // Asynchronous preloading
                CompletableFuture.runAsync(() -> {
                    GpufNative.loadModel(model);
                });
            }
        }
    }
}
```

### 2. Memory Management

```java
public class MemoryAwareModelManager {
    public void switchModelWithMemoryCheck(String newModel) {
        // Check available memory
        Runtime runtime = Runtime.getRuntime();
        long maxMemory = runtime.maxMemory();
        long usedMemory = runtime.totalMemory() - runtime.freeMemory();
        long availableMemory = maxMemory - usedMemory;
        
        // Estimate model size
        long modelSize = estimateModelSize(newModel);
        
        if (availableMemory > modelSize * 2) { // Keep 2x buffer
            GpufNative.loadModel(newModel);
        } else {
            // Clean memory and retry
            System.gc();
            try {
                Thread.sleep(1000);
            } catch (InterruptedException e) {
                // ignore
            }
            
            if (runtime.freeMemory() > modelSize) {
                GpufNative.loadModel(newModel);
            } else {
                Log.w(TAG, "Insufficient memory for model: " + newModel);
            }
        }
    }
    
    private long estimateModelSize(String modelPath) {
        File file = new File(modelPath);
        return file.exists() ? file.length() : 0;
    }
}
```

### 3. Error Recovery

```java
public class RobustModelManager {
    private String lastSuccessfulModel;
    
    public boolean safeLoadModel(String modelPath) {
        try {
            int result = GpufNative.loadModel(modelPath);
            if (result == 0) {
                lastSuccessfulModel = modelPath;
                return true;
            }
        } catch (Exception e) {
            Log.e(TAG, "Exception loading model: " + e.getMessage());
        }
        
        // Loading failed, fall back to last successful model
        if (lastSuccessfulModel != null) {
            Log.i(TAG, "Falling back to last successful model: " + lastSuccessfulModel);
            return GpufNative.loadModel(lastSuccessfulModel) == 0;
        }
        
        return false;
    }
}
```

## 🚀 Performance Optimization

### 1. Model Caching Strategy

- ✅ Keep frequently used models in memory
- ✅ Preload models based on usage frequency
- ✅ Intelligently unload infrequently used models

### 2. Asynchronous Loading

- ✅ All model operations are asynchronous
- ✅ Does not block main thread
- ✅ Provides progress callback mechanism

### 3. Network Optimization

- ✅ Offline mode skips network requests
- ✅ Automatic degradation on network failure
- ✅ Batch notifications reduce request count

---

*Last updated: November 25, 2025*
*Version: v1.0.0*
*Features: Complete model management functionality, supporting dynamic loading and server notifications*
