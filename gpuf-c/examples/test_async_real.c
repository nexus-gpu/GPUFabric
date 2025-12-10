/**
 * Real async inference test - using real model and context
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct llama_context llama_context;
typedef struct llama_model llama_model;

// API declarations
extern int gpuf_init(void);
extern llama_model* gpuf_load_model(const char* path);
extern llama_context* gpuf_create_context(llama_model* model);
extern void llama_free(llama_context* ctx);
extern void llama_free_model(llama_model* model);

extern int gpuf_start_generation_async(
    llama_context* ctx,
    const char* prompt,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    void (*on_token_callback)(const char* token, void* user_data),
    void* user_data
);

extern int gpuf_stop_generation(llama_context* ctx);

// User data structure
typedef struct {
    int token_count;
    long long start_time;
    char buffer[4096];
} GenerationContext;

static long long get_time_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

// Token callback function
void token_callback(const char* token, void* user_data) {
    GenerationContext* ctx = (GenerationContext*)user_data;
    ctx->token_count++;
    
    printf("%s", token);
    fflush(stdout);
    
    // Accumulate to buffer
    if (strlen(ctx->buffer) + strlen(token) < sizeof(ctx->buffer) - 1) {
        strcat(ctx->buffer, token);
    }
}

int main() {
    printf("\n");
    printf("╔════════════════════════════════════════╗\n");
    printf("║  🚀 Real Async Inference Test           ║\n");
    printf("║  gpuf_start_generation_async          ║\n");
    printf("╚════════════════════════════════════════╝\n");
    printf("\n");
    
    // Initialize
    printf("🔧 Initializing GPUFabric...\n");
    int init_result = gpuf_init();
    if (init_result < 0) {
        printf("❌ Initialization failed: %d\n", init_result);
        return 1;
    }
    printf("✅ Initialization successful (return value: %d)\n\n", init_result);
    
    const char* model_path = "/data/local/tmp/SmolVLM-500M-Instruct-Q8_0.gguf";
    
    // Load model
    printf("📦 Loading model...\n");
    llama_model* model = gpuf_load_model(model_path);
    if (!model) {
        printf("❌ Model loading failed\n");
        return 1;
    }
    printf("✅ Model loading successful: %p\n", (void*)model);
    
    // Create context
    printf("🔧 Creating context...\n");
    llama_context* ctx = gpuf_create_context(model);
    if (!ctx) {
        printf("❌ Context creation failed\n");
        llama_free_model(model);
        return 1;
    }
    printf("✅ Context creation successful: %p\n\n", (void*)ctx);
    
    // Test 1: Simple Q&A
    printf("════════════════════════════════════════\n");
    printf("📝 Test 1: Simple Q&A\n");
    printf("════════════════════════════════════════\n");
    
    GenerationContext gen_ctx1 = {
        .token_count = 0,
        .start_time = get_time_ms(),
        .buffer = {0}
    };
    
    const char* prompt1 = "<|begin_of_text|><|start_header_id|>user<|end_header_id|>\n\nHello, how are you?<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n";
    printf("Prompt: %s\n", prompt1);
    printf("Assistant: ");
    fflush(stdout);
    
    int result1 = gpuf_start_generation_async(
        ctx,
        prompt1,
        30,      // max_tokens
        0.7f,    // temperature
        40,      // top_k
        0.9f,    // top_p
        1.1f,    // repeat_penalty
        token_callback,
        &gen_ctx1
    );
    
    long long elapsed1 = get_time_ms() - gen_ctx1.start_time;
    
    printf("\n\n");
    printf("Result: %d\n", result1);
    printf("Generated tokens: %d\n", gen_ctx1.token_count);
    printf("Time elapsed: %lld ms\n", elapsed1);
    if (gen_ctx1.token_count > 0) {
        printf("Speed: %.2f tokens/s\n", gen_ctx1.token_count * 1000.0 / elapsed1);
    }
    printf("\n");
    
    // Test 2: Math problem
    printf("════════════════════════════════════════\n");
    printf("📝 Test 2: Math problem\n");
    printf("════════════════════════════════════════\n");
    
    GenerationContext gen_ctx2 = {
        .token_count = 0,
        .start_time = get_time_ms(),
        .buffer = {0}
    };
    
    const char* prompt2 = "<|begin_of_text|><|start_header_id|>user<|end_header_id|>\n\nWhat is 2+2?<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n";
    printf("Prompt: %s\n", prompt2);
    printf("Assistant: ");
    fflush(stdout);
    
    int result2 = gpuf_start_generation_async(
        ctx,
        prompt2,
        20,      // max_tokens
        0.5f,    // temperature (lower, more deterministic)
        20,      // top_k
        0.8f,    // top_p
        1.2f,    // repeat_penalty
        token_callback,
        &gen_ctx2
    );
    
    long long elapsed2 = get_time_ms() - gen_ctx2.start_time;
    
    printf("\n\n");
    printf("Result: %d\n", result2);
    printf("Generated tokens: %d\n", gen_ctx2.token_count);
    printf("Time elapsed: %lld ms\n", elapsed2);
    if (gen_ctx2.token_count > 0) {
        printf("Speed: %.2f tokens/s\n", gen_ctx2.token_count * 1000.0 / elapsed2);
    }
    printf("\n");
    
    // Test 3: No callback (should see output in logs)
    printf("════════════════════════════════════════\n");
    printf("📝 Test 3: No callback mode\n");
    printf("════════════════════════════════════════\n");
    
    const char* prompt3 = "Hi";
    printf("Prompt: %s\n", prompt3);
    printf("(Should see tokens in logs)\n\n");
    
    long long start3 = get_time_ms();
    
    int result3 = gpuf_start_generation_async(
        ctx,
        prompt3,
        15,      // max_tokens
        0.7f,
        40,
        0.9f,
        1.1f,
        NULL,    // No callback
        NULL
    );
    
    long long elapsed3 = get_time_ms() - start3;
    
    printf("\nResult: %d\n", result3);
    printf("Time elapsed: %lld ms\n\n", elapsed3);
    
    // Cleanup
    printf("🧹 Cleaning up resources...\n");
    llama_free(ctx);
    llama_free_model(model);
    printf("✅ Completed\n\n");
    
    // Summary
    printf("╔════════════════════════════════════════╗\n");
    printf("║  📊 Test Summary                       ║\n");
    printf("╚════════════════════════════════════════╝\n");
    printf("\n");
    printf("Test 1 (Simple Q&A):\n");
    printf("  - Status: %s\n", result1 >= 0 ? "✅ Success" : "❌ Failed");
    printf("  - Tokens: %d\n", gen_ctx1.token_count);
    if (gen_ctx1.token_count > 0) {
        printf("  - Speed: %.2f tokens/s\n", gen_ctx1.token_count * 1000.0 / elapsed1);
    }
    printf("\n");
    
    printf("Test 2 (Math problem):\n");
    printf("  - Status: %s\n", result2 >= 0 ? "✅ Success" : "❌ Failed");
    printf("  - Tokens: %d\n", gen_ctx2.token_count);
    if (gen_ctx2.token_count > 0) {
        printf("  - Speed: %.2f tokens/s\n", gen_ctx2.token_count * 1000.0 / elapsed2);
    }
    printf("\n");
    
    printf("Test 3 (No callback):\n");
    printf("  - Status: %s\n", result3 >= 0 ? "✅ Success" : "❌ Failed");
    printf("\n");
    
    printf("════════════════════════════════════════\n");
    printf("✅ All tests completed!\n");
    printf("════════════════════════════════════════\n");
    printf("\n");
    
    return 0;
}
