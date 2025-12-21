#ifndef GPUF_C_H
#define GPUF_C_H

#pragma once

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum ProjectorType {
  Unknown = 0,
  LLaVA = 1,
  Qwen2VL = 2,
  Qwen25VL = 3,
  Qwen3VL = 4,
  Pixtral = 5,
} ProjectorType;

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
  uint32_t n_ubatch;
  uint32_t n_seq_max;
  int32_t n_threads;
  int32_t n_threads_batch;
  int32_t rope_scaling_type;
  int32_t pooling_type;
  int32_t attention_type;
  int32_t flash_attn_type;
  float rope_freq_base;
  float rope_freq_scale;
  float yarn_ext_factor;
  float yarn_attn_factor;
  float yarn_beta_fast;
  float yarn_beta_slow;
  uint32_t yarn_orig_ctx;
  float defrag_thold;
  void *cb_eval;
  void *cb_eval_user_data;
  int32_t type_k;
  int32_t type_v;
  void *abort_callback;
  void *abort_callback_data;
  bool embeddings;
  bool offload_kqv;
  bool no_perf;
  bool op_offload;
  bool swa_full;
  bool kv_unified;
} llama_context_params;

typedef struct llama_vocab {
  uint8_t _private[0];
} llama_vocab;

typedef int32_t LlamaToken;

typedef int LlamaPos;

typedef int LlamaSeqId;

typedef struct llama_batch {
  int n_tokens;
  const LlamaToken *token;
  const float *embd;
  const LlamaPos *pos;
  const int *n_seq_id;
  const LlamaSeqId *seq_id;
  const int8_t *logits;
  LlamaPos all_pos_0;
  LlamaPos all_pos_1;
  int all_seq_id;
} llama_batch;

typedef struct MtmdContextParams {
  bool use_gpu;
  bool print_timings;
  int n_threads;
  const char *image_marker;
  const char *media_marker;
  int flash_attn_type;
  bool warmup;
  int image_min_tokens;
  int image_max_tokens;
} MtmdContextParams;

typedef struct MtmdContext {
  uint8_t _private[0];
} MtmdContext;

typedef struct MtmdBitmap {
  uint8_t _private[0];
} MtmdBitmap;

typedef struct MtmdInputChunks {
  uint8_t _private[0];
} MtmdInputChunks;

typedef struct MtmdInputText {
  const char *text;
  bool add_special;
  bool parse_special;
} MtmdInputText;

typedef int MtmdLlamaPos;

typedef int MtmdLlamaSeqId;

typedef struct llama_sampler {
  uint8_t _private[0];
} llama_sampler;

typedef struct llama_sampler_chain_params {
  bool no_perf_fac;
} llama_sampler_chain_params;

typedef struct llama_token_data {
  LlamaToken id;
  float logit;
  float p;
} llama_token_data;

typedef struct llama_token_data_array {
  struct llama_token_data *data;
  uintptr_t size;
  bool sorted;
} llama_token_data_array;

typedef struct gpuf_multimodal_model {
  struct llama_model *text_model;
  struct MtmdContext *mtmd_context;
  enum ProjectorType projector_type;
  const struct llama_vocab *vocab;
  bool is_multimodal;
  CString _media_marker;
} gpuf_multimodal_model;

/**
 * Token callback: called for each generated token
 * Parameters: user_data, token_text, token_id
 */
typedef void (*TokenCallback)(void*, const char*, int);

/**
 * Completion callback: called when generation completes
 * Parameters: user_data, full_text, token_count
 */
typedef void (*CompletionCallback)(void*, const char*, int);

extern int llama_backend_init(void);

extern void llama_backend_free(void);

extern struct llama_model *llama_load_model_from_file(const char *path,
                                                      struct llama_model_params params);

extern struct llama_context *llama_init_from_model(const struct llama_model *model,
                                                   struct llama_context_params params);

extern const struct llama_model *llama_get_model(const struct llama_context *ctx);

extern int llama_tokenize(const struct llama_vocab *vocab,
                          const char *text,
                          int text_len,
                          LlamaToken *tokens,
                          int n_tokens_max,
                          bool add_bos,
                          bool parse_special);

extern int llama_decode(struct llama_context *ctx, const struct llama_batch *batch);

extern struct MtmdContextParams mtmd_context_params_default(void);

extern struct MtmdContext *mtmd_init_from_file(const char *mmproj_fname,
                                               const struct llama_model *text_model,
                                               struct MtmdContextParams ctx_params);

extern void mtmd_free(struct MtmdContext *ctx);

extern bool mtmd_support_vision(struct MtmdContext *ctx);

extern struct MtmdBitmap *mtmd_bitmap_init(uint32_t nx, uint32_t ny, const uint8_t *data);

extern void mtmd_bitmap_free(struct MtmdBitmap *bitmap);

extern struct MtmdInputChunks *mtmd_input_chunks_init(void);

extern void mtmd_input_chunks_free(struct MtmdInputChunks *chunks);

extern int mtmd_tokenize(struct MtmdContext *ctx,
                         struct MtmdInputChunks *output,
                         const struct MtmdInputText *text,
                         struct MtmdBitmap *const *bitmaps,
                         uintptr_t n_bitmaps);

extern int mtmd_encode_chunk(struct MtmdContext *ctx, const void *chunk);

extern int mtmd_helper_eval_chunks(struct MtmdContext *ctx,
                                   struct llama_context *lctx,
                                   void *chunks,
                                   MtmdLlamaPos n_past,
                                   MtmdLlamaSeqId seq_id,
                                   int n_batch,
                                   bool logits_last,
                                   MtmdLlamaPos *new_n_past);

extern float *mtmd_get_output_embd(struct MtmdContext *ctx);

extern struct llama_sampler *llama_sampler_init_top_k(int k);

extern struct llama_sampler *llama_sampler_init_top_p(float p, uintptr_t min_keep);

extern struct llama_sampler *llama_sampler_init_temp(float t);

extern struct llama_sampler *llama_sampler_init_dist(uint32_t seed);

extern struct llama_sampler *llama_sampler_init_greedy(void);

extern struct llama_sampler *llama_sampler_init_penalties(int penalty_last_n,
                                                          float penalty_repeat,
                                                          float penalty_freq,
                                                          float penalty_present);

extern int llama_vocab_n_tokens(const struct llama_vocab *vocab);

extern int llama_n_batch(struct llama_context *ctx);

extern struct llama_batch llama_batch_init(int n_tokens, int embd, int n_seq_max);

extern void llama_batch_free(struct llama_batch batch);

extern struct llama_batch llama_batch_get_one(const LlamaToken *token,
                                              int n_tokens,
                                              LlamaPos pos_0,
                                              int seq_id);

extern void *llama_get_memory(struct llama_context *ctx);

extern bool llama_memory_seq_rm(void *mem, int seq_id, LlamaPos p0, LlamaPos p1);

extern void llama_memory_clear(void *mem, bool data);

extern struct llama_sampler *llama_sampler_chain_init(struct llama_sampler_chain_params params);

extern void llama_sampler_chain_add(struct llama_sampler *chain, struct llama_sampler *sampler);

extern LlamaToken llama_sampler_sample(struct llama_sampler *sampler,
                                       struct llama_context *ctx,
                                       int idx);

extern void llama_sampler_free(struct llama_sampler *sampler);

extern void llama_sampler_apply(struct llama_sampler *sampler,
                                struct llama_token_data_array *candidates);

extern int llama_n_ctx(const struct llama_context *ctx);

extern int llama_n_vocab(struct llama_context *ctx);

extern LlamaToken llama_token_bos(const struct llama_model *model);

extern LlamaToken llama_token_eos(const struct llama_model *model);

extern const struct llama_vocab *llama_model_get_vocab(const struct llama_model *model);

extern int llama_token_to_piece(const struct llama_vocab *vocab,
                                LlamaToken token,
                                char *buf,
                                int length,
                                int lstrip,
                                bool special);

extern const char *llama_vocab_get_text(const struct llama_vocab *vocab, LlamaToken token);

extern bool llama_vocab_is_control(const struct llama_vocab *vocab, LlamaToken token);

extern bool llama_vocab_is_eog(const struct llama_vocab *vocab, LlamaToken token);

extern const float *llama_get_logits(struct llama_context *ctx);

extern void llama_model_free(struct llama_model *model);

extern void llama_free(struct llama_context *ctx);

extern void *ggml_backend_dev_by_type(int32_t type_);

extern void *ggml_backend_dev_get(int32_t i);

extern int32_t ggml_backend_dev_count(void);

extern void ggml_backend_load_all(void);

extern struct llama_model_params llama_model_default_params(void);

extern struct llama_context_params llama_context_default_params(void);

struct llama_context *gpuf_create_context(struct llama_model *model);

/**
 * Start async model loading (realistic implementation)
 */
bool gpuf_load_model_async_start(const char *path);

/**
 * Get loading status (realistic polling)
 */
int32_t gpuf_load_model_get_status(void);

/**
 * Get loading progress (limited but realistic)
 */
float gpuf_load_model_get_progress(void);

/**
 * Check if loading is complete
 */
bool gpuf_load_model_is_complete(void);

/**
 * Check if loading has error
 */
bool gpuf_load_model_has_error(void);

/**
 * Get loaded model pointer (only valid after completion)
 */
struct llama_model *gpuf_load_model_get_result(void);

/**
 * Wait for loading to complete (blocking)
 */
int32_t gpuf_load_model_wait(void);

/**
 * Cleanup async loading state
 */
void gpuf_load_model_cleanup(void);

/**
 * Legacy async model loading with callback (for backward compatibility)
 */
struct llama_model *gpuf_load_model_async(const char *path,
                                          void (*on_progress)(float, void*),
                                          void *user_data);

/**
 * Context creation remains synchronous (fast operation)
 * Use the regular gpuf_create_context for context creation
 */
struct llama_context *gpuf_create_context_async(struct llama_model *model,
                                                void (*on_progress)(float, void*),
                                                void *user_data);

/**
 * Check if model is loaded (non-blocking)
 */
bool gpuf_is_model_loaded(void);

/**
 * Check if context is created (non-blocking)
 */
bool gpuf_is_context_ready(void);

/**
 * Get model loading status
 */
int gpuf_get_model_status(void);

struct llama_model *gpuf_load_model(const char *path);

struct gpuf_multimodal_model *gpuf_load_multimodal_model(const char *text_model_path,
                                                         const char *mmproj_path);

struct llama_context *gpuf_create_multimodal_context(struct gpuf_multimodal_model *multimodal_model);

int gpuf_generate_multimodal(struct gpuf_multimodal_model *multimodal_model,
                             struct llama_context *ctx,
                             const char *text_prompt,
                             const uint8_t *image_data,
                             unsigned long long image_size,
                             int max_tokens,
                             float temperature,
                             int top_k,
                             float top_p,
                             float repeat_penalty,
                             char *output,
                             int output_len);

int gpuf_generate_multimodal_stream(struct gpuf_multimodal_model *multimodal_model,
                                    struct llama_context *ctx,
                                    const char *text_prompt,
                                    const uint8_t *image_data,
                                    unsigned long long image_size,
                                    int max_tokens,
                                    float temperature,
                                    int top_k,
                                    float top_p,
                                    float repeat_penalty,
                                    TokenCallback on_token,
                                    CompletionCallback on_complete,
                                    void *user_data);

void gpuf_free_multimodal_model(struct gpuf_multimodal_model *multimodal_model);

bool gpuf_multimodal_supports_vision(struct gpuf_multimodal_model *multimodal_model);

int gpuf_get_multimodal_info(struct gpuf_multimodal_model *multimodal_model, bool *has_vision);

int gpuf_get_vision_tokens(struct gpuf_multimodal_model *multimodal_model,
                           char *start_token,
                           char *end_token,
                           char *media_token,
                           int max_length);

int gpuf_generate_final_solution_text(const struct llama_model *model,
                                      struct llama_context *ctx,
                                      const char *prompt,
                                      int _max_tokens,
                                      char *output,
                                      int output_len);

int gpuf_generate_with_sampling(const struct llama_model *model,
                                struct llama_context *ctx,
                                const char *prompt,
                                int max_tokens,
                                float temperature,
                                int top_k,
                                float top_p,
                                float repeat_penalty,
                                char *output,
                                int output_len,
                                LlamaToken *token_buffer,
                                int token_buffer_size);

const char *gpuf_system_info(void);

const char *gpuf_version(void);

int gpuf_init(void);

int gpuf_cleanup(void);

/**
 * Stop ongoing generation
 */
int gpuf_stop_generation(struct llama_context *_ctx);

/**
 * Start async generation with streaming callback (simplified version)
 */
int gpuf_start_generation_async(struct llama_context *ctx,
                                const char *prompt,
                                int max_tokens,
                                float temperature,
                                int top_k,
                                float top_p,
                                float repeat_penalty,
                                void (*on_token_callback)(const char*, void*),
                                void *user_data);

/**
 * Simple single token generation for testing
 */
int gpuf_generate_single_token(const struct llama_model *model,
                               struct llama_context *ctx,
                               const char *prompt,
                               char *output,
                               int output_len);

/**
 * Start remote worker and initialize global worker (C API)
 */
int start_remote_worker(const char *server_addr,
                        int control_port,
                        int proxy_port,
                        const char *worker_type,
                        const char *client_id);

/**
 * Set remote worker model (C API) - Safe Hot Swapping Version
 *
 * This function supports safe hot swapping without stopping the worker.
 * Uses coordination mutex to ensure no inference requests access freed memory.
 *
 * # Parameters
 * - `model_path`: Path to the model file (.gguf)
 *
 * # Returns
 * - `0`: Success (model loaded and context created)
 * - `-1`: Backend initialization failed
 * - `-2`: Path conversion failed
 * - `-3`: Model loading failed
 * - `-4`: Context creation failed
 *
 * # Safety
 * Caller must ensure `model_path` is a valid null-terminated C string
 *
 * # Hot Swapping
 * This function can be called multiple times without stopping the worker.
 * Inference requests will be briefly paused during the swap but the worker
 * remains connected and continues processing afterward.
 */
int set_remote_worker_model(const char *model_path);

/**
 * Start remote worker background tasks (C API)
 */
int start_remote_worker_tasks(void);

/**
 * Start remote worker background tasks with callback support (C API)
 */
int start_remote_worker_tasks_with_callback_ptr(void (*callback)(const char*, void*));

/**
 * Stop remote worker and cleanup (C API)
 */
int stop_remote_worker(void);

/**
 * Get remote worker status (C API)
 *
 * # Parameters
 * - `buffer`: Output buffer to write status string
 * - `buffer_size`: Size of the output buffer
 *
 * # Returns
 * - `0`: Success (status written to buffer)
 * - `-1`: Error (buffer too small or other error)
 *
 * # Safety
 * Caller must ensure `buffer` is valid and can hold `buffer_size` bytes
 */
int get_remote_worker_status(char *buffer, size_t buffer_size);

/**
 * Sets the model path for the remote worker (hot swapping support)
 *
 * Java signature:
 * public static native int setRemoteWorkerModel(String modelPath);
 *
 * @param modelPath Path to the GGUF model file
 * @return 0 on success, -1 on failure
 */
jint Java_com_gpuf_c_RemoteWorker_setRemoteWorkerModel(JNIEnv env,
                                                       JClass _class,
                                                       JString model_path);

jint Java_com_gpuf_c_RemoteWorker_registerCallbackEmitter(JNIEnv env,
                                                          JClass _class,
                                                          JObject emitter);

jint Java_com_gpuf_c_RemoteWorker_startRemoteWorkerTasksWithJavaCallback(JNIEnv _env,
                                                                         JClass _class);

/**
 * Starts the remote worker connection to the server
 *
 * Java signature:
 * public static native int startRemoteWorker(
 *     String serverAddr,
 *     int controlPort,
 *     int proxyPort,
 *     String workerType,
 *     String clientId
 * );
 *
 * @param serverAddr Server IP address or hostname
 * @param controlPort Control port number
 * @param proxyPort Proxy port number
 * @param workerType Worker type ("TCP" or "WS")
 * @param clientId Client ID (32 hex characters)
 * @return 0 on success, -1 on failure
 */
jint Java_com_gpuf_c_RemoteWorker_startRemoteWorker(JNIEnv env,
                                                    JClass _class,
                                                    JString server_addr,
                                                    jint control_port,
                                                    jint proxy_port,
                                                    JString worker_type,
                                                    JString client_id);

/**
 * Starts the background tasks for the remote worker with optional callback
 *
 * Java signature:
 * public static native int startRemoteWorkerTasks(long callbackFunctionPtr);
 *
 * @param callbackFunctionPtr Optional function pointer for status updates
 * @return 0 on success, -1 on failure
 */
jint Java_com_gpuf_c_RemoteWorker_startRemoteWorkerTasks(JNIEnv _env,
                                                         JClass _class,
                                                         jlong callback_function_ptr);

/**
 * Gets the current status of the remote worker
 *
 * Java signature:
 * public static native String getRemoteWorkerStatus();
 *
 * @return Status string or null on failure
 */
jstring Java_com_gpuf_c_RemoteWorker_getRemoteWorkerStatus(JNIEnv env, JClass _class);

/**
 * Stops the remote worker and cleans up resources
 *
 * Java signature:
 * public static native int stopRemoteWorker();
 *
 * @return 0 on success, -1 on failure
 */
jint Java_com_gpuf_c_RemoteWorker_stopRemoteWorker(JNIEnv _env, JClass _class);

/**
 * Initialize the GPUFabric engine
 *
 * Java signature:
 * public static native int initialize();
 */
jint Java_com_gpuf_c_GPUEngine_initialize(JNIEnv _env, JClass _class);

/**
 * Get GPUFabric version string
 *
 * Java signature:
 * public static native String getVersion();
 */
jstring Java_com_gpuf_c_GPUEngine_getVersion(JNIEnv env, JClass _class);

/**
 * Cleanup and free resources
 *
 * Java signature:
 * public static native int cleanup();
 */
jint Java_com_gpuf_c_GPUEngine_cleanup(JNIEnv _env, JClass _class);

/**
 * Get system information
 *
 * Java signature:
 * public static native String getSystemInfo();
 */
jstring Java_com_gpuf_c_GPUEngine_getSystemInfo(JNIEnv env, JClass _class);

/**
 * Load a LLaMA model from file
 *
 * Java signature:
 * public static native long loadModel(String modelPath);
 *
 * Returns: model pointer as long, or 0 on failure
 */
jlong Java_com_gpuf_c_GPUEngine_loadModel(JNIEnv env, JClass _class, JString model_path);

/**
 * Create inference context for a model
 *
 * Java signature:
 * public static native long createContext(long modelPtr);
 *
 * Returns: context pointer as long, or 0 on failure
 */
jlong Java_com_gpuf_c_GPUEngine_createContext(JNIEnv _env, JClass _class, jlong model_ptr);

/**
 * Check if model is loaded
 *
 * Java signature:
 * public static native boolean isModelLoaded();
 */
jboolean Java_com_gpuf_c_GPUEngine_isModelLoaded(JNIEnv _env, JClass _class);

/**
 * Check if context is ready
 *
 * Java signature:
 * public static native boolean isContextReady();
 */
jboolean Java_com_gpuf_c_GPUEngine_isContextReady(JNIEnv _env, JClass _class);

/**
 * Get model loading status
 *
 * Java signature:
 * public static native String getModelStatus();
 *
 * Returns: "not_loaded", "loading", "ready", "error", or "unknown"
 */
jstring Java_com_gpuf_c_GPUEngine_getModelStatus(JNIEnv env, JClass _class);

/**
 * Start inference service with model loading
 *
 * Java signature:
 * public static native int startInferenceService(String modelPath, int port);
 */
jint Java_com_gpuf_c_GPUEngine_startInferenceService(JNIEnv env,
                                                     JClass _class,
                                                     JString model_path,
                                                     jint _port);

/**
 * Start inference service asynchronously with progress callback
 *
 * Java signature:
 * public static native int startInferenceServiceAsync(String modelPath, int port, Object progressCallback);
 */
jint Java_com_gpuf_c_GPUEngine_startInferenceServiceAsync(JNIEnv env,
                                                          JClass _class,
                                                          JString model_path,
                                                          jint _port,
                                                          JObject progress_callback);

/**
 * Stop inference service
 *
 * Java signature:
 * public static native int stopInferenceService();
 */
jint Java_com_gpuf_c_GPUEngine_stopInferenceService(JNIEnv _env, JClass _class);

/**
 * Load model dynamically (alternative method)
 *
 * Java signature:
 * public static native int loadModelNew(String modelPath);
 */
jint Java_com_gpuf_c_GPUEngine_loadModelNew(JNIEnv env, JClass _class, JString model_path);

/**
 * Get current loaded model path
 *
 * Java signature:
 * public static native String getCurrentModel();
 */
jstring Java_com_gpuf_c_GPUEngine_getCurrentModel(JNIEnv env, JClass _class);

/**
 * Get model loading status string
 *
 * Java signature:
 * public static native String getModelLoadingStatus();
 */
jstring Java_com_gpuf_c_GPUEngine_getModelLoadingStatus(JNIEnv env, JClass _class);

/**
 * Generate text using loaded model (basic version)
 *
 * Java signature:
 * public static native int generate(long modelPtr, long contextPtr, String prompt, int maxTokens, Object outputBuffer);
 */
jint Java_com_gpuf_c_GPUEngine_generate(JNIEnv env,
                                        JClass _class,
                                        jlong model_ptr,
                                        jlong context_ptr,
                                        JString prompt,
                                        jint max_tokens,
                                        JObject _output_buffer);

/**
 * Generate text using global model
 *
 * Java signature:
 * public static native String generateText(String prompt, int maxTokens);
 */
jstring Java_com_gpuf_c_GPUEngine_generateText(JNIEnv env,
                                               JClass _class,
                                               JString prompt,
                                               jint max_tokens);

/**
 * Generate text with sampling parameters
 *
 * Java signature:
 * public static native String generateTextWithSampling(String prompt, int maxTokens, float temperature, int topK, float topP, float repeatPenalty);
 */
jstring Java_com_gpuf_c_GPUEngine_generateTextWithSampling(JNIEnv env,
                                                           JClass _class,
                                                           JString prompt,
                                                           jint max_tokens,
                                                           jfloat temperature,
                                                           jint top_k,
                                                           jfloat top_p,
                                                           jfloat repeat_penalty);

/**
 * Check inference service health
 *
 * Java signature:
 * public static native String isInferenceServiceHealthy();
 */
jstring Java_com_gpuf_c_GPUEngine_isInferenceServiceHealthy(JNIEnv env, JClass _class);

/**
 * Start async generation with streaming callback
 *
 * Java signature:
 * public static native int startGenerationAsync(long ctxPtr, String prompt, int maxTokens, float temperature, int topK, float topP, float repeatPenalty, long callbackFunctionPtr);
 */
jint Java_com_gpuf_c_GPUEngine_startGenerationAsync(JNIEnv env,
                                                    JClass _class,
                                                    jlong ctx_ptr,
                                                    JString prompt,
                                                    jint max_tokens,
                                                    jfloat temperature,
                                                    jint top_k,
                                                    jfloat top_p,
                                                    jfloat repeat_penalty,
                                                    jlong callback_function_ptr);

/**
 * Stop ongoing generation
 *
 * Java signature:
 * public static native int stopGeneration(long ctxPtr);
 */
jint Java_com_gpuf_c_GPUEngine_stopGeneration(JNIEnv _env, JClass _class, jlong ctx_ptr);

/**
 * Check if generation can be started
 *
 * Java signature:
 * public static native boolean canStartGeneration(long ctxPtr);
 */
jboolean Java_com_gpuf_c_GPUEngine_canStartGeneration(JNIEnv _env, JClass _class, jlong ctx_ptr);

/**
 * Get current generation status
 *
 * Java signature:
 * public static native String getGenerationStatus();
 */
jstring Java_com_gpuf_c_GPUEngine_getGenerationStatus(JNIEnv env, JClass _class);

/**
 * Load multimodal model (text model + mmproj)
 *
 * Java signature:
 * public static native long loadMultimodalModel(String textModelPath, String mmprojPath);
 */
jlong Java_com_gpuf_c_GPUEngine_loadMultimodalModel(JNIEnv env,
                                                    JClass _class,
                                                    JString text_model_path,
                                                    JString mmproj_path);

/**
 * Create context for multimodal model
 *
 * Java signature:
 * public static native long createMultimodalContext(long multimodalModelPtr);
 */
jlong Java_com_gpuf_c_GPUEngine_createMultimodalContext(JNIEnv _env,
                                                        JClass _class,
                                                        jlong multimodal_model_ptr);

/**
 * Generate with multimodal input (text + image)
 *
 * Java signature:
 * public static native String generateMultimodal(long multimodalModelPtr, long ctxPtr, String textPrompt, byte[] imageData, int maxTokens, float temperature, int topK, float topP);
 */
jstring Java_com_gpuf_c_GPUEngine_generateMultimodal(JNIEnv env,
                                                     JClass _class,
                                                     jlong multimodal_model_ptr,
                                                     jlong ctx_ptr,
                                                     JString text_prompt,
                                                     jbyteArray image_data,
                                                     jint max_tokens,
                                                     jfloat temperature,
                                                     jint top_k,
                                                     jfloat top_p);

/**
 * Check if multimodal model supports vision
 *
 * Java signature:
 * public static native boolean supportsVision(long multimodalModelPtr);
 */
jboolean Java_com_gpuf_c_GPUEngine_supportsVision(JNIEnv _env,
                                                  JClass _class,
                                                  jlong multimodal_model_ptr);

/**
 * Free multimodal model
 *
 * Java signature:
 * public static native void freeMultimodalModel(long multimodalModelPtr);
 */
void Java_com_gpuf_c_GPUEngine_freeMultimodalModel(JNIEnv _env,
                                                   JClass _class,
                                                   jlong multimodal_model_ptr);

#endif /* GPUF_C_H */
