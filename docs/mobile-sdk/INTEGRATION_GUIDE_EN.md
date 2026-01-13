# GPUFabric Mobile SDK Integration Guide

This guide provides detailed instructions for integrating GPUFabric Mobile SDK into Android and iOS applications.

## 📋 Table of Contents

- [Quick Integration](#quick-integration)
- [Android Integration](#android-integration)
- [iOS Integration](#ios-integration)
- [API Reference](#api-reference)
- [Best Practices](#best-practices)
- [Troubleshooting](#troubleshooting)

## 🚀 Quick Integration

### Android (5-minute integration)
1. **Copy library files**
   ```
   app/src/main/jniLibs/arm64-v8a/libgpuf_c.so
   ```

2. **Add dependencies**
   ```gradle
   implementation("net.java.dev.jna:jna:5.13.0@aar")
   ```

3. **Call API**
   ```kotlin
   val client = GPUFabricClient()
   client.initialize()
   val version = client.getVersion()
   ```

### iOS (5-minute integration)
1. **Add static library**
   ```
   libgpuf_c.a
   gpuf_c.h
   ```

2. **Configure Build Settings**
   - Add library search paths
   - Link static library

3. **Call API**
   ```swift
   let result = gpuf_init()
   let version = gpuf_version()
   ```

---

## 🤖 Android Integration

### 1. Project Setup

#### Create New Project
```bash
# Using Android Studio
# File → New → New Project → Empty Activity
# Language: Kotlin
# Minimum SDK: API 24
```

#### Or integrate into existing project

### 2. Add Library Files

#### Directory Structure
```
app/src/main/
├── jniLibs/
│   ├── arm64-v8a/
│   │   └── libgpuf_c.so          # ARM64 library
│   ├── armeabi-v7a/
│   │   └── libgpuf_c.so          # ARMv7 library (optional)
│   └── x86_64/
│       └── libgpuf_c.so          # x86_64 library (emulator)
└── cpp/
    └── gpuf_c.h                   # C header file (reference)
```

#### Copy Files
```powershell
# Copy from build output
Copy-Item "target\aarch64-linux-android\release\libgpuf_c.so" `
         "app\src\main\jniLibs\arm64-v8a\"
```

### 3. Configure Gradle

#### app/build.gradle.kts
```kotlin
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.example.gpufabric"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.example.gpufabric"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"

        ndk {
            abiFilters.add("arm64-v8a")  // Main architecture
            // abiFilters.add("armeabi-v7a")  // Optional
            // abiFilters.add("x86_64")      // Emulator
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.11.0")
    
    // JNA for native library access
    implementation("net.java.dev.jna:jna:5.13.0@aar")
    
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
}
```

### 4. Create SDK Wrapper Class

#### GPUFabricClient.kt
```kotlin
package com.example.gpufabric

import android.util.Log
import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer

interface GPUFabricNative : Library {
    companion object {
        val INSTANCE: GPUFabricNative by lazy {
            System.loadLibrary("gpuf_c")
            Native.load("gpuf_c", GPUFabricNative::class.java)
        }
    }
    
    // Basic functions
    fun gpuf_init(): Int
    fun gpuf_get_last_error(): Pointer?
    fun gpuf_free_string(ptr: Pointer?)
    fun gpuf_version(): Pointer?
    
    // LLM inference functions
    fun gpuf_llm_init(modelPath: String, nCtx: Int, nGpuLayers: Int): Int
    fun gpuf_llm_generate(prompt: String, maxTokens: Int): Pointer?
}

class GPUFabricClient {
    private val TAG = "GPUFabric"
    
    /**
     * Initialize GPUFabric library
     */
    fun initialize(): Boolean {
        return try {
            Log.i(TAG, "Initializing GPUFabric...")
            val result = GPUFabricNative.INSTANCE.gpuf_init()
            if (result != 0) {
                val error = getLastError()
                Log.e(TAG, "Initialization failed: $error")
                false
            } else {
                Log.i(TAG, "✅ Initialization successful")
                true
            }
        } catch (e: Exception) {
            Log.e(TAG, "Initialization exception: ${e.message}", e)
            false
        }
    }
    
    /**
     * Get version information
     */
    fun getVersion(): String {
        return try {
            val versionPtr = GPUFabricNative.INSTANCE.gpuf_version()
            versionPtr?.getString(0) ?: "unknown"
        } catch (e: Exception) {
            Log.e(TAG, "Failed to get version: ${e.message}")
            "error"
        }
    }
    
    /**
     * Initialize LLM engine
     */
    fun initLLM(modelPath: String, contextSize: Int = 2048, gpuLayers: Int = 0): Boolean {
        return try {
            Log.i(TAG, "Initializing LLM with model: $modelPath")
            val result = GPUFabricNative.INSTANCE.gpuf_llm_init(modelPath, contextSize, gpuLayers)
            if (result != 0) {
                val error = getLastError()
                Log.e(TAG, "LLM initialization failed: $error")
                false
            } else {
                Log.i(TAG, "✅ LLM initialization successful")
                true
            }
        } catch (e: Exception) {
            Log.e(TAG, "LLM initialization exception: ${e.message}", e)
            false
        }
    }
    
    /**
     * Generate text
     */
    fun generateText(prompt: String, maxTokens: Int = 100): String {
        return try {
            Log.i(TAG, "Generating text for prompt: $prompt")
            val resultPtr = GPUFabricNative.INSTANCE.gpuf_llm_generate(prompt, maxTokens)
            if (resultPtr == null) {
                val error = getLastError()
                Log.e(TAG, "Generation failed: $error")
                ""
            } else {
                val text = resultPtr.getString(0)
                GPUFabricNative.INSTANCE.gpuf_free_string(resultPtr)
                text
            }
        } catch (e: Exception) {
            Log.e(TAG, "Generation exception: ${e.message}", e)
            ""
        }
    }
    
    /**
     * Get last error message
     */
    private fun getLastError(): String {
        return try {
            val errorPtr = GPUFabricNative.INSTANCE.gpuf_get_last_error()
            if (errorPtr == null) "unknown error" 
            else {
                val error = errorPtr.getString(0)
                GPUFabricNative.INSTANCE.gpuf_free_string(errorPtr)
                error
            }
        } catch (e: Exception) {
            "Failed to get error: ${e.message}"
        }
    }
}
```

### 5. Usage Example

#### MainActivity.kt
```kotlin
package com.example.gpufabric

import android.os.Bundle
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {
    private lateinit var client: GPUFabricClient
    private lateinit var logText: TextView
    
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        
        client = GPUFabricClient()
        logText = findViewById(R.id.logText)
        
        setupButtons()
    }
    
    private fun setupButtons() {
        findViewById<Button>(R.id.btnInit).setOnClickListener {
            testInitialization()
        }
        
        findViewById<Button>(R.id.btnVersion).setOnClickListener {
            testVersion()
        }
        
        findViewById<Button>(R.id.btnLLM).setOnClickListener {
            testLLM()
        }
    }
    
    private fun testInitialization() {
        logText.append("=== Testing Initialization ===\n")
        val success = client.initialize()
        logText.append(if (success) "✅ Success\n" else "❌ Failed\n")
    }
    
    private fun testVersion() {
        logText.append("=== Getting Version ===\n")
        val version = client.getVersion()
        logText.append("Version: $version\n")
    }
    
    private fun testLLM() {
        logText.append("=== Testing LLM ===\n")
        
        // Note: Need to provide actual model file path
        val modelPath = "/sdcard/Download/model.gguf"
        val initSuccess = client.initLLM(modelPath)
        
        if (initSuccess) {
            val result = client.generateText("Hello, how are you?", 50)
            logText.append("Generated: $result\n")
        } else {
            logText.append("❌ LLM initialization failed\n")
        }
    }
}
```

---

## 🍎 iOS Integration

### 1. Project Setup

#### Create New Project
```bash
# Using Xcode
# File → New → Project → App
# Interface: SwiftUI
# Language: Swift
# Minimum Deployment Target: iOS 14.0
```

### 2. Add Library Files

#### Method 1: Direct Drag & Drop
1. Right-click on project folder in Xcode
2. Select "Add Files to Project"
3. Add `libgpuf_c.a` and `gpuf_c.h`

#### Method 2: Via Build Settings
1. Open project settings
2. Find in "Build Settings":
   - **Library Search Paths**: Add directory containing `.a` file
   - **Header Search Paths**: Add directory containing `.h` file

### 3. Configure Build Settings

#### Linking Settings
```
Build Settings → Linking → Other Linker Flags
Add: -lgpuf_c
```

#### Architecture Settings
```
Build Settings → Architectures → Valid Architectures
Ensure includes: arm64, x86_64
```

### 4. Create Swift Bridging

#### GPUFabric-Bridging-Header.h
```objc
#ifndef GPUFabric_Bridging_Header_h
#define GPUFabric_Bridging_Header_h

#include "gpuf_c.h"

#endif /* GPUFabric_Bridging_Header_h */
```

#### Set in Build Settings
```
Build Settings → Swift Compiler → General → Objective-C Bridging Header
Add: GPUFabric-Bridging-Header.h
```

### 5. Create Swift Wrapper

#### GPUFabricClient.swift
```swift
import Foundation

class GPUFabricClient {
    static let shared = GPUFabricClient()
    
    private init() {
        // Initialize library
        let result = gpuf_init()
        if result != 0 {
            let error = getLastError()
            print("❌ GPUFabric initialization failed: \(error)")
        } else {
            print("✅ GPUFabric initialized successfully")
        }
    }
    
    // MARK: - Basic Functions
    
    /// Get version information
    func getVersion() -> String {
        guard let versionPtr = gpuf_version() else {
            return "unknown"
        }
        let version = String(cString: versionPtr)
        gpuf_free_string(versionPtr)
        return version
    }
    
    /// Get last error
    private func getLastError() -> String {
        guard let errorPtr = gpuf_get_last_error() else {
            return "unknown error"
        }
        let error = String(cString: errorPtr)
        gpuf_free_string(errorPtr)
        return error
    }
    
    // MARK: - LLM Functions
    
    /// Initialize LLM engine
    func initLLM(modelPath: String, contextSize: Int32 = 2048, gpuLayers: Int32 = 0) -> Bool {
        let result = gpuf_llm_init(modelPath, contextSize, gpuLayers)
        if result != 0 {
            let error = getLastError()
            print("❌ LLM initialization failed: \(error)")
            return false
        } else {
            print("✅ LLM initialized successfully")
            return true
        }
    }
    
    /// Generate text
    func generateText(prompt: String, maxTokens: Int32 = 100) -> String {
        guard let resultPtr = gpuf_llm_generate(prompt, maxTokens) else {
            let error = getLastError()
            print("❌ Generation failed: \(error)")
            return ""
        }
        
        let result = String(cString: resultPtr)
        gpuf_free_string(resultPtr)
        return result
    }
}
```

### 6. Usage Example

#### ContentView.swift
```swift
import SwiftUI

struct ContentView: View {
    @State private var logText = "Ready for testing...\n"
    @State private var isLoading = false
    
    var body: some View {
        VStack(spacing: 16) {
            Text("GPUFabric iOS Test")
                .font(.title)
                .padding()
            
            ScrollView {
                Text(logText)
                    .font(.system(.caption, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
                    .background(Color(.systemGray6))
                    .cornerRadius(8)
            }
            .frame(maxHeight: 300)
            
            VStack(spacing: 8) {
                HStack(spacing: 16) {
                    Button("Init SDK") {
                        testInitialization()
                    }
                    .buttonStyle(.borderedProminent)
                    
                    Button("Get Version") {
                        testVersion()
                    }
                    .buttonStyle(.bordered)
                }
                
                HStack(spacing: 16) {
                    Button("Init LLM") {
                        testLLMInit()
                    }
                    .buttonStyle(.bordered)
                    
                    Button("Generate") {
                        testGeneration()
                    }
                    .buttonStyle(.bordered)
                }
                
                Button("Clear Log") {
                    logText = ""
                }
                .buttonStyle(.bordered)
            }
            
            Spacer()
        }
        .padding()
    }
    
    private func appendLog(_ message: String) {
        DispatchQueue.main.async {
            logText += message + "\n"
        }
    }
    
    private func testInitialization() {
        appendLog("=== Testing Initialization ===")
        let client = GPUFabricClient.shared
        appendLog("✅ SDK loaded successfully")
    }
    
    private func testVersion() {
        appendLog("=== Getting Version ===")
        let version = GPUFabricClient.shared.getVersion()
        appendLog("Version: \(version)")
    }
    
    private func testLLMInit() {
        appendLog("=== Testing LLM Initialization ===")
        
        // Need to provide actual model file path
        let modelPath = Bundle.main.path(forResource: "model", ofType: "gguf") ?? "/tmp/model.gguf"
        let success = GPUFabricClient.shared.initLLM(modelPath)
        
        appendLog(success ? "✅ LLM initialized" : "❌ LLM initialization failed")
    }
    
    private func testGeneration() {
        appendLog("=== Testing Text Generation ===")
        
        let result = GPUFabricClient.shared.generateText("Hello, how are you?", maxTokens: 50)
        appendLog("Generated: \(result)")
    }
}
```

---

## 📚 API Reference

### Basic Functions

| Function | Description | Return Value |
|----------|-------------|--------------|
| `gpuf_init()` | Initialize library | `0`=success, `non-zero`=failure |
| `gpuf_version()` | Get version | `char*` (version string) |
| `gpuf_get_last_error()` | Get error | `char*` (error message) |
| `gpuf_free_string(ptr)` | Free string | `void` |

### LLM Functions

| Function | Description | Parameters | Return Value |
|----------|-------------|------------|--------------|
| `gpuf_llm_init()` | Initialize LLM | `modelPath`, `nCtx`, `nGpuLayers` | `0`=success, `non-zero`=failure |
| `gpuf_llm_generate()` | Generate text | `prompt`, `maxTokens` | `char*` (generation result) |

### Parameter Description

- `modelPath`: GGUF model file path
- `nCtx`: Context window size (recommended 2048)
- `nGpuLayers`: Number of GPU layers (0=CPU only, >0=GPU acceleration)
- `maxTokens`: Maximum generation token count

---

## 💡 Best Practices

### 1. Performance Optimization

#### Android
```kotlin
// Initialize in Application
class MyApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        // Preload library
        GPUFabricClient.shared.initialize()
    }
}
```

#### iOS
```swift
// Initialize in AppDelegate
func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
    _ = GPUFabricClient.shared
    return true
}
```

### 2. Memory Management

```kotlin
// Release strings promptly
val resultPtr = GPUFabricNative.INSTANCE.gpuf_llm_generate(prompt, maxTokens)
try {
    val result = resultPtr.getString(0)
    // Use result
} finally {
    GPUFabricNative.INSTANCE.gpuf_free_string(resultPtr)
}
```

### 3. Error Handling

```kotlin
fun safeOperation(): String? {
    return try {
        val result = client.generateText("Hello")
        if (result.isEmpty()) {
            val error = client.getLastError()
            Log.e("GPUFabric", "Operation failed: $error")
            null
        } else {
            result
        }
    } catch (e: Exception) {
        Log.e("GPUFabric", "Exception: ${e.message}", e)
        null
    }
}
```

### 4. Model Management

```kotlin
class ModelManager {
    private var currentModel: String? = null
    
    fun loadModel(modelPath: String): Boolean {
        // Unload current model (if needed)
        currentModel?.let { 
            // Implement model unload logic
        }
        
        // Load new model
        val success = client.initLLM(modelPath)
        if (success) {
            currentModel = modelPath
        }
        return success
    }
}
```

---

## 🐛 Troubleshooting

### Android

#### Library Load Failure
```
java.lang.UnsatisfiedLinkError: couldn't find "libgpuf_c.so"
```
**Solution**:
- Check `jniLibs` directory structure
- Confirm ABI filter configuration
- Verify file permissions

#### Initialization Failure
```
gpuf_init() returns -1
```
**Solution**:
- Check Logcat output
- Confirm device architecture support
- Verify dependency library integrity

### iOS

#### Linking Error
```
ld: library not found for -lgpuf_c
```
**Solution**:
- Check Library Search Paths
- Confirm static library file exists
- Verify architecture matching

#### Header File Not Found
```
'gpuf_c.h' file not found
```
**Solution**:
- Check Header Search Paths
- Confirm bridging header file path
- Verify file import

### General

#### Performance Issues
- Use GPU acceleration (`nGpuLayers > 0`)
- Choose appropriate context size
- Use quantized models

#### Memory Issues
- Release strings promptly
- Monitor memory usage
- Avoid loading multiple models simultaneously

---

## 📖 More Resources

- [Build Guide](./BUILD_GUIDE.md)
- [Mobile SDK Index](./README.md)
- [Example Projects (Android)](../../gpuf-c/examples/android/)
- [Example Projects (Mobile)](../../gpuf-c/examples/mobile/)

## 🤝 Contributing

Welcome to submit integration issues and improvement suggestions!
