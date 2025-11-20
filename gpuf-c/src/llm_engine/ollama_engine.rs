use super::{Engine, OllamaEngine, OLLAMA_CONTAINER_NAME, OLLAMA_DEFAULT_PORT};
#[cfg(not(target_os = "macos"))]
use crate::util::system_info::get_gpu_count;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;
use tracing::{debug, error, info};

impl OllamaEngine {
    pub fn new() -> Self {
        OllamaEngine {
            models: [0; 16],
            models_name: Vec::new(),
            client: Client::new(),
            base_url: format!("http://localhost:{}", OLLAMA_DEFAULT_PORT),
            container_id: None,
            #[cfg(not(target_os = "macos"))]
            gpu_count: get_gpu_count().unwrap_or(0) as u32,
            #[cfg(target_os = "macos")]
            gpu_count: 1,
        }
    }

    async fn is_container_running(&self) -> bool {
        if let Some(container_id) = &self.container_id {
            let output = Command::new("docker")
                .args(&["inspect", "-f", "{{.State.Running}}", container_id])
                .output()
                .await
                .ok();

            if let Some(output) = output {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    return stdout.trim() == "true";
                }
            }
        }
        false
    }

    async fn start_container(&mut self) -> Result<()> {
        if self.is_container_running().await {
            info!("Ollama container is already running");
            return Ok(());
        }

        info!("Starting Ollama container...");

        let image_check = Command::new("docker")
            .args(&["inspect", "--type=image", "ollama/ollama:latest"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        if image_check.is_err() || !image_check.unwrap().success() {
            info!("Pulling Ollama image...");
            let pull_output = Command::new("docker")
                .args(&["pull", "ollama/ollama:latest"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
                .wait_with_output()
                .await?;

            if !pull_output.status.success() {
                let stderr = String::from_utf8_lossy(&pull_output.stderr);
                return Err(anyhow!("Failed to pull Ollama image: {}", stderr));
            }
        }

        let mut command = Command::new("docker");

        let name_flag = format!("--name={}", OLLAMA_CONTAINER_NAME);
        let port_flag = format!("-p{}:11434", OLLAMA_DEFAULT_PORT);

        
        let model_dir = if cfg!(target_os = "windows") {
            // Windows  %USERPROFILE%\.ollama\models
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
            format!("{}\\.ollama\\models", home.replace('\\', "\\\\"))
        } else {
            // Unix-like  ~/.ollama/models
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.ollama/models", home)
        };
        if let Err(e) = std::fs::create_dir_all(&model_dir) {
            error!("Failed to create model directory {}: {}", model_dir, e);
        }
        let volume_flag = format!("-v {}:/root/.ollama", model_dir);

        // TODO: auto set gpu number
        let mut args = vec![
            "run",
            "-d",
            "--rm",
            &name_flag,
            &port_flag,
            &volume_flag,
            "-e",
            "OLLAMA_HOST=0.0.0.0",
            "-e",
            "OLLAMA_GPU_LAYERS=all",
        ];

        if cfg!(target_os = "macos") {
            if cfg!(target_arch = "x86_64") {
                args.extend_from_slice(&["--platform", "linux/amd64"]);
            } 
        } else if cfg!(target_os = "linux") {
            args.extend_from_slice(&[
                "--platform",
                "linux/amd64",
                "--gpus",
                "all",
                "--device",
                "/dev/kfd",
                "--device",
                "/dev/dri",
                "--group-add",
                "video",
            ]);
        } else {
            args.extend_from_slice(&["--platform", "linux/amd64"]);
        }
        // TODO: auto set shm-size
        args.extend_from_slice(&["--shm-size", "2g", "ollama/ollama:latest"]);

        let output = command
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .wait_with_output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to start Ollama container: {}", stderr));
        }

        self.container_id = Some(OLLAMA_CONTAINER_NAME.to_string());

        let wait_time = if cfg!(target_os = "macos") {
            info!("Increasing container startup wait time on macOS...");
            10
        } else {
            2
        };
        sleep(Duration::from_secs(wait_time)).await;

        self.wait_for_ollama_ready().await?;
        Ok(())
    }

    #[allow(dead_code)]
    async fn stop_container(&self) -> Result<()> {
        if let Some(container_id) = &self.container_id {
            info!("Stopping Ollama container...");
            let _ = Command::new("docker")
                .args(&["stop", container_id])
                .status()
                .await?;
        }
        Ok(())
    }

    async fn wait_for_ollama_ready(&self) -> Result<()> {
        const MAX_RETRIES: u8 = 10;
        const RETRY_DELAY: u64 = 2; // seconds

        for attempt in 1..=MAX_RETRIES {
            match self
                .client
                .get(&format!("{}/api/tags", self.base_url))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    info!("Ollama server is ready");
                    return Ok(());
                }
                _ => {
                    if attempt < MAX_RETRIES {
                        debug!(
                            "Waiting for Ollama server to be ready (attempt {}/{})",
                            attempt, MAX_RETRIES
                        );
                        sleep(Duration::from_secs(RETRY_DELAY)).await;
                    }
                }
            }
        }

        Err(anyhow!(
            "Failed to connect to Ollama server after {} attempts",
            MAX_RETRIES
        ))
    }

    #[allow(dead_code)]
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(&format!("{}/api/tags", self.base_url))
            .send()
            .await?
            .json::<Value>()
            .await?;

        let models = response["models"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|m| m["name"].as_str().map(String::from))
            .collect();

        Ok(models)
    }

    #[allow(dead_code)]
    pub async fn pull_model(&self, model: &str) -> Result<()> {
        info!("Pulling model: {}", model);
        let response = self
            .client
            .post(&format!("{}/api/pull", self.base_url))
            .json(&serde_json::json!({
                "name": model,
                "stream": false
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to pull model {}: {}",
                model,
                response.text().await?
            ));
        }

        info!("Successfully pulled model: {}", model);
        Ok(())
    }
}

impl Engine for OllamaEngine {

    fn init(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Initializing Ollama engine...");
            self.start_container().await?;
            Ok(())
        }
    }

    fn set_models(&mut self, models: Vec<String>) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            if models.is_empty() {
                return Err(anyhow!("Model list cannot be empty"));
            }

            info!("Setting Ollama models: {:?}", models);
            self.models_name = models;

            Ok(())
        }
    }

    fn start_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            if self.models.is_empty() {
                return Err(anyhow!("No models loaded, cannot start worker"));
            }

            if !self.is_container_running().await {
                return Err(anyhow!("Ollama container is not running"));
            }

            for model in &self.models_name {
                if let Err(e) = self.pull_model(model).await {
                    error!("Failed to load model {}: {}", model, e);
                    return Err(e);
                }
            }

            info!("Ollama worker started successfully");
            Ok(())
        }
    }

    fn stop_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Stopping Ollama worker...");
            self.stop_container().await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::init_logging;
    #[tokio::test]
    async fn test_pull_model_success() {
        init_logging();
        
        let mut engine = OllamaEngine::new();
        match engine.start_container().await {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to start Ollama container: {}", e);
                return;
            }
        }
        let result = engine.pull_model("llama2").await;
        assert!(result.is_ok());
        engine
            .stop_container()
            .await
            .expect("start_container error");
    }
}
