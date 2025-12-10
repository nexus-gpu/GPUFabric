# 🔧 Multimodal Generation Implementation Guide

## Overview

GPUFabric supports complete multimodal generation functionality, including image understanding, streaming output, and real-time callback mechanisms.

## 🎯 Core Features

### 1. Multimodal Model Loading
```c
gpuf_multimodal_model* model = gpuf_load_multimodal_model(
    "/path/to/model.gguf",      // Main model file
    "/path/to/mmproj.gguf"      // Vision projector file
);
```

### 2. Streaming Generation API
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

### 3. Callback Function Design
```c
// Token callback - called when each token is generated
void on_token_callback(void* user_data, const char* token, int token_id) {
    printf("%s", token);  // Real-time printing
    fflush(stdout);
}

// Completion callback - called when generation is complete
void on_complete_callback(void* user_data, const char* full_text, int token_count) {
    printf("\n✅ Generated %d tokens\n", token_count);
}
```

## 🔧 Technical Implementation Details

### Key Fixes

1. **Unified Generation Path**
   - Remove logic confusion caused by dual generation paths
   - Unify using `generate_multimodal_response_with_vocab`
   - Ensure correct vocab pointer and n_past position are always passed

2. **Position Management Optimization**
   ```rust
   // Correct position passing after encoding
   let generated_text = generate_multimodal_response_with_vocab(
       &llama_model,
       &mut ctx,
       &prompt_with_media,
       vocab,              // Direct from model
       image_data,
       image_size,
       max_tokens as i32,
       temperature as f32,
       top_k as i32,
       top_p as f32,
       repeat_penalty as f32,
       new_n_past as i32,  // Use correct position after encoding
       true,   // logits_last ← must be true
   );
   ```

3. **EOS Token Detection Fix**
   ```rust
   // ❌ Incorrect method (causes segmentation fault)
   let eos_token = llama_token_eos(model);
   if new_token_id == eos_token { ... }
   
   // ✅ Correct method
   if llama_vocab_is_eog(vocab, new_token_id) { ... }
   ```

4. **Debug Information Enhancement**
   - Context state comparison before and after encoding
   - Complete state check before generation loop
   - Logits pointer verification
   - Detailed information for each token

## 📊 Performance Metrics

| Metric | Expected Value | Description |
|--------|----------------|-------------|
| Model Loading | 10-30 seconds | Depends on device performance |
| Image Encoding | 1-3 seconds | 224x224 RGB image |
| First Token Latency | 200-500ms | First token |
| Generation Speed | 2-5 tokens/s | CPU inference speed |
| Memory Usage | Normal | Depends on model size |

## 🎯 Usage Scenarios

### 1. Real-time UI Updates
```c
void token_callback(void* user_data, const char* token, int token_id) {
    update_text_view(token);  // Update UI display
}
```

### 2. Progress Tracking
```c
void progress_callback(void* user_data, const char* token, int token_id) {
    int* count = (int*)user_data;
    (*count)++;
    
    // Update progress bar
    update_progress_bar(*count, max_tokens);
}
```

### 3. Content Filtering
```c
void filter_callback(void* user_data, const char* token, int token_id) {
    // Check for inappropriate content
    if (contains_inappropriate_content(token)) {
        stop_generation();  // Stop generation when inappropriate content detected
    }
}
```

## 🐛 Common Issues Diagnosis

### Issue A: Garbled or Empty Output
**Symptoms**: Consecutive control tokens, no actual text output

**Possible Causes**:
1. n_past position error: starting from position 0 instead of encoded position
2. Vocab corruption: Token decoding returns garbage characters
3. Invalid Logits: Context not properly encoded

**Check Methods**:
```bash
adb logcat | grep -E "Initial n_past|New n_past|Sampled token"
```

### Issue B: Logits Pointer is Null
**Symptoms**: `⚠️ WARNING: logits pointer is null!`

**Fix Method**:
```rust
// Ensure logits_last parameter is true
llama_decode(
    ctx,
    batch,
    true,   // logits_last ← must be true
);
```

## 🚀 Testing Verification

### Quick Test
```bash
cd /home/jack/codedir/GPUFabric/gpuf-c/examples
./quick_multimodal_test.sh
```

### Success Indicators
- ✅ Image encoding successful
- ✅ Position management correct
- ✅ Normal token generation
- ✅ Meaningful text output

## 📚 Related Documents
- **Testing Guide**: `MULTIMODAL_TESTING.md`
- **API Reference**: `../gpuf_c.h`
- **Build Guide**: `BUILD_GUIDE.md`
- **Android Deployment**: `ANDROID_JNI_NETWORK_BUILD_GUIDE.md`

---

**Status**: ✅ Core functionality implementation complete  
**Last Updated**: 2024-12-10
