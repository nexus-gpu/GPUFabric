# 🎬 GPUFabric Streaming API Guide

## Overview

GPUFabric supports **streaming generation**, allowing real-time reception of each generated token, providing a ChatGPT-like typing effect. Supports both pure text and multimodal generation.

## 🆕 Streaming APIs

### Text Streaming Generation
```c
int gpuf_start_generation_async(
    llama_context* ctx,
    const char* prompt,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    void (*on_token)(const char* token, void* user_data),
    void* user_data
);
```

### Multimodal Streaming Generation
```c
int gpuf_generate_multimodal_stream(
    gpuf_multimodal_model* model,
    llama_context* ctx,              // Can pass NULL, auto-create
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
```

## 📝 Usage Examples

### Basic Text Streaming Generation
```c
#include <stdio.h>

// Define Token callback
void my_token_callback(const char* token, void* user_data) {
    printf("%s", token);  // Real-time printing
    fflush(stdout);
}

int main() {
    // Load model and create context
    llama_context* ctx = create_context(...);
    
    // Build Llama 3.2 chat template prompt
    const char* prompt = 
        "<|begin_of_text|><|start_header_id|>user<|end_header_id|>\n\n"
        "Hello, how are you?<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n";
    
    // 🔑 Call streaming API
    int result = gpuf_start_generation_async(
        ctx,
        prompt,
        100,   // max_tokens
        0.7f,  // temperature
        40,    // top_k
        0.9f,  // top_p
        1.1f,  // repeat_penalty
        my_token_callback,  // Token callback
        NULL                // user_data
    );
    
    // Clean up
    free_context(ctx);
    return 0;
}
```

### Multimodal Streaming Generation
```c
// Define callbacks
void multimodal_token_callback(void* user_data, const char* token, int token_id) {
    printf("[Token %d] %s", token_id, token);
    fflush(stdout);
}

void complete_callback(void* user_data, const char* full_text, int token_count) {
    printf("\n✅ Complete: %d tokens\n", token_count);
}

int main() {
    // Load multimodal model
    gpuf_multimodal_model* model = gpuf_load_multimodal_model(
        "/path/to/model.gguf",
        "/path/to/mmproj.gguf"
    );
    
    // Load image
    size_t image_size = 0;
    unsigned char* image_data = load_rgb_file("test.rgb", &image_size);
    
    // Build prompt
    char prompt[1024];
    snprintf(prompt, sizeof(prompt), 
        "User\nDescribe this image.<__media__>\n"
        "Assistant\n");
    
    // 🔑 Call multimodal streaming API
    int result = gpuf_generate_multimodal_stream(
        model,
        NULL,  // Auto-create context
        prompt,
        image_data,
        image_size,
        100,   // max_tokens
        0.7f,  // temperature
        40,    // top_k
        0.9f,  // top_p
        1.1f,  // repeat_penalty
        multimodal_token_callback,
        complete_callback,
        NULL                    // user_data
    );
    
    // Clean up
    gpuf_free_multimodal_model(model);
    return 0;
}
```

### Advanced Example: With State Tracking
```c
typedef struct {
    int token_count;
    long long start_time;
    char accumulated_text[4096];
} StreamContext;

void advanced_token_callback(const char* token, void* user_data) {
    StreamContext* ctx = (StreamContext*)user_data;
    ctx->token_count++;
    
    // Accumulate text
    strcat(ctx->accumulated_text, token);
    
    // Real-time display
    printf("%s", token);
    fflush(stdout);
}

void advanced_complete_callback(void* user_data, const char* full_text, int token_count) {
    StreamContext* ctx = (StreamContext*)user_data;
    long long elapsed = get_time_ms() - ctx->start_time;
    
    printf("\n\n=== Statistics ===\n");
    printf("Tokens: %d\n", token_count);
    printf("Time: %lld ms\n", elapsed);
    printf("Speed: %.2f tokens/s\n", token_count * 1000.0 / elapsed);
}

int main() {
    StreamContext ctx = {
        .token_count = 0,
        .start_time = get_time_ms(),
        .accumulated_text = {0}
    };
    
    gpuf_start_generation_async(
        ctx, prompt, max_tokens, temperature, top_k, top_p, repeat_penalty,
        advanced_token_callback,
        &ctx  // Pass context
    );
    
    return 0;
}
```

## 🆚 API Comparison

### Original API (Blocking)
```c
char output[4096];
int result = gpuf_generate_text(
    ctx, prompt, max_tokens, temperature, top_k, top_p, repeat_penalty,
    output, sizeof(output)  // Output buffer
);

printf("Result: %s\n", output);  // One-time output

**Features:**
- ✅ Simple to use
- ❌ Blocking wait
- ❌ Cannot display in real-time
- ❌ Cannot cancel midway

### New API (Streaming Callback)
```c
gpuf_start_generation_async(
    ctx, prompt, max_tokens, temperature, top_k, top_p, repeat_penalty,
    on_token,      // Token callback
    user_data
);
```

**Features:**
- ✅ Real-time feedback
- ✅ Streaming experience
- ✅ Trackable progress
- ✅ Flexible state management

## 🎯 Usage Scenarios

### 1. Real-time UI Updates
```c
void token_callback(const char* token, void* user_data) {
    // Update UI display
    update_text_view(token);
}
```

### 2. Progress Tracking
```c
void progress_callback(const char* token, void* user_data) {
    int* token_count = (int*)user_data;
    (*token_count)++;
    
    // Update progress bar
    update_progress_bar(*token_count, max_tokens);
}
```

### 3. Log Recording
```c
void logging_callback(const char* token, void* user_data) {
    FILE* log_file = (FILE*)user_data;
    
    // Record each token
    fputs(token, log_file);
    fflush(log_file);
}
```

## 📊 Performance Considerations

### Callback Overhead
- Each token triggers a callback
- Callbacks should return quickly to avoid blocking generation
- Avoid performing time-consuming operations within callbacks

### Best Practices
```c
// ✅ Good practice
void fast_callback(const char* token, void* user_data) {
    // Fast operation: update in-memory state
    append_to_buffer(token);
}

// ❌ Avoid this
void slow_callback(const char* token, void* user_data) {
    // Slow operation: write file every time
    FILE* f = fopen("log.txt", "a");  // Frequent file opening
    fputs(token, f);
    fclose(f);  // Expensive operation
}
```

## 🔧 Compilation and Testing

### Android Compilation Example
```bash
cd /home/jack/codedir/GPUFabric/gpuf-c/examples

# Compile streaming test
./build.sh test_async_real.c

# Push to device
adb push test_async_real /data/local/tmp/

# Run
adb shell "cd /data/local/tmp && ./test_async_real"
```

### Expected Output
```
🚀 Real Async Inference Test
=============================
🔧 Initializing GPUFabric...
✅ Initialization successful
📦 Loading model...
✅ Model loading successful
🔧 Creating context...
✅ Context creation successful

📝 Test 1: Simple Q&A
====================
Hello, how are you?
I'm doing well, thank you for asking!...

✅ Generation successful!
Generated 15 tokens in 1234 ms
Speed: 12.15 tokens/s
Speed: 5.47 tokens/s
========================================
```

## 📚 Complete Examples

Reference files:
- `examples/test_async_real.c` - Text streaming generation example
- `examples/test_multimodal_streaming.c` - Multimodal streaming example  
- `examples/test_callback_only.c` - Basic callback test

## 🎉 Summary

Streaming API provides:
- ✅ Real-time feedback
- ✅ Better user experience
- ✅ Flexible state management
- ✅ Backward compatible (original API still available)

Start using streaming API to make your application smoother!

---

**Status**: ✅ Streaming API fully available  
**Last Updated**: 2024-12-10
