#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef int LlamaToken;

// C interface function declarations
extern int gpuf_init(void);
extern void* gpuf_load_model(const char* path);
extern void* gpuf_create_context(void* model);
extern void gpuf_cleanup(void);

extern int gpuf_generate_with_sampling(
    const void* model,
    void* ctx, 
    const char* prompt,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    char* output,
    int output_len,
    LlamaToken* token_buffer,
    int token_buffer_size
);

int main(int argc, char* argv[]) {
    printf("🧪 Android Inference Test - OPTIMIZED PARAMETERS\n");
    printf("===============================================\n\n");
    
    if (argc != 2) {
        printf("Usage: %s \"prompt\"\n", argv[0]);
        printf("Example: %s \"Hello\"\n", argv[0]);
        printf("Example: %s \"What is your name?\"\n", argv[0]);
        return 1;
    }
    
    const char* prompt = argv[1];
    printf("📝 Testprompt: \"%s\"\n\n", prompt);
    
    // Initialize system
    printf("🔧 Initializing GPUFabric SDK...\n");
    if (!gpuf_init()) {
        printf("❌ System initialization failed\n");
        return 1;
    }
    printf("✅ System initialization successful\n\n");
    
    // LoadModel
    printf("📦 Loading SmolVLM-500M model...\n");
    const char* model_path = "/data/local/tmp/SmolVLM-500M-Instruct-Q8_0.gguf";
    void* model = gpuf_load_model(model_path);
    if (!model) {
        printf("❌ Model loading failed: %s\n", model_path);
        gpuf_cleanup();
        return 1;
    }
    printf("✅ Model loaded successfully\n\n");
    
    // createbuildupdowntext
    printf("🎯 Creating inference context...\n");
    void* ctx = gpuf_create_context(model);
    if (!ctx) {
        printf("❌ Context creation failed\n");
        gpuf_cleanup();
        return 1;
    }
    printf("✅ Context created successfully\n\n");
    
    // Generatetextscript - useuseexcellent-izeParameters
    printf("🚀 Starting AI inference...\n");
    printf("⚙️  excellent-izeParameters: Temperature=0.8, Top-K=40, Top-P=0.9, Repeat=1.1\n\n");
    
    char output[1024] = {0};
    LlamaToken token_buffer[32];
    
    int result = gpuf_generate_with_sampling(
        model, ctx, prompt,
        40,      // increaseaddto 40 tokens
        0.8f,    // Set temperature to 0.8
        40,      // increaseadd Top-K to 40
        0.9f,    // Set Top-P to 0.9
        1.1f,    // Add repeat penalty of 1.1
        output, sizeof(output) - 1,
        token_buffer, 32
    );
    
    printf("📊 Inference Results:\n");
    printf("=============\n");
    
    if (result > 0) {
        printf("✅ Generation successful!\n");
        printf("📝 Output: \"%s\"\n", output);
        printf("📊 Length: %d tokens\n\n", result);
        
        // Analyze output quality
        printf("🔍 Output quality analysis:\n");
        if (strlen(output) > 10) {
            printf("✅ Generation completed with meaningful content\n");
        } else {
            printf("⚠️  Content too short\n");
        }
        
        if (strstr(output, " ") && strstr(output, ".")) {
            printf("✅ Output contains complete sentence structure\n");
        } else {
            printf("⚠️  Sentence structure incomplete\n");
        }
        
        if (strstr(output, prompt)) {
            printf("⚠️  Output contains complex prompt\n");
        } else {
            printf("✅ No complex prompt detected\n");
        }
    } else {
        printf("❌ Generation Failed: Error code %d\n", result);
    }
    
    // Cleanup resources
    printf("\n🧹 Cleaning up resources...\n");
    gpuf_cleanup();
    
    printf("\n🎉 Android AI inference test completed!\n");
    printf("=====================================\n");
    return 0;
}
