use crate::util::system_info;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio::time::Duration;
use tracing::{error, info, warn, debug};

use super::{Engine, VLLMEngine, VLLM_CONTAINER_NAME, VLLM_DEFAULT_PORT, DEFAULT_CHAT_TEMPLATE,VLLM_CONTAINER_PATH};

macro_rules! setup_tensor_parallel {
    ($args:expr, $gpu_count:expr) => {{
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            //TODO: vllm not support mps
            // $args.extend(["--device", "mps"]);
            system_info::get_apple_gpu_cores().unwrap_or(1).to_string()
        }
        #[cfg(not(target_os = "macos"))]
        {
            $args.extend([
                "--gpus",
                "all",
                "-e",
                "VLLM_DEVICE_TYPE=cuda",
                "-e",
                "TORCH_CUDA_ARCH_LIST=7.0 7.5 8.0 8.6 8.9 9.0+PTX",
            ]);
            $gpu_count.to_string()
        }
    }};
}

impl VLLMEngine {
    pub fn new(hugging_face_hub_token: Option<String>, chat_template_path: Option<String>) -> Self {
        VLLMEngine {
            models: Arc::new(RwLock::new(HashMap::new())),
            models_name: Vec::new(),
            worker_handler: None,
            show_worker_log: false,
            base_url: format!("http://localhost:{}", VLLM_DEFAULT_PORT),
            container_id: None,
            #[cfg(not(target_os = "macos"))]
            gpu_count: system_info::get_gpu_count().unwrap_or(0) as u32,
            #[cfg(target_os = "macos")]
            gpu_count: 1,
            hugging_face_hub_token,
            chat_template_path,
        }
    }

    async fn is_container_running(&self) -> bool {
        if let Some(container_id) = &self.container_id {
            let output = Command::new("docker")
                .args(["inspect", "-f", "{{.State.Running}}", container_id])
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
            info!("VLLM container is already running");
            return Ok(());
        }

        let _ = Command::new("docker")
            .args(["rm", "-f", VLLM_CONTAINER_NAME])
            .status()
            .await
            .map_err(|e| {
                warn!("Failed to remove existing container: {}", e);
                e
            })?;

        // Check if VLLM image exists
        let image_check = Command::new("docker")
            .args(["inspect", "--type=image", "vllm/vllm-openai:latest"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        info!("image_check: {:?}", image_check);
        if image_check.is_err() || !image_check.unwrap().success() {
            info!("Pulling VLLM image...");
            let pull_output = Command::new("docker")
                .args(["pull", "vllm/vllm-openai:latest"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
                .wait_with_output()
                .await?;

            if !pull_output.status.success() {
                let stderr = String::from_utf8_lossy(&pull_output.stderr);
                error!("Failed to pull VLLM image: {}", stderr);
                return Err(anyhow!("Failed to pull VLLM image: {}", stderr));
            }
        }
        let model_dir = if cfg!(target_os = "windows") {
            // Windows: %USERPROFILE%\.vllm\models
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
            format!("{}\\.vllm\\models", home.replace('\\', "\\\\"))
        } else {
            // Unix-like: ~/.vllm/models
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.vllm/models", home)
        };
        if let Err(e) = std::fs::create_dir_all(&model_dir) {
            warn!("Failed to create model directory {}: {}", model_dir, e);
        }
        info!("Model directory: {}", model_dir);
        let name_flag = format!("--name={}", VLLM_CONTAINER_NAME);

        let port = VLLM_DEFAULT_PORT.to_string();
        let volume_flag = format!("{}:/root/.cache/huggingface/hub", model_dir);

        let mut args = vec![
            "run",
            "-d",
            "--rm",
            &name_flag,

        ];
        #[cfg(not(target_os = "macos"))]
        {
            args.extend(["--network","host","--ipc", "host"]);
        }   

        // Port mapping for macOS and Windows (host network not supported)
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let port_flag = format!("-p{}:{}", VLLM_DEFAULT_PORT, VLLM_DEFAULT_PORT);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        args.push(&port_flag);

        args.extend(["-v", &volume_flag]);

        let template_path = match &self.chat_template_path {
            Some(path) => {
                format!("{}:{}", path, DEFAULT_CHAT_TEMPLATE)
            }
            None => {
                let temp_dir = std::env::var("TEMP_DIR")
                    .unwrap_or_else(|_| "/tmp".to_string());
                let temp_dir = std::path::Path::new(&temp_dir);
                let template_path = temp_dir.join("vllm_default_template.jinja");
                std::fs::write(&template_path, DEFAULT_CHAT_TEMPLATE)?;
                format!("{}:{}", template_path.display(), VLLM_CONTAINER_PATH)
            
            }
        };
        args.push("-v");
        args.push(template_path.as_str());
   
        let tensor_parallel = setup_tensor_parallel!(args, self.gpu_count);
        if let Some(hugging_face_hub_token) = &self.hugging_face_hub_token {
            args.push("-e");
            args.push("HUGGING_FACE_HUB_TOKEN");
            args.push(hugging_face_hub_token);
        }
        args.push("vllm/vllm-openai:latest");
        args.push("--host");
        args.push("0.0.0.0");
        args.push("--port");
        args.push(&port);
        args.push("--chat-template");
        args.push(VLLM_CONTAINER_PATH);
        
        // args.push("--api-key");
        // args.push("dummy");
        args.push("--tensor-parallel-size");
        args.push(&tensor_parallel);
        // Add model if specified
        if self.models_name.is_empty() {
            warn!("No model specified, using default model");
            self.models_name.push("facebook/opt-125m".to_string());
        }
        if let Some(model) = self.models_name.first() {
            args.push("--model");
            args.push(model);
        }
        

        debug!("VLLM args: {:?}", args);
        let output = Command::new("docker")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .wait_with_output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Failed to start VLLM container: {}", stderr);
            return Err(anyhow!("Failed to start VLLM container: {}", stderr));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        self.container_id = Some(container_id);

        // Wait for VLLM to be ready
        self.wait_until_ready(Duration::from_secs(120)).await?;
        info!("VLLM container started successfully");
        Ok(())
    }

    #[allow(dead_code)]
    async fn stop_container(&self) -> Result<()> {
        if let Some(container_id) = &self.container_id {
            info!("Stopping VLLM container...");
            let _ = Command::new("docker")
                .args(["stop", container_id])
                .output()
                .await?;
            info!("VLLM container stopped");
        }
        Ok(())
    }

    async fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        let client = reqwest::Client::new();
        let endpoint = format!("http://localhost:{}/health", VLLM_DEFAULT_PORT);

        info!("Waiting for VLLM to be ready at {}...", endpoint);
        let mut attempt = 0;
        while start.elapsed() < timeout {
            match client
                .get(&endpoint)
                .timeout(Duration::from_secs(2)) // Add timeout
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    let headers = response.headers().clone();
                    let text = response.text().await.unwrap_or_else(|_| "".to_string());

                    info!(
                        "VLLM response - Status: {}, Headers: {:?}, Body: {}",
                        status, headers, text
                    );

                    if status.is_success() {
                        info!("VLLM is ready! Took {:?}", start.elapsed());
                        return Ok(());
                    }
                }
                Err(e) => {
                    info!(
                        "⌛ Waiting for VLLM service... (attempt {}, elapsed: {:.2?}, error: {})", 
                        attempt,
                        start.elapsed(),
                        e
                    );
                    
                }
            }
            attempt += 2;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        error!("Timed out waiting for VLLM to be ready after {:?}", timeout);

        if let Some(container_id) = &self.container_id {
            if let Ok(output) = Command::new("docker")
                .args(&["logs", container_id])
                .output()
                .await
            {
                let logs = String::from_utf8_lossy(&output.stdout);
                error!("Container logs:\n{}", logs);
            }
        }
        Err(anyhow::anyhow!("Timed out waiting for VLLM to be ready"))
    }

    #[allow(dead_code)]
    async fn wait_for_vllm_ready(&self) -> Result<()> {
        info!("Waiting for VLLM to be ready...");
        let client = reqwest::Client::new();
        let health_url = format!("{}/health", self.base_url);

        for _ in 0..30 {
            // Try for 30 seconds
            match client.get(&health_url).send().await {
                Ok(response) if response.status().is_success() => {
                    info!("VLLM is ready");
                    return Ok(());
                }
                _ => {
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }

        Err(anyhow!("Timed out waiting for VLLM to be ready"))
    }
}

impl Engine for VLLMEngine {
    fn init(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Initializing VLLM engine...");
            self.start_container().await?;
            info!("VLLM engine initialized successfully");
            Ok(())
        }
    }

    fn set_models(&mut self, models: Vec<String>) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            if models.is_empty() {
                return Err(anyhow!("Model list cannot be empty"));
            }
            if self.models_name == models {
                info!("Models are the same, no need to update");
                return Ok(());
            }
            info!("Setting models: {:?}", models);
            self.models_name = models.clone();
            if models.first().is_some() {
                // Stop existing container if running
                self.stop_container().await?;
                // Start new container with the specified model
                self.start_container().await?;
            }
            Ok(())
        }
    }

    fn start_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Starting VLLM worker...");
            if self.models_name.is_empty() {
                return Err(anyhow!("No models loaded, cannot start worker"));
            }
            self.start_container().await?;
            info!("VLLM worker started successfully");
            Ok(())
        }
    }

    fn stop_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("Stopping VLLM worker...");
            self.stop_container().await?;
            info!("VLLM worker stopped successfully");
            Ok(())
        }
    }
}

#[tokio::test]
async fn test_pull_model_success() {
    use crate::util;
    util::init_logging();

    let mut engine = VLLMEngine::new(None, None);
    engine.models_name = vec!["facebook/opt-125m".to_string()];

    info!("Docker command: docker run -d --name=vllm_engine_container -p8000:8000 -v...");

    match engine.start_container().await {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to start VLLM container: {}", e);
            return;
        }
    }
    // let result = engine.pull_model("llama2").await;
    // assert!(result.is_ok());
    match engine.stop_container().await {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to stop VLLM container: {}", e);
            return;
        }
    }
}
