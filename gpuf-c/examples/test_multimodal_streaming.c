/**
 * Multimodal streaming test - real-time token output with images
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>

typedef struct gpuf_multimodal_model gpuf_multimodal_model;
typedef struct llama_context llama_context;

extern gpuf_multimodal_model* gpuf_load_multimodal_model(const char* text_model_path, const char* mmproj_path);
extern int gpuf_generate_multimodal_stream(
    gpuf_multimodal_model* model,
    llama_context* ctx,
    const char* text_prompt,
    const unsigned char* image_data,
    unsigned long long image_size,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    void (*on_token)(void* user_data, const char* token, int token_id),
    void (*on_complete)(void* user_data, const char* full_text, int token_count),
    void* user_data
);
extern void gpuf_free_multimodal_model(gpuf_multimodal_model* model);
extern int gpuf_get_vision_tokens(
    gpuf_multimodal_model* model,
    char* start_token,
    char* end_token,
    char* media_token,
    int max_length
);

// User data structure
typedef struct {
    int token_count;
    long long start_time;
    char accumulated_text[4096];
} StreamContext;

static long long get_time_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

// 🔑 Token callback - display each token in real time
void on_token_callback(void* user_data, const char* token, int token_id) {
    StreamContext* ctx = (StreamContext*)user_data;
    ctx->token_count++;
    
    // Real-time printing (ChatGPT-like effect)
    printf("%s", token);
    fflush(stdout);
    
    // Accumulate text
    strncat(ctx->accumulated_text, token, sizeof(ctx->accumulated_text) - strlen(ctx->accumulated_text) - 1);
}

// 🔑 Completion callback - show statistics
void on_complete_callback(void* user_data, const char* full_text, int token_count) {
    StreamContext* ctx = (StreamContext*)user_data;
    long long elapsed = get_time_ms() - ctx->start_time;
    
    printf("\n\n");
    printf("========================================\n");
    printf("✅ Generation completed!\n");
    printf("========================================\n");
    printf("Total tokens: %d\n", ctx->token_count);
    printf("Time elapsed: %lld ms\n", elapsed);
    printf("Speed: %.2f tokens/s\n", ctx->token_count * 1000.0 / elapsed);
    printf("========================================\n");
}

static uint8_t* load_rgb_file(const char* filename, size_t* out_size) {
    FILE* f = fopen(filename, "rb");
    if (!f) return NULL;
    
    fseek(f, 0, SEEK_END);
    size_t size = ftell(f);
    fseek(f, 0, SEEK_SET);
    
    uint8_t* data = (uint8_t*)malloc(size);
    if (!data) {
        fclose(f);
        return NULL;
    }
    
    fread(data, 1, size, f);
    fclose(f);
    
    *out_size = size;
    return data;
}

int main() {
    printf("\n");
    printf("╔════════════════════════════════════════╗\n");
    printf("║  🎬 Multimodal Streaming Test          ║\n");
    printf("║  Real-time Token-by-Token Output     ║\n");
    printf("╚════════════════════════════════════════╝\n");
    printf("\n");
    
    const char* text_model_path = "/data/local/tmp/Qwen2-VL-2B-Instruct-Q4_K_M.gguf";
    const char* mmproj_path = "/data/local/tmp/mmproj-Qwen2-VL-2B-Instruct-f16.gguf";
    const char* image_path = "/data/local/tmp/test_image.rgb";
    
    // Load model
    printf("📦 Loading model...\n");
    gpuf_multimodal_model* model = gpuf_load_multimodal_model(text_model_path, mmproj_path);
    if (!model) {
        printf("❌ Model loading failed\n");
        return 1;
    }
    printf("✅ Model loading successful\n");
    
    // Get vision tokens
    char media_token[64] = {0};
    gpuf_get_vision_tokens(model, NULL, NULL, media_token, sizeof(media_token));
    printf("🎯 Media token: %s\n", media_token);
    
    // Load image
    printf("🖼️  Loading image...\n");
    size_t image_size = 0;
    uint8_t* image_data = load_rgb_file(image_path, &image_size);
    if (!image_data) {
        printf(" Image loading failed\n");
        return 1;
    }
    printf(" Image loading successful: %zu bytes\n\n", image_size);
    
    // Test 1: Describe image
    printf("════════════════════════════════════════\n");
    printf(" Test 1: Describe this image\n");
    printf("════════════════════════════════════════\n");
    
    char prompt1[1024];
    snprintf(prompt1, sizeof(prompt1), 
        " system\nYou are a helpful assistant. \n"
        " user\n%s\nDescribe this image in detail. \n"
        " assistant\n", 
        media_token);
    
    StreamContext ctx1 = {
        .token_count = 0,
        .start_time = get_time_ms(),
        .accumulated_text = {0}
    };
    
    printf("\n Assistant: ");
    fflush(stdout);
    
    int result1 = gpuf_generate_multimodal_stream(
        model,
        NULL,  // Auto-create context
        prompt1,
        image_data,
        image_size,
        100,    // max_tokens
        0.7f,   // temperature
        40,     // top_k
        0.9f,   // top_p
        1.1f,   // repeat_penalty
        on_token_callback,
        on_complete_callback,
        &ctx1
    );
    
    if (result1 < 0) {
        printf("\n Generation failed\n");
    }
    
    // Test 2: Short question
    printf("\n════════════════════════════════════════\n");
    printf(" Test 2: What is this?\n");
    printf("════════════════════════════════════════\n");
    
    char prompt2[1024];
    snprintf(prompt2, sizeof(prompt2), 
        " user\n%s\nWhat is this? Answer in one sentence. \n"
        " assistant\n", 
        "<|im_start|>assistant\n", 
        media_token);
    
    StreamContext ctx2 = {
        .token_count = 0,
        .start_time = get_time_ms(),
        .accumulated_text = {0}
    };
    
    printf("\n🤖 Assistant: ");
    fflush(stdout);
    
    int result2 = gpuf_generate_multimodal_stream(
        model,
        NULL,
        prompt2,
        image_data,
        image_size,
        50,     // max_tokens
        0.3f,   // temperature (lower, more deterministic)
        20,     // top_k
        0.7f,   // top_p
        1.3f,   // repeat_penalty
        on_token_callback,
        on_complete_callback,
        &ctx2
    );
    
    if (result2 < 0) {
        printf("\n❌ 生成失败\n");
    }
    
    // Cleanup
    free(image_data);
    gpuf_free_multimodal_model(model);
    
    printf("\n");
    printf("╔════════════════════════════════════════╗\n");
    printf("║  ✅ All Tests Completed               ║\n");
    printf("╚════════════════════════════════════════╝\n");
    printf("\n");
    
    return 0;
}
