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
    printf("🧪 Android Inference Test\n");
    printf("========================\n\n");
    
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
    
    // Generatetextscript
    printf("🚀 Starting AI inference...\n");
    printf("⚙️  Parameters: Temperature=0.3, Top-K=10, Top-P=0.8\n\n");
    
    char output[1024] = {0};
    LlamaToken token_buffer[32];
    
    int result = gpuf_generate_with_sampling(
        model, ctx, prompt,
        30, 0.3f, 10, 0.8f, 1.0f,
        output, sizeof(output) - 1,
        token_buffer, 32
    );
    
    printf("📊 Inference Results:\n");
    printf("=============\n");
    
    if (result > 0) {
        printf("✅ Generation successful!\n");
        printf("📝 Output: \"%s\"\n", output);
        printf("📊 Length: %d tokens\n\n", result);
        
        // Analyze output type
        printf("🔍 Output Analysis:\n");
        if (strstr(output, "Explanation") || strstr(output, "function")) {
            printf("⚠️  Check technical bias - SmolVLM training difference\n");
        } else if (strstr(output, "Hello") || strstr(output, "Hi")) {
            printf("✅ Greeting response\n");
        } else if (strstr(output, "?") || strstr(output, "answer")) {
            printf("✅ Question and answer format\n");
        } else if (strstr(output, "=") || strstr(output, "4") || strstr(output, "calculate")) {
            printf("✅ Mathematical calculation answer\n");
        } else {
            printf("🤔 Other type of answer\n");
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
