#ifndef GPUF_C_H
#define GPUF_C_H

#pragma once

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct llama_model {
  uint8_t _private[0];
} llama_model;

typedef struct llama_model_params {
  int32_t n_gpu_layers;
  int32_t main_gpu;
  const float *tensor_split;
  bool use_mmap;
  bool use_mlock;
  void (*progress_callback)(float, void*);
  void *progress_callback_user_data;
  const char *kv_overrides;
  bool vocab_only;
} llama_model_params;

typedef struct llama_context {
  uint8_t _private[0];
} llama_context;

typedef struct llama_context_params {
  uint32_t n_ctx;
  uint32_t n_batch;
  int32_t n_gpu_layers;
  int32_t main_gpu;
  const float *tensor_split;
  bool f16_kv;
  bool logits_all;
  bool embedding;
  bool offload_kqv;
  int32_t rope_scaling_type;
  float rope_freq_base;
  float rope_freq_scale;
  float yarn_ext_factor;
  float yarn_attn_factor;
  float yarn_beta_fast;
  float yarn_beta_slow;
  int32_t yarn_orig_ctx;
  int32_t pooling_type;
} llama_context_params;

typedef int32_t LlamaToken;

extern int llama_backend_init(void);

extern void llama_backend_free(void);

extern struct llama_model *llama_load_model_from_file(const char *path,
                                                      struct llama_model_params params);

extern struct llama_context *llama_init_from_model(const struct llama_model *model,
                                                   struct llama_context_params params);

extern int llama_tokenize(struct llama_context *ctx,
                          const char *text,
                          LlamaToken *tokens,
                          int n_max_tokens,
                          bool add_bos);

extern int llama_generate(struct llama_context *ctx,
                          const LlamaToken *tokens,
                          int n_tokens,
                          int *n_past,
                          int n_threads);

extern int llama_n_ctx(const struct llama_context *ctx);

extern int llama_model_n_vocab(const struct llama_model *model);

extern LlamaToken llama_token_bos(const struct llama_model *model);

extern LlamaToken llama_token_eos(const struct llama_model *model);

extern void llama_model_free(struct llama_model *model);

extern void llama_free(struct llama_context *ctx);

extern void *ggml_backend_dev_by_type(int32_t type_);

extern void *ggml_backend_dev_get(int32_t i);

extern int32_t ggml_backend_dev_count(void);

struct llama_model *gpuf_load_model(const char *path);

struct llama_context *gpuf_create_context(struct llama_model *model);

int gpuf_tokenize_text(struct llama_context *ctx,
                       const char *text,
                       LlamaToken *tokens,
                       int max_tokens);

int gpuf_generate_final_solution_text(const struct llama_model *model,
                                      struct llama_context *ctx,
                                      const char *prompt,
                                      int _max_tokens,
                                      char *output,
                                      int output_len);

const char *gpuf_system_info(void);

const char *gpuf_version(void);

int gpuf_init(void);

int gpuf_cleanup(void);

jint Java_com_gpuf_c_GPUEngine_initialize(JNIEnv _env, JClass _class);

jlong Java_com_gpuf_c_GPUEngine_loadModel(JNIEnv env, JClass _class, JString model_path);

jlong Java_com_gpuf_c_GPUEngine_createContext(JNIEnv _env, JClass _class, jlong model_ptr);

jint Java_com_gpuf_c_GPUEngine_generate(JNIEnv env,
                                        JClass _class,
                                        jlong model_ptr,
                                        jlong context_ptr,
                                        JString prompt,
                                        jint max_tokens,
                                        JObject _output_buffer);

jstring Java_com_gpuf_c_GPUEngine_getVersion(JNIEnv env, JClass _class);

jint Java_com_gpuf_c_GPUEngine_cleanup(JNIEnv _env, JClass _class);

jstring Java_com_gpuf_c_GPUEngine_getSystemInfo(JNIEnv env, JClass _class);

jint Java_com_gpuf_c_GPUEngine_gpuf_1init(JNIEnv _env, JClass _class);

#endif /* GPUF_C_H */
