// GPUFabric Android Rust Direct NDK SDK Header
#ifndef GPUFABRIC_ANDROID_RUST_DIRECT_NDK_H
#define GPUFABRIC_ANDROID_RUST_DIRECT_NDK_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>
#include <jni.h>

// 核心函数
int gpuf_init(void);
int gpuf_cleanup(void);
const char* gpuf_version(void);
const char* gpuf_get_last_error(void);

// Rust 引擎函数
int gpuf_llm_load_model(const char* model_path);
const char* gpuf_llm_generate(const char* prompt, int max_tokens);
int gpuf_llm_unload(void);
const char* gpuf_llm_get_model_info(const char* model_path);

// 内存管理
void gpuf_free_string(char* ptr);

// 扩展功能
int gpuf_get_model_count(void);
int gpuf_is_model_loaded(const char* model_path);
const char* gpuf_get_performance_stats(void);
int gpuf_register_model(const char* name, const char* path);

// JNI 接口
JNIEXPORT jint JNICALL Java_com_gpuf_c_GPUEngine_init(JNIEnv *env, jobject thiz);
JNIEXPORT jstring JNICALL Java_com_gpuf_c_GPUEngine_getVersion(JNIEnv *env, jobject thiz);
JNIEXPORT jstring JNICALL Java_com_gpuf_c_GPUEngine_generate(JNIEnv *env, jobject thiz, jstring prompt);
JNIEXPORT jint JNICALL Java_com_gpuf_c_GPUEngine_loadModel(JNIEnv *env, jobject thiz, jstring model_path);
JNIEXPORT void JNICALL Java_com_gpuf_c_GPUEngine_cleanup(JNIEnv *env, jobject thiz);
JNIEXPORT jstring JNICALL Java_com_gpuf_c_GPUEngine_getLastError(JNIEnv *env, jobject thiz);
JNIEXPORT jint JNICALL Java_com_gpuf_c_GPUEngine_getModelCount(JNIEnv *env, jobject thiz);
JNIEXPORT jboolean JNICALL Java_com_gpuf_c_GPUEngine_isModelLoaded(JNIEnv *env, jobject thiz, jstring model_path);
JNIEXPORT jstring JNICALL Java_com_gpuf_c_GPUEngine_getPerformanceStats(JNIEnv *env, jobject thiz);
JNIEXPORT jstring JNICALL Java_com_gpuf_c_GPUEngine_getModelInfo(JNIEnv *env, jobject thiz, jstring model_path);
JNIEXPORT jint JNICALL Java_com_gpuf_c_GPUEngine_registerModel(JNIEnv *env, jobject thiz, jstring name, jstring path);
JNIEXPORT jobjectArray JNICALL Java_com_gpuf_c_GPUEngine_batchGenerate(JNIEnv *env, jobject thiz, jobjectArray prompts);

#ifdef __cplusplus
}
#endif

#endif // GPUFABRIC_ANDROID_RUST_DIRECT_NDK_H
