#ifndef GPUF_C_H
#define GPUF_C_H

#pragma once

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * 初始化 GPUFabric 库
 * 返回: 0 成功, -1 失败
 */
int32_t gpuf_init(void);

/**
 * 获取最后一次错误信息
 * 返回: 错误信息字符串指针，调用者需要调用 gpuf_free_string 释放
 */
char *gpuf_get_last_error(void);

/**
 * 释放由库分配的字符串
 */
void gpuf_free_string(char *s);

/**
 * 创建 Worker 配置
 * 返回: 配置句柄，失败返回 null
 */
void *gpuf_create_config(const char *server_addr,
                         uint16_t control_port,
                         const char *local_addr,
                         uint16_t local_port);

/**
 * 释放配置
 */
void gpuf_free_config(void *config);

/**
 * 获取版本信息
 */
const char *gpuf_version(void);

/**
 * 初始化 LLM 引擎
 * model_path: 模型文件路径
 * n_ctx: 上下文大小
 * n_gpu_layers: GPU 层数（0 表示 CPU only）
 * 返回: 0 成功, -1 失败
 */
int32_t gpuf_llm_init(const char *model_path, uint32_t n_ctx, uint32_t n_gpu_layers);

/**
 * 生成文本
 * prompt: 输入提示词
 * max_tokens: 最大生成 token 数
 * 返回: 生成的文本指针，需要调用 gpuf_free_string 释放
 */
char *gpuf_llm_generate(const char *prompt, uintptr_t max_tokens);

#endif /* GPUF_C_H */
