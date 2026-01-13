use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::handle::ActiveClients;
use crate::util::protoc::ClientId;
use common::{Command, CommandV1, OutputPhase};

// Type aliases for easier function signatures
// Note: Can't create type alias for enum variants in Rust

// OpenAI Compatible Request/Response Types
#[derive(Debug, Deserialize)]
pub struct CompletionRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub repeat_last_n: Option<i32>,
    pub min_keep: Option<u32>,
    #[allow(dead_code)] // Part of OpenAI API spec, will be used later
    pub model: Option<String>,
    #[allow(dead_code)] // Streaming support to be implemented later
    pub stream: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub repeat_last_n: Option<i32>,
    pub min_keep: Option<u32>,
    pub stream: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct CompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<CompletionChoice>,
    pub usage: CompletionUsage,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: CompletionUsage,
}

#[derive(Debug, Serialize)]
pub struct CompletionChoice {
    pub text: String,
    pub index: i32,
    pub logprobs: Option<serde_json::Value>,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChoice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub analysis_tokens: Option<u32>,
    pub final_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

// Task result tracking
type PendingTask = oneshot::Sender<Result<CompletionResponse>>;

#[derive(Debug)]
pub enum StreamEvent {
    Delta(String, OutputPhase),
    Finish(Option<CompletionUsage>),
    Done,
    Error(String),
}

// Inference Scheduler
pub struct InferenceScheduler {
    pending_tasks: Arc<Mutex<HashMap<String, PendingTask>>>,
    partial_results: Arc<Mutex<HashMap<String, String>>>,
    pending_streams: Arc<Mutex<HashMap<String, mpsc::Sender<StreamEvent>>>>,
    stream_usages: Arc<Mutex<HashMap<String, CompletionUsage>>>,
    active_clients: ActiveClients,
}

impl InferenceScheduler {
    pub fn new(active_clients: ActiveClients) -> Self {
        Self {
            pending_tasks: Arc::new(Mutex::new(HashMap::new())),
            partial_results: Arc::new(Mutex::new(HashMap::new())),
            pending_streams: Arc::new(Mutex::new(HashMap::new())),
            stream_usages: Arc::new(Mutex::new(HashMap::new())),
            active_clients,
        }
    }

    pub async fn execute_inference_stream(
        &self,
        request: CompletionRequest,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<(String, ClientId, mpsc::Receiver<StreamEvent>)> {
        let task_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel::<StreamEvent>(128);

        {
            let mut streams = self.pending_streams.lock().await;
            streams.insert(task_id.clone(), tx);
        }

        let device_id = self.select_best_device(allowed_client_ids).await?;
        if let Err(e) = self
            .send_task_to_device(
                &device_id,
                task_id.clone(),
                request.prompt,
                request.max_tokens.unwrap_or(4090),
                request.temperature.unwrap_or(0.7),
                request.top_k.unwrap_or(40),
                request.top_p.unwrap_or(0.9),
                request.repeat_penalty.unwrap_or(1.1),
                request.repeat_last_n.unwrap_or(64),
                request.min_keep.unwrap_or(1),
            )
            .await
        {
            let mut streams = self.pending_streams.lock().await;
            streams.remove(&task_id);
            return Err(e);
        }

        Ok((task_id, device_id, rx))
    }

    async fn select_best_device_for_model(
        &self,
        model_name: &str,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<ClientId> {
        let clients = self.active_clients.lock().await;

        let mut best_device: Option<(ClientId, u16)> = None;

        for (client_id, client_info) in clients.iter() {
            if let Some(allowed) = allowed_client_ids {
                if !allowed.iter().any(|id| id == client_id) {
                    continue;
                }
            }

            if !client_info.authed {
                continue;
            }

            let Some(models) = &client_info.models else {
                continue;
            };
            if !models.iter().any(|m| m.id == model_name) {
                continue;
            }

            let Some(system_info) = &client_info.system_info else {
                continue;
            };
            let total_load: u16 = (system_info.cpu_usage + system_info.memory_usage) as u16;

            match best_device {
                None => best_device = Some((*client_id, total_load)),
                Some((_best_id, best_load)) if total_load < best_load => {
                    best_device = Some((*client_id, total_load))
                }
                _ => {}
            }
        }

        best_device
            .map(|(id, _)| id)
            .ok_or_else(|| anyhow!("No compatible client found for model '{model_name}'"))
    }

    pub async fn execute_chat_inference_stream(
        &self,
        model: String,
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<(String, ClientId, mpsc::Receiver<StreamEvent>)> {
        let task_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel::<StreamEvent>(128);

        {
            let mut streams = self.pending_streams.lock().await;
            streams.insert(task_id.clone(), tx);
        }

        let device_id = match self
            .select_best_device_for_model(&model, allowed_client_ids)
            .await
        {
            Ok(d) => d,
            Err(e) => {
                warn!(
                    "No model-compatible device found for model '{}': {}. Falling back to generic device selection.",
                    model, e
                );
                self.select_best_device(allowed_client_ids).await?
            }
        };
        debug!("Selected device {} for model {}", device_id, model);
        let common_messages = messages
            .into_iter()
            .map(|m| common::ChatMessage {
                role: m.role,
                content: m.content,
            })
            .collect::<Vec<_>>();

        if let Err(e) = self
            .send_chat_task_to_device(
                &device_id,
                task_id.clone(),
                model,
                common_messages,
                max_tokens,
                temperature,
                top_k,
                top_p,
                repeat_penalty,
                repeat_last_n,
                min_keep,
            )
            .await
        {
            let mut streams = self.pending_streams.lock().await;
            streams.remove(&task_id);
            return Err(e);
        }

        Ok((task_id, device_id, rx))
    }

    pub async fn cancel_inference(&self, task_id: &str, device_id: &ClientId) -> Result<()> {
        debug!(
            "Cancelling inference for task {} on device {}",
            task_id, device_id
        );
        {
            let mut streams = self.pending_streams.lock().await;
            streams.remove(task_id);
        }

        use common::write_command;

        let mut clients = self.active_clients.lock().await;
        let client_info = clients
            .get_mut(device_id)
            .ok_or_else(|| anyhow!("Device {:?} not found or not connected", device_id))?;

        if !client_info.authed {
            return Err(anyhow!("Device {:?} not authenticated", device_id));
        }

        let mut writer = client_info.writer.lock().await;

        let cancel = CommandV1::CancelInference {
            task_id: task_id.to_string(),
        };
        let command = Command::V1(cancel);
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn send_chat_task_to_device(
        &self,
        device_id: &ClientId,
        task_id: String,
        model: String,
        messages: Vec<common::ChatMessage>,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    ) -> Result<()> {
        use common::write_command;

        let mut clients = self.active_clients.lock().await;
        let client_info = clients
            .get_mut(device_id)
            .ok_or_else(|| anyhow!("Device {:?} not found or not connected", device_id))?;

        if !client_info.authed {
            error!("Device {:?} not authenticated", device_id);
            return Err(anyhow!("Device {:?} not authenticated", device_id));
        }

        let mut writer = client_info
            .writer
            .try_lock()
            .map_err(|_| anyhow!("Device {:?} is busy, please try again", device_id))?;

        let chat_task = CommandV1::ChatInferenceTask {
            task_id: task_id.clone(),
            model,
            messages,
            max_tokens,
            temperature,
            top_k,
            top_p,
            repeat_penalty,
            repeat_last_n,
            min_keep,
        };

        let command = Command::V1(chat_task);
        info!(
            "sent chat inference task {} to device {:?} :{:?}",
            task_id, device_id, command
        );
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        Ok(())
    }

    pub async fn handle_inference_result_chunk(
        &self,
        task_id: String,
        _seq: u32,
        delta: String,
        phase: OutputPhase,
        done: bool,
        error: Option<String>,
        prompt_tokens: u32,
        completion_tokens: u32,
        analysis_tokens: u32,
        final_tokens: u32,
    ) {
        let stream_sender = {
            let streams = self.pending_streams.lock().await;
            streams.get(&task_id).cloned()
        };

        if let Some(sender) = stream_sender {
            if let Some(err) = error {
                let _ = sender.send(StreamEvent::Error(err)).await;
                let _ = sender.send(StreamEvent::Done).await;
                let mut streams = self.pending_streams.lock().await;
                streams.remove(&task_id);
                let mut usages = self.stream_usages.lock().await;
                usages.remove(&task_id);
                return;
            }

            if !delta.is_empty() {
                let _ = sender.send(StreamEvent::Delta(delta, phase)).await;
            }

            if done {
                let usage = CompletionUsage {
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens.saturating_add(completion_tokens),
                    analysis_tokens: Some(analysis_tokens),
                    final_tokens: Some(final_tokens),
                };
                {
                    let mut usages = self.stream_usages.lock().await;
                    usages.insert(task_id.clone(), usage.clone());
                }

                let usage_for_finish = {
                    let usages = self.stream_usages.lock().await;
                    usages.get(&task_id).cloned()
                };

                let _ = sender.send(StreamEvent::Finish(usage_for_finish)).await;
                let _ = sender.send(StreamEvent::Done).await;
                let mut streams = self.pending_streams.lock().await;
                streams.remove(&task_id);
                let mut usages = self.stream_usages.lock().await;
                usages.remove(&task_id);
            }
            return;
        }

        if let Some(err) = error {
            self.handle_inference_result(task_id, false, None, Some(err), 0, 0, 0)
                .await;
            return;
        }

        {
            let mut partial = self.partial_results.lock().await;
            let entry = partial.entry(task_id.clone()).or_insert_with(String::new);
            entry.push_str(&delta);
        }

        if done {
            let result = {
                let mut partial = self.partial_results.lock().await;
                partial.remove(&task_id).unwrap_or_default()
            };
            self.handle_inference_result(
                task_id,
                true,
                Some(result),
                None,
                0,
                prompt_tokens,
                completion_tokens,
            )
            .await;
        }
    }

    /// Handle inference result from device
    pub async fn handle_inference_result(
        &self,
        task_id: String,
        success: bool,
        result: Option<String>,
        error: Option<String>,
        _execution_time_ms: u64,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) {
        info!(
            "Handling inference result for task {} (success: {})",
            task_id, success
        );

        let mut tasks = self.pending_tasks.lock().await;
        let all_tasks_before: Vec<String> = tasks.keys().cloned().collect();
        info!("Current pending tasks count: {}", tasks.len());
        info!("All tasks before removal: {:?}", all_tasks_before);

        // Find the sender for this taskretain
        let sender = tasks.remove(&task_id);
        info!("pop sender : {:?}", sender);
        if let Some(sender) = sender {
            info!("Found and removed task {} from pending_tasks", task_id);
            let remaining_tasks: Vec<String> = tasks.keys().cloned().collect();
            info!("Remaining tasks after removal: {:?}", remaining_tasks);
            let response = if success {
                Ok(CompletionResponse {
                    id: task_id.clone(),
                    object: "text_completion".to_string(),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    model: "gpuf-android".to_string(),
                    choices: vec![CompletionChoice {
                        text: result.unwrap_or_default(),
                        index: 0,
                        logprobs: None,
                        finish_reason: "stop".to_string(),
                    }],
                    usage: CompletionUsage {
                        prompt_tokens,
                        completion_tokens,
                        total_tokens: prompt_tokens.saturating_add(completion_tokens),
                        analysis_tokens: None,
                        final_tokens: None,
                    },
                })
            } else {
                Err(anyhow!("Inference failed: {}", error.unwrap_or_default()))
            };

            if let Err(_) = sender.send(response) {
                warn!("Failed to send result for task {}", task_id);
            }
        } else {
            {
                let mut partial = self.partial_results.lock().await;
                partial.remove(&task_id);
            }

            // This commonly happens when the SSE client disconnects and we cancel/remove the
            // stream sender before the device finishes sending its final chunks.
            debug!(
                "Dropping inference result for task {} because it is no longer pending (likely canceled/disconnected). Available tasks were: {:?}",
                task_id,
                all_tasks_before
            );
        }
    }

    /// Select best Android device for inference
    async fn select_best_device(
        &self,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<ClientId> {
        let clients = self.active_clients.lock().await;

        let mut best_device: Option<(ClientId, u16)> = None;
        let mut device_count = 0;

        let mut consider_device =
            |client_id: &ClientId, client_info: &crate::handle::ClientInfo| {
                // Only consider authenticated Android devices
                if !client_info.authed {
                    return;
                }

                // Check if device has system info (Android devices should have this)
                let Some(system_info) = &client_info.system_info else {
                    return;
                };

                // Simple load balancing: choose device with lowest CPU + Memory usage
                let total_load: u16 = (system_info.cpu_usage + system_info.memory_usage) as u16;
                device_count += 1;

                if best_device.is_none() || total_load < best_device.as_ref().unwrap().1 {
                    best_device = Some((*client_id, total_load));
                }
            };

        match allowed_client_ids {
            Some(allowed) => {
                // Base set = allowed ids; lookup active client info from map (O(1) average)
                for client_id in allowed {
                    if let Some(client_info) = clients.get(client_id) {
                        consider_device(client_id, client_info);
                    }
                }
            }
            None => {
                // No restriction; base set = all active clients
                for (client_id, client_info) in clients.iter() {
                    consider_device(client_id, client_info);
                }
            }
        }

        if let Some((client_id, _load)) = best_device {
            info!(
                "Selected device {:?} for inference (load: {}%, available devices: {})",
                client_id, _load, device_count
            );
            Ok(client_id)
        } else {
            Err(anyhow!("No available Android devices found"))
        }
    }

    /// Send inference task to device
    async fn send_task_to_device(
        &self,
        device_id: &ClientId,
        task_id: String,
        prompt: String,
        max_tokens: u32,
        temperature: f32,
        top_k: u32,
        top_p: f32,
        repeat_penalty: f32,
        repeat_last_n: i32,
        min_keep: u32,
    ) -> Result<()> {
        use common::write_command;

        // Find active client connection
        let mut clients = self.active_clients.lock().await;
        let client_info = clients
            .get_mut(device_id)
            .ok_or_else(|| anyhow!("Device {:?} not found or not connected", device_id))?;

        // Check if client is authenticated and ready
        if !client_info.authed {
            error!("Device {:?} not authenticated", device_id);
            return Err(anyhow!("Device {:?} not authenticated", device_id));
        }

        // Try to acquire writer lock (non-blocking to avoid deadlocks)
        let mut writer = client_info
            .writer
            .try_lock()
            .map_err(|_| anyhow!("Device {:?} is busy, please try again", device_id))?;

        // Create and send inference task command
        let inference_task = CommandV1::InferenceTask {
            task_id: task_id.clone(),
            prompt,
            max_tokens,
            temperature,
            top_k,
            top_p,
            repeat_penalty,
            repeat_last_n,
            min_keep,
        };

        let command = Command::V1(inference_task);
        info!(
            "sent inference task {} to device {:?} :{:?}",
            task_id, device_id, command
        );
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;

        info!(
            "Successfully sent inference task {} to device {:?}",
            task_id, device_id
        );
        Ok(())
    }

    /// Execute inference task
    pub async fn execute_inference(
        &self,
        request: CompletionRequest,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Result<CompletionResponse> {
        let task_id = Uuid::new_v4().to_string();

        // Create response channel
        let (sender, receiver) = oneshot::channel();
        {
            let mut tasks = self.pending_tasks.lock().await;
            let existing_tasks: Vec<String> = tasks.keys().cloned().collect();
            info!("Existing tasks before insert: {:?}", existing_tasks);
            tasks.insert(task_id.clone(), sender);
            let all_tasks: Vec<String> = tasks.keys().cloned().collect();
            info!(
                "Stored task {} in pending_tasks (total: {})",
                task_id,
                tasks.len()
            );
            info!("All tasks in pending_tasks: {:?}", all_tasks);
        }

        // Select best available device
        let device_id = self.select_best_device(allowed_client_ids).await?;

        // Send task to device
        info!("About to send task {} to device {:?}", task_id, device_id);
        if let Err(e) = self
            .send_task_to_device(
                &device_id,
                task_id.clone(),
                request.prompt,
                request.max_tokens.unwrap_or(1024),
                request.temperature.unwrap_or(0.7),
                request.top_k.unwrap_or(40),
                request.top_p.unwrap_or(0.9),
                request.repeat_penalty.unwrap_or(1.1),
                request.repeat_last_n.unwrap_or(64),
                request.min_keep.unwrap_or(1),
            )
            .await
        {
            // Clean up pending task on failure
            let mut tasks = self.pending_tasks.lock().await;
            tasks.remove(&task_id);
            error!(
                "Failed to send inference task to device {:?}: {}",
                device_id, e
            );
            return Err(e);
        }

        info!(
            "Task {} sent successfully, now waiting for result...",
            task_id
        );

        // Check if task is still in pending_tasks before waiting
        {
            let tasks = self.pending_tasks.lock().await;
            info!("Pending tasks count before timeout wait: {}", tasks.len());
            if !tasks.contains_key(&task_id) {
                error!("Task {} missing from pending_tasks before wait!", task_id);
                return Err(anyhow!(
                    "Task {} was removed from pending_tasks unexpectedly",
                    task_id
                ));
            }
        }

        // Wait for result with timeout
        let timeout_secs: u64 = std::env::var("GPUF_INFERENCE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(300);

        info!(
            "Waiting for result of task {} with {}s timeout...",
            task_id, timeout_secs
        );
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), receiver).await {
            Ok(Ok(response)) => {
                info!("Task {} completed successfully", task_id);
                response
            }
            Ok(Err(_)) => {
                warn!("Task {} response channel closed", task_id);
                Err(anyhow!("Task response channel closed"))
            }
            Err(_) => {
                // Clean up pending task on timeout
                let mut tasks = self.pending_tasks.lock().await;
                tasks.remove(&task_id);
                warn!("Task {} timed out after {} seconds", task_id, timeout_secs);
                Err(anyhow!(
                    "Inference task timed out after {} seconds",
                    timeout_secs
                ))
            }
        }
    }

    /// Get list of available devices
    pub async fn get_available_devices(
        &self,
        allowed_client_ids: Option<&[ClientId]>,
    ) -> Vec<DeviceInfo> {
        let clients = self.active_clients.lock().await;
        let mut devices = Vec::new();

        let mut maybe_push_device =
            |client_id: &ClientId, client_info: &crate::handle::ClientInfo| {
                if !client_info.authed {
                    return;
                }
                let device = DeviceInfo {
                    client_id: hex::encode(&client_id.0),
                    status: if client_info.system_info.is_some() {
                        "online".to_string()
                    } else {
                        "initializing".to_string()
                    },
                    cpu_usage: client_info
                        .system_info
                        .as_ref()
                        .map(|s| s.cpu_usage)
                        .unwrap_or(0),
                    memory_usage: client_info
                        .system_info
                        .as_ref()
                        .map(|s| s.memory_usage)
                        .unwrap_or(0),
                    device_count: client_info.devices_info.len() as u32,
                };
                devices.push(device);
            };

        match allowed_client_ids {
            Some(allowed) => {
                for client_id in allowed {
                    if let Some(client_info) = clients.get(client_id) {
                        maybe_push_device(client_id, client_info);
                    }
                }
            }
            None => {
                for (client_id, client_info) in clients.iter() {
                    maybe_push_device(client_id, client_info);
                }
            }
        }

        devices
    }
}

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub client_id: String,
    pub status: String,
    pub cpu_usage: u8,
    pub memory_usage: u8,
    pub device_count: u32,
}
