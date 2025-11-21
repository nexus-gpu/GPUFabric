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
 * Get version information
 */
const char *gpuf_version(void);

/**
 * Initialize LLM engine with model
 * model_path: Model file path (null-terminated string)
 * n_ctx: Context size for the model
 * n_gpu_layers: Number of GPU layers (0 = CPU only)
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_llm_init(const char *model_path, uint32_t n_ctx, uint32_t n_gpu_layers);

/**
 * Generate text using the initialized LLM engine
 * prompt: Input prompt (null-terminated string)
 * max_tokens: Maximum number of tokens to generate
 * Returns: Generated text pointer, needs to call gpuf_free_string to release
 */
char *gpuf_llm_generate(const char *prompt, uintptr_t max_tokens);

/**
 * Check if LLM engine is initialized
 * Returns: 1 if initialized, 0 if not
 */
int32_t gpuf_llm_is_initialized(void);

/**
 * Unload LLM engine and free resources
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_llm_unload(void);

/**
 * Initialize GPUFabric client with configuration
 * config_json: JSON string with client configuration
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_client_init(const char *config_json);

/**
 * Connect and register the client to the server
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_client_connect(void);

/**
 * Get client status as JSON string
 * Returns: Status JSON string pointer, needs to call gpuf_free_string to release
 */
char *gpuf_client_get_status(void);

/**
 * Get device information as JSON string
 * Returns: Device info JSON string pointer, needs to call gpuf_free_string to release
 */
char *gpuf_client_get_device_info(void);

/**
 * Get client metrics as JSON string
 * Returns: Metrics JSON string pointer, needs to call gpuf_free_string to release
 */
char *gpuf_client_get_metrics(void);

/**
 * Update device information
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_client_update_device_info(void);

/**
 * Disconnect client from server
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_client_disconnect(void);

/**
 * Cleanup client resources
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_client_cleanup(void);

#endif /* GPUF_C_H */
