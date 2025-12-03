#include <jni.h>
#include <android/log.h>
#include <string>
#include <mutex>
#include <memory>
#include <chrono>

#define LOG_TAG "GPUFabric-Android-Rust-Direct-NDK"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)
#define LOGD(...) __android_log_print(ANDROID_LOG_DEBUG, LOG_TAG, __VA_ARGS__)

// Rust 函数声明
extern "C" {
    int gpuf_init();
    int gpuf_cleanup();
    const char* gpuf_version();
    const char* gpuf_get_last_error();
    int gpuf_llm_load_model(const char* model_path);
    const char* gpuf_llm_generate(const char* prompt, int max_tokens);
    int gpuf_llm_unload();
    void gpuf_free_string(char* ptr);
    int gpuf_get_model_count();
    int gpuf_is_model_loaded(const char* model_path);
    const char* gpuf_llm_get_model_info(const char* model_path);
    const char* gpuf_get_performance_stats();
    int gpuf_register_model(const char* name, const char* path);
}

// 全局互斥锁
static std::mutex g_jni_mutex;
static bool g_jni_initialized = false;

// 辅助函数: 安全获取字符串
static std::string safe_get_string(JNIEnv *env, jstring jstr) {
    if (!jstr) return "";
    
    const char* c_str = env->GetStringUTFChars(jstr, nullptr);
    if (!c_str) return "";
    
    std::string result(c_str);
    env->ReleaseStringUTFChars(jstr, c_str);
    return result;
}

// JNI 实现
extern "C" {

JNIEXPORT jint JNICALL
Java_com_gpuf_c_GPUEngine_init(JNIEnv *env, jobject thiz) {
    std::lock_guard<std::mutex> lock(g_jni_mutex);
    
    LOGI("Initializing GPUFabric Android Rust Direct NDK SDK");
    
    if (!g_jni_initialized) {
        g_jni_initialized = true;
    }
    
    auto start_time = std::chrono::high_resolution_clock::now();
    
    int result = gpuf_init();
    
    auto end_time = std::chrono::high_resolution_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::milliseconds>(end_time - start_time);
    
    if (result == 0) {
        LOGI("GPUFabric Rust engine initialized successfully in %ld ms", duration.count());
    } else {
        LOGE("GPUFabric Rust engine initialization failed: %s", gpuf_get_last_error());
    }
    
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_getVersion(JNIEnv *env, jobject thiz) {
    const char* version = gpuf_version();
    if (version) {
        return env->NewStringUTF(version);
    }
    return env->NewStringUTF("unknown");
}

JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_generate(JNIEnv *env, jobject thiz, jstring prompt) {
    if (!prompt) {
        LOGE("Prompt is null");
        return env->NewStringUTF("Error: Prompt is null");
    }
    
    std::string prompt_str = safe_get_string(env, prompt);
    if (prompt_str.empty()) {
        LOGE("Failed to get prompt string");
        return env->NewStringUTF("Error: Failed to get prompt");
    }
    
    LOGD("Generating Rust engine response for prompt: %.100s...", prompt_str.c_str());
    
    auto start_time = std::chrono::high_resolution_clock::now();
    
    const char* result = gpuf_llm_generate(prompt_str.c_str(), 1024);
    
    auto end_time = std::chrono::high_resolution_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::milliseconds>(end_time - start_time);
    
    jstring j_result = env->NewStringUTF(result ? result : "Error: Generation failed");
    
    LOGD("Rust engine generation completed in %ld ms", duration.count());
    
    return j_result;
}

JNIEXPORT jint JNICALL
Java_com_gpuf_c_GPUEngine_loadModel(JNIEnv *env, jobject thiz, jstring model_path) {
    if (!model_path) {
        LOGE("Model path is null");
        return -1;
    }
    
    std::string path_str = safe_get_string(env, model_path);
    if (path_str.empty()) {
        LOGE("Failed to get model path string");
        return -1;
    }
    
    LOGI("Loading Rust engine model: %s", path_str.c_str());
    
    auto start_time = std::chrono::high_resolution_clock::now();
    
    int result = gpuf_llm_load_model(path_str.c_str());
    
    auto end_time = std::chrono::high_resolution_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::milliseconds>(end_time - start_time);
    
    if (result == 0) {
        LOGI("Rust engine model loaded successfully in %ld ms", duration.count());
    } else {
        LOGE("Rust engine model loading failed: %s", gpuf_get_last_error());
    }
    
    return result;
}

JNIEXPORT void JNICALL
Java_com_gpuf_c_GPUEngine_cleanup(JNIEnv *env, jobject thiz) {
    std::lock_guard<std::mutex> lock(g_jni_mutex);
    
    LOGI("Cleaning up GPUFabric Android Rust Direct NDK SDK");
    
    gpuf_cleanup();
    g_jni_initialized = false;
    
    LOGI("GPUFabric Rust engine cleaned up successfully");
}

// 扩展 JNI 方法
JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_getLastError(JNIEnv *env, jobject thiz) {
    const char* error = gpuf_get_last_error();
    return env->NewStringUTF(error ? error : "No error");
}

JNIEXPORT jint JNICALL
Java_com_gpuf_c_GPUEngine_getModelCount(JNIEnv *env, jobject thiz) {
    return gpuf_get_model_count();
}

JNIEXPORT jboolean JNICALL
Java_com_gpuf_c_GPUEngine_isModelLoaded(JNIEnv *env, jobject thiz, jstring model_path) {
    if (!model_path) {
        return JNI_FALSE;
    }
    
    std::string path_str = safe_get_string(env, model_path);
    if (path_str.empty()) {
        return JNI_FALSE;
    }
    
    int result = gpuf_is_model_loaded(path_str.c_str());
    return result == 1 ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_getPerformanceStats(JNIEnv *env, jobject thiz) {
    const char* stats = gpuf_get_performance_stats();
    return env->NewStringUTF(stats ? stats : "Stats unavailable");
}

// Rust 引擎特定方法
JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_getModelInfo(JNIEnv *env, jobject thiz, jstring model_path) {
    if (!model_path) {
        LOGE("Model path is null");
        return env->NewStringUTF("Error: Model path is null");
    }
    
    std::string path_str = safe_get_string(env, model_path);
    if (path_str.empty()) {
        LOGE("Failed to get model path string");
        return env->NewStringUTF("Error: Failed to get model path");
    }
    
    const char* info = gpuf_llm_get_model_info(path_str.c_str());
    return env->NewStringUTF(info ? info : "Model info unavailable");
}

// 注册新模型
JNIEXPORT jint JNICALL
Java_com_gpuf_c_GPUEngine_registerModel(JNIEnv *env, jobject thiz, jstring name, jstring path) {
    if (!name || !path) {
        LOGE("Name or path is null");
        return -1;
    }
    
    std::string name_str = safe_get_string(env, name);
    std::string path_str = safe_get_string(env, path);
    
    if (name_str.empty() || path_str.empty()) {
        LOGE("Failed to get name or path string");
        return -1;
    }
    
    LOGI("Registering model: %s -> %s", name_str.c_str(), path_str.c_str());
    
    int result = gpuf_register_model(name_str.c_str(), path_str.c_str());
    
    if (result == 0) {
        LOGI("Model registered successfully");
    } else {
        LOGE("Model registration failed: %s", gpuf_get_last_error());
    }
    
    return result;
}

// 批量生成方法
JNIEXPORT jobjectArray JNICALL
Java_com_gpuf_c_GPUEngine_batchGenerate(JNIEnv *env, jobject thiz, jobjectArray prompts) {
    if (!prompts) {
        LOGE("Prompts array is null");
        return nullptr;
    }
    
    jsize size = env->GetArrayLength(prompts);
    if (size <= 0) {
        LOGE("Empty prompts array");
        return nullptr;
    }
    
    LOGD("Batch generating %d Rust engine responses", size);
    
    // 创建结果数组
    jobjectArray result = env->NewObjectArray(size, env->FindClass("java/lang/String"), nullptr);
    if (!result) {
        LOGE("Failed to create result array");
        return nullptr;
    }
    
    // 逐个处理
    for (jsize i = 0; i < size; i++) {
        jstring prompt = (jstring)env->GetObjectArrayElement(prompts, i);
        jstring response = Java_com_gpuf_c_GPUEngine_generate(env, thiz, prompt);
        env->SetObjectArrayElement(result, i, response);
        
        if (prompt) env->DeleteLocalRef(prompt);
        if (response) env->DeleteLocalRef(response);
    }
    
    LOGD("Rust engine batch generation completed");
    return result;
}

} // extern "C"
