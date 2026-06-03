// ============================================================================
// GPUFabric Minimal C API Header
// ============================================================================
//
// This header contains only the C API functions for Remote Worker Management
// without JNI dependencies. Use for pure C/C++ applications.
//
#ifndef GPUF_C_MINIMAL_H
#define GPUF_C_MINIMAL_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>

// Basic type definitions
typedef int32_t jint;
typedef int64_t jlong;

// Remote Worker Management C API Functions

/**
 * Set remote worker model with safe hot swapping support
 * @param model_path Path to the model file (.gguf)
 * @return 0 on success, negative error code on failure
 */
int set_remote_worker_model(const char* model_path);

/**
 * Start remote worker connection
 * @param server_addr Server address
 * @param control_port Control port number
 * @param proxy_port Proxy port number  
 * @param worker_type Worker type (e.g., "TCP")
 * @param client_id Client ID (32 hex characters)
 * @return 0 on success, negative error code on failure
 */
int start_remote_worker(const char* server_addr, jint control_port, jint proxy_port, 
                       const char* worker_type, const char* client_id);

/**
 * Start remote worker background tasks
 * @return 0 on success, negative error code on failure
 */
int start_remote_worker_tasks(void);

/**
 * Start remote worker background tasks with callback support
 * @param callback_ptr Function pointer for status callbacks (cast to jlong)
 * @return 0 on success, negative error code on failure
 * 
 * Callback signature: void callback(const char* message, void* user_data)
 * The callback will be invoked with status updates like:
 * - "STARTING - Initializing background tasks..."
 * - "HEARTBEAT - Sending heartbeat to server"
 * - "HANDLER_START - Handler thread started"
 * - "LOGIN_SUCCESS - Login successful"
 * - "COMMAND_RECEIVED - V1(InferenceTask {...})"
 * - "INFERENCE_START - Task: xxx-xxx-xxx"
 * - "INFERENCE_SUCCESS - Task: xxx-xxx-xxx in XXXms"
 * - "INFERENCE_FAILED - Task: xxx-xxx-xxx - error message"
 */
int start_remote_worker_tasks_with_callback_ptr(jlong callback_ptr);

/**
 * Stop remote worker
 * @return 0 on success, negative error code on failure
 */
int stop_remote_worker(void);

/**
 * Get remote worker status
 * @param status_buffer Buffer to store status string
 * @param buffer_size Size of the status buffer
 * @return 0 on success, negative error code on failure
 */
int get_remote_worker_status(char* status_buffer, jint buffer_size);

#ifdef __cplusplus
}
#endif

#endif // GPUF_C_MINIMAL_H
