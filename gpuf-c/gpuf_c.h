// GPUFabric C Header - Linker Script Approach
#ifndef GPUFABRIC_C_H
#define GPUFABRIC_C_H

#ifdef __cplusplus
extern "C" {
#endif

// Core functions
int gpuf_init(void);
int gpuf_cleanup(void);
const char* gpuf_version(void);
const char* gpuf_get_last_error(void);

// LLM functions
int gpuf_llm_load_model(const char* model_path);
const char* gpuf_llm_generate(const char* prompt, int max_tokens);
int gpuf_llm_unload(void);

// Advanced functions
const char* gpuf_llm_generate_with_params(
    const char* prompt, 
    int max_tokens,
    float temperature,
    float top_p,
    int top_k
);

#ifdef __cplusplus
}
#endif

#endif // GPUFABRIC_C_H
