#ifndef GPUF_C_H
#define GPUF_C_H

#pragma once

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Initialize GPUFabric library
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_init(void);

/**
 * Get last error information
 * Returns: Error message string pointer, caller needs to call gpuf_free_string to release
 */
char *gpuf_get_last_error(void);

/**
 * Release string allocated by the library
 */
void gpuf_free_string(char *s);

/**
 * Create Worker configuration
 * Returns: Configuration handle, returns null on failure
 */
void *gpuf_create_config(const char *server_addr,
                         uint16_t _control_port,
                         const char *_local_addr,
                         uint16_t _local_port);

/**
 * Release configuration
 */
void gpuf_free_config(void *config);

/**
 * Get version information
 */
const char *gpuf_version(void);

/**
 * Initialize LLM engine
 * model_path: Model file path
 * n_ctx: Context size
 * n_gpu_layers: Number of GPU layers (0 means CPU only)
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_llm_init(const char *model_path, uint32_t n_ctx, uint32_t n_gpu_layers);

/**
 * Generate text
 * prompt: Input prompt
 * max_tokens: Maximum number of tokens to generate
 * Returns: Generated text pointer, needs to call gpuf_free_string to release
 */
char *gpuf_llm_generate(const char *prompt, uintptr_t max_tokens);

#endif /* GPUF_C_H */
