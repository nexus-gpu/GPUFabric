/**
 * Minimal Multimodal Test for Android
 * 
 * This program tests the core multimodal functions without complex initialization
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>

// GPUFabric C API Declarations
typedef struct gpuf_multimodal_model gpuf_multimodal_model;
typedef struct llama_context llama_context;

// Function declarations
extern gpuf_multimodal_model* gpuf_load_multimodal_model(const char* text_model_path, const char* mmproj_path);
extern llama_context* gpuf_create_multimodal_context(gpuf_multimodal_model* model);
extern int gpuf_generate_multimodal(
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
    char* output,
    int output_len
);
extern void gpuf_free_multimodal_model(gpuf_multimodal_model* model);
extern int gpuf_get_vision_tokens(
    gpuf_multimodal_model* model,
    char* start_token,
    char* end_token,
    char* media_token,
    int max_length
);

// Utility functions
static long long get_time_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static void print_header(const char* title) {
    printf("\n========================================\n");
    printf("  %s\n", title);
    printf("========================================\n");
}

int main(int argc, char** argv) {
    printf("\n🔥 Minimal Multimodal Test for Android\n");
    printf("Focus: gpuf_load_multimodal_model & gpuf_generate_multimodal\n\n");
    
    const char* text_model_path = "/data/local/tmp/Qwen2-VL-2B-Instruct-Q4_K_M.gguf";
    const char* mmproj_path = "/data/local/tmp/mmproj-Qwen2-VL-2B-Instruct-f16.gguf";
    
    gpuf_multimodal_model* model = NULL;
    llama_context* ctx = NULL;
    int test_failed = 0;
    
    // Test 1: Load multimodal model
    print_header("Test 1: gpuf_load_multimodal_model");
    printf("Loading models...\n");
    printf("Text model: %s\n", text_model_path);
    printf("MMProj: %s\n", mmproj_path);
    
    long long start = get_time_ms();
    model = gpuf_load_multimodal_model(text_model_path, mmproj_path);
    long long elapsed = get_time_ms() - start;
    
    if (!model) {
        printf("❌ Failed to load multimodal model\n");
        test_failed = 1;
        goto cleanup;
    }
    printf("✅ Model loaded successfully\n");
    
    // 🆕 Test vision token detection
    char start_token[64] = {0};
    char end_token[64] = {0};
    char media_token[64] = {0};
    int model_type = gpuf_get_vision_tokens(model, start_token, end_token, media_token, sizeof(start_token));
    printf("🎯 Detected model type: %d\n", model_type);
    if (strlen(start_token) > 0) {
        printf("  Vision tokens: %s ... %s\n", start_token, end_token);
    }
    if (strlen(media_token) > 0) {
        printf("  Media marker: %s\n", media_token);
    }
    printf("✅ Model loaded successfully in %lld ms\n", elapsed);
    printf("Model pointer: %p\n", (void*)model);
    
    // Test 2: Create context
    print_header("Test 2: gpuf_create_multimodal_context");
    printf("Creating context...\n");
    
    start = get_time_ms();
    ctx = gpuf_create_multimodal_context(model);
    elapsed = get_time_ms() - start;
    
    if (ctx == NULL) {
        printf("❌ gpuf_create_multimodal_context() failed - returned NULL\n");
        test_failed = 1;
        goto cleanup;
    }
    
    printf("✅ Context created successfully in %lld ms\n", elapsed);
    printf("Context pointer: %p\n", (void*)ctx);
    
    // Test 3: Text-only generation
    print_header("Test 3: gpuf_generate_multimodal (text-only)");
    const char* text_prompt = "Hello! Please introduce yourself briefly.";
    char output[2048] = {0};
    
    printf("Prompt: \"%s\"\n", text_prompt);
    printf("Generating response...\n");
    
    start = get_time_ms();
    int result = gpuf_generate_multimodal(
        model,
        ctx,
        text_prompt,
        NULL,   // No image data
        0,      // Image size = 0
        50,     // max_tokens
        0.7f,   // temperature
        40,     // top_k
        0.9f,   // top_p
        1.1f,   // repeat_penalty
        output,
        sizeof(output)
    );
    elapsed = get_time_ms() - start;
    
    printf("Return code: %d\n", result);
    printf("Generation time: %lld ms\n", elapsed);
    
    if (result > 0) {
        printf("\n--- Generated Text ---\n");
        printf("%s\n", output);
        printf("--- End ---\n\n");
        printf("Tokens generated: %d\n", result);
        printf("Speed: %.2f tokens/sec\n", result * 1000.0 / elapsed);
        printf("✅ Text-only generation successful\n");
    } else {
        printf("❌ Text-only generation failed with code: %d\n", result);
        test_failed = 1;
    }
    
    // Test 4: Multimodal generation (text + image)
    print_header("Test 4: gpuf_generate_multimodal (with image)");
    
    // Create a simple 224x224 RGB test image
    const int width = 224;
    const int height = 224;
    const int channels = 3;
    size_t image_size = width * height * channels;
    uint8_t* image_data = (uint8_t*)malloc(image_size);
    
    if (image_data) {
        // Create a more meaningful test image - red circle on white background
        for (int y = 0; y < height; y++) {
            for (int x = 0; x < width; x++) {
                int idx = (y * width + x) * channels;
                int center_x = width / 2;
                int center_y = height / 2;
                int radius = width / 4;
                int distance = (x - center_x) * (x - center_x) + (y - center_y) * (y - center_y);
                
                if (distance <= radius * radius) {
                    // Red circle
                    image_data[idx + 0] = 255;  // R
                    image_data[idx + 1] = 0;    // G
                    image_data[idx + 2] = 0;    // B
                } else {
                    // White background
                    image_data[idx + 0] = 255;  // R
                    image_data[idx + 1] = 255;  // G
                    image_data[idx + 2] = 255;  // B
                }
            }
        }
        
        // 🆕 Use proper chat template for Qwen models, plain text for others
        char image_prompt[1024];
        if (strstr(start_token, "<|vision_start|>") != NULL) {
            // Qwen2-VL needs chat template format
            snprintf(image_prompt, sizeof(image_prompt), 
                "<|im_start|>system\nYou are Qwen, created by Alibaba Cloud. You are a helpful assistant.<|im_end|>\n"
                "<|im_start|>user\nPlease look at this image and tell me what objects or shapes you can see. Describe the main colors and forms.%s<|im_end|>\n"
                "<|im_start|>assistant\n", 
                media_token);
        } else {
            // SmolVLM and others use plain text
            snprintf(image_prompt, sizeof(image_prompt), 
                "Please look at this image and tell me what objects or shapes you can see. Describe the main colors and forms.\n%s", 
                media_token);
        }
        memset(output, 0, sizeof(output));
        
        printf("Created test image: %dx%dx%d (%zu bytes)\n", width, height, channels, image_size);
        printf("Prompt: \"%s\"\n", image_prompt);
        printf("Generating response with image...\n");
        
        start = get_time_ms();
        result = gpuf_generate_multimodal(
            model,
            ctx,
            image_prompt,
            image_data,
            image_size,
            40,     // max_tokens
            0.7f,   // temperature (higher for more diverse output)
            40,    // top_k (more options)
            0.9f,   // top_p (more creative)
            1.15f,  // repeat_penalty (slight penalty)
            output,
            sizeof(output)
        );
        elapsed = get_time_ms() - start;
        
        free(image_data);
        
        printf("Return code: %d\n", result);
        printf("Generation time: %lld ms\n", elapsed);
        
        if (result > 0) {
            printf("\n--- Generated Text ---\n");
            printf("%s\n", output);
            printf("--- End ---\n\n");
            printf("Tokens generated: %d\n", result);
            printf("Speed: %.2f tokens/sec\n", result * 1000.0 / elapsed);
            printf("✅ Multimodal generation successful\n");
        } else {
            printf("❌ Multimodal generation failed with code: %d\n", result);
            test_failed = 1;
        }
    } else {
        printf("❌ Failed to allocate memory for test image\n");
        test_failed = 1;
    }
    
cleanup:
    // Cleanup
    print_header("Cleanup");
    if (model != NULL) {
        gpuf_free_multimodal_model(model);
        printf("✅ Model freed\n");
    }
    
    // Final result
    printf("\n========================================\n");
    if (test_failed) {
        printf("❌ SOME TESTS FAILED\n");
    } else {
        printf("✅ ALL TESTS PASSED\n");
    }
    printf("========================================\n\n");
    
    return test_failed ? 1 : 0;
}
