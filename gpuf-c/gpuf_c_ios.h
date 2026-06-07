#ifndef GPUF_C_H
#define GPUF_C_H

#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*gpuf_status_callback)(const char *message, void *user_data);

struct llama_model;
struct llama_context;
struct gpuf_multimodal_model;

int gpuf_init(void);
int gpuf_cleanup(void);
const char *gpuf_version(void);
const char *gpuf_system_info(void);

struct llama_model *gpuf_load_model(const char *model_path);
struct llama_context *gpuf_create_context(struct llama_model *model);

int gpuf_generate_final_solution_text(
    const struct llama_model *model,
    struct llama_context *context,
    const char *prompt,
    int max_tokens,
    char *output_buffer,
    int output_buffer_size
);

int gpuf_generate_with_sampling(
    const struct llama_model *model,
    struct llama_context *context,
    const char *prompt,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    char *output_buffer,
    int output_buffer_size,
    int32_t *token_buffer,
    int token_buffer_size
);

void llama_model_free(struct llama_model *model);
void llama_free(struct llama_context *context);

int set_remote_worker_model(const char *model_path);

int start_remote_worker(
    const char *server_addr,
    int control_port,
    int proxy_port,
    const char *worker_type,
    const char *client_id
);

int start_remote_worker_with_tls(
    const char *server_addr,
    int control_port,
    int proxy_port,
    const char *worker_type,
    const char *client_id,
    const char *ca_cert_path,
    const char *control_tls_server_name,
    const char *cert_sha256_pin
);

int gpuf_validate_mobile_tls_policy(
    const char *ca_cert_path,
    const char *control_tls_server_name,
    const char *cert_sha256_pin
);

int start_remote_worker_tasks(void);
int start_remote_worker_tasks_with_callback_ptr(gpuf_status_callback callback);
int gpuf_register_remote_worker_callback(gpuf_status_callback callback, void *user_data);
int get_remote_worker_status(char *buffer, size_t buffer_size);
int stop_remote_worker(void);

struct gpuf_multimodal_model *gpuf_load_multimodal_model(
    const char *text_model_path,
    const char *mmproj_path
);

struct llama_context *gpuf_create_multimodal_context(
    struct gpuf_multimodal_model *multimodal_model
);

int gpuf_generate_multimodal(
    struct gpuf_multimodal_model *multimodal_model,
    struct llama_context *context,
    const char *text_prompt,
    const uint8_t *image_data,
    uint64_t image_size,
    int max_tokens,
    float temperature,
    int top_k,
    float top_p,
    float repeat_penalty,
    char *output_buffer,
    int output_buffer_size
);

void gpuf_free_multimodal_model(struct gpuf_multimodal_model *multimodal_model);
bool gpuf_multimodal_supports_vision(struct gpuf_multimodal_model *multimodal_model);
int gpuf_get_multimodal_info(struct gpuf_multimodal_model *multimodal_model, bool *has_vision);
int gpuf_get_vision_tokens(
    struct gpuf_multimodal_model *multimodal_model,
    char *start_token,
    char *end_token,
    char *media_token,
    int max_length
);

#ifdef __cplusplus
}
#endif

#endif /* GPUF_C_H */
