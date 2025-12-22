use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{oneshot, Mutex};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use tracing::{info, warn,error};

use common::{Command, CommandV1};
use crate::handle::ActiveClients;
use crate::util::protoc::ClientId;

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

#[derive(Debug, Deserialize, Serialize)]
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
    // pub usage: CompletionUsage,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    // pub usage: CompletionUsage,
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

#[derive(Debug, Serialize)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
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

// Inference Scheduler
pub struct InferenceScheduler {
    pending_tasks: Arc<Mutex<HashMap<String, PendingTask>>>,
    active_clients: ActiveClients,
}

impl InferenceScheduler {
    pub fn new(active_clients: ActiveClients) -> Self {
        Self {
            pending_tasks: Arc::new(Mutex::new(HashMap::new())),
            active_clients,
        }
    }

    /// Handle inference result from device
    pub async fn handle_inference_result(&self, 
        task_id: String,
        success: bool,
        result: Option<String>,
        error: Option<String>,
        _execution_time_ms: u64,
        _prompt_tokens: u32,
        _completion_tokens: u32,
    ) {
        info!("Handling inference result for task {} (success: {})", task_id, success);
        
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
                    // usage: CompletionUsage {
                    //     prompt_tokens,
                    //     completion_tokens,
                    //     total_tokens: prompt_tokens + completion_tokens,
                    // },
                })
            } else {
                Err(anyhow!("Inference failed: {}", error.unwrap_or_default()))
            };

            if let Err(_) = sender.send(response) {
                warn!("Failed to send result for task {}", task_id);
            }
        } else {
            error!("RACE CONDITION: Task {} not found in pending_tasks!", task_id);
            error!("This means the task was removed before handle_inference_result was called");
            error!("Available tasks were: {:?}", all_tasks_before);
            
            // Log the inference result for debugging
            if success {
                info!("Lost inference result: {}", result.unwrap_or_default());
            } else {
                warn!("Lost inference error: {}", error.unwrap_or_default());
            }
        }
    }

    /// Select best Android device for inference
    async fn select_best_device(&self, allowed_client_ids: Option<&[ClientId]>) -> Result<ClientId> {
        let clients = self.active_clients.lock().await;
        
        let mut best_device: Option<(ClientId, u16)> = None;
        let mut device_count = 0;

        let mut consider_device = |client_id: &ClientId, client_info: &crate::handle::ClientInfo| {
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
            info!("Selected device {:?} for inference (load: {}%, available devices: {})", 
                  client_id, _load, device_count);
            Ok(client_id)
        } else {
            Err(anyhow!("No available Android devices found"))
        }
    }

    /// Send inference task to device
    async fn send_task_to_device(&self, device_id: &ClientId, task_id: String, prompt: String, max_tokens: u32, temperature: f32, top_k: u32, top_p: f32, repeat_penalty: f32, repeat_last_n: i32, min_keep: u32) -> Result<()> {
        use common::write_command;
        
        // Find active client connection
        let mut clients = self.active_clients.lock().await;
        let client_info = clients.get_mut(device_id)
            .ok_or_else(|| anyhow!("Device {:?} not found or not connected", device_id))?;
        
        // Check if client is authenticated and ready
        if !client_info.authed {
            error!("Device {:?} not authenticated", device_id);
            return Err(anyhow!("Device {:?} not authenticated", device_id));
        }
        
        // Try to acquire writer lock (non-blocking to avoid deadlocks)
        let mut writer = client_info.writer.try_lock()
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
        info!("sent inference task {} to device {:?} :{:?}", task_id, device_id, command);
        write_command(&mut *writer, &command).await?;
        writer.flush().await?;
        
        info!("Successfully sent inference task {} to device {:?}", task_id, device_id);
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
            info!("Stored task {} in pending_tasks (total: {})", task_id, tasks.len());
            info!("All tasks in pending_tasks: {:?}", all_tasks);
        }

        // Select best available device
        let device_id = self.select_best_device(allowed_client_ids).await?;
        
        // Send task to device
        info!("About to send task {} to device {:?}", task_id, device_id);
        if let Err(e) = self.send_task_to_device(
            &device_id,
            task_id.clone(),
            request.prompt,
            request.max_tokens.unwrap_or(100),
            request.temperature.unwrap_or(0.7),
            request.top_k.unwrap_or(40),
            request.top_p.unwrap_or(0.9),
            request.repeat_penalty.unwrap_or(1.1),
            request.repeat_last_n.unwrap_or(64),
            request.min_keep.unwrap_or(1),
        ).await {
            // Clean up pending task on failure
            let mut tasks = self.pending_tasks.lock().await;
            tasks.remove(&task_id);
            error!("Failed to send inference task to device {:?}: {}", device_id, e);
            return Err(e);
        }

        info!("Task {} sent successfully, now waiting for result...", task_id);
        
        // Check if task is still in pending_tasks before waiting
        {
            let tasks = self.pending_tasks.lock().await;
            info!("Pending tasks count before timeout wait: {}", tasks.len());
            if !tasks.contains_key(&task_id) {
                error!("Task {} missing from pending_tasks before wait!", task_id);
                return Err(anyhow!("Task {} was removed from pending_tasks unexpectedly", task_id));
            }
        }

        // Wait for result with timeout
        info!("Waiting for result of task {} with 60s timeout...", task_id);
        match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            receiver
        ).await {
            Ok(Ok(response)) => {
                info!("Task {} completed successfully", task_id);
                response
            },
            Ok(Err(_)) => {
                warn!("Task {} response channel closed", task_id);
                Err(anyhow!("Task response channel closed"))
            },
            Err(_) => {
                // Clean up pending task on timeout
                let mut tasks = self.pending_tasks.lock().await;
                tasks.remove(&task_id);
                warn!("Task {} timed out after 60 seconds", task_id);
                Err(anyhow!("Inference task timed out after 60 seconds"))
            }
        }
    }

    /// Get list of available devices
    pub async fn get_available_devices(&self, allowed_client_ids: Option<&[ClientId]>) -> Vec<DeviceInfo> {
        let clients = self.active_clients.lock().await;
        let mut devices = Vec::new();
        
        let mut maybe_push_device = |client_id: &ClientId, client_info: &crate::handle::ClientInfo| {
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
                cpu_usage: client_info.system_info.as_ref().map(|s| s.cpu_usage).unwrap_or(0),
                memory_usage: client_info.system_info.as_ref().map(|s| s.memory_usage).unwrap_or(0),
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
