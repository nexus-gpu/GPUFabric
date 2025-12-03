#include <jni.h>
#include <android/log.h>
#include <string>
#include <mutex>

#define LOG_TAG "GPUFabric-Real-Inference"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)

// 真实推理 Rust 函数声明
extern "C" {
    int gpuf_real_init();
    int gpuf_real_cleanup();
    const char* gpuf_real_version();
    const char* gpuf_real_get_last_error();
    int gpuf_real_load_model(const char* model_path);
    const char* gpuf_real_generate(const char* prompt, int max_tokens);
    void gpuf_free_string(char* ptr);
}

static std::mutex g_real_mutex;

// JNI 实现
extern "C" {

JNIEXPORT jint JNICALL
Java_com_gpuf_c_GPUEngine_realInit(JNIEnv *env, jobject thiz) {
    std::lock_guard<std::mutex> lock(g_real_mutex);
    
    LOGI("Initializing Real LLAMA.cpp Inference");
    
    int result = gpuf_real_init();
    
    if (result == 0) {
        LOGI("Real LLAMA.cpp inference initialized successfully");
    } else {
        LOGE("Real LLAMA.cpp inference initialization failed: %s", gpuf_real_get_last_error());
    }
    
    return result;
}

JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_realGenerate(JNIEnv *env, jobject thiz, jstring prompt) {
    if (!prompt) {
        LOGE("Prompt is null");
        return env->NewStringUTF("Error: Prompt is null");
    }
    
    std::string prompt_str;
    const char* c_str = env->GetStringUTFChars(prompt, nullptr);
    if (c_str) {
        prompt_str = c_str;
        env->ReleaseStringUTFChars(prompt, c_str);
    }
    
    LOGI("Real LLAMA.cpp generating: %.100s...", prompt_str.c_str());
    
    const char* result = gpuf_real_generate(prompt_str.c_str(), 1024);
    
    jstring j_result = env->NewStringUTF(result ? result : "Error: Generation failed");
    
    // 释放 Rust 分配的字符串
    if (result) {
        gpuf_free_string(const_cast<char*>(result));
    }
    
    return j_result;
}

JNIEXPORT jint JNICALL
Java_com_gpuf_c_GPUEngine_realLoadModel(JNIEnv *env, jobject thiz, jstring model_path) {
    if (!model_path) {
        LOGE("Model path is null");
        return -1;
    }
    
    std::string path_str;
    const char* c_str = env->GetStringUTFChars(model_path, nullptr);
    if (c_str) {
        path_str = c_str;
        env->ReleaseStringUTFChars(model_path, c_str);
    }
    
    LOGI("Loading real LLAMA.cpp model: %s", path_str.c_str());
    
    int result = gpuf_real_load_model(path_str.c_str());
    
    if (result == 0) {
        LOGI("Real LLAMA.cpp model loaded successfully");
    } else {
        LOGE("Real LLAMA.cpp model loading failed: %s", gpuf_real_get_last_error());
    }
    
    return result;
}

JNIEXPORT void JNICALL
Java_com_gpuf_c_GPUEngine_realCleanup(JNIEnv *env, jobject thiz) {
    std::lock_guard<std::mutex> lock(g_real_mutex);
    
    LOGI("Cleaning up Real LLAMA.cpp Inference");
    
    gpuf_real_cleanup();
    
    LOGI("Real LLAMA.cpp inference cleaned up successfully");
}

JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_realGetVersion(JNIEnv *env, jobject thiz) {
    const char* version = gpuf_real_version();
    return env->NewStringUTF(version ? version : "unknown");
}

JNIEXPORT jstring JNICALL
Java_com_gpuf_c_GPUEngine_realGetLastError(JNIEnv *env, jobject thiz) {
    const char* error = gpuf_real_get_last_error();
    return env->NewStringUTF(error ? error : "No error");
}

} // extern "C"
