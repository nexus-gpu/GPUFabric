pub mod inference_service;
pub mod llama_engine;
pub mod llama_server;
pub mod ollama_engine;
pub mod vllm_engine;

// Re-export commonly used types
use crate::util::cmd::EngineType;
use anyhow::Result;
pub use llama_engine::LlamaEngine;
use reqwest::Client;

const OLLAMA_DEFAULT_PORT: u16 = 11434;
const OLLAMA_CONTAINER_NAME: &str = "ollama_engine_container";

const VLLM_DEFAULT_PORT: u16 = 8000;
const VLLM_CONTAINER_NAME: &str = "vllm_engine_container";
const VLLM_CONTAINER_PATH: &str = "/app/default_template.jinja";

const DEFAULT_CHAT_TEMPLATE: &str = r#"
{% if not add_generation_prompt is defined %}
  {% set add_generation_prompt = false %}
{% endif %}
{% for message in messages %}
  <message role="{{ message['role'] }}">
    {{ message['content'] }}
  </message>
{% endfor %}
{% if add_generation_prompt %}
  <message role="assistant">
    {{ add_generation_prompt }}
  </message>
{% endif %}
"#;

pub trait Engine {
    fn init(&mut self) -> impl std::future::Future<Output = Result<()>> + Send;
    #[allow(dead_code)]
    fn set_models(
        &mut self,
        models: Vec<String>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
    #[allow(dead_code)]
    fn start_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send;
    #[allow(dead_code)]
    fn stop_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send;
}

#[allow(dead_code)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub status: String,
}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct VLLMEngine {
    #[allow(dead_code)]
    models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    models_name: Vec<String>,
    #[allow(dead_code)]
    worker_handler: Option<tokio::task::JoinHandle<()>>,
    #[allow(dead_code)]
    show_worker_log: bool,
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    gpu_count: u32,
    container_id: Option<String>,
    //HUGGING_FACE_HUB_TOKEN
    hugging_face_hub_token: Option<String>,
    chat_template_path: Option<String>,
}

impl Default for VLLMEngine {
    fn default() -> Self {
        Self::new(None, None)
    }
}

//TODO: delete unused field
#[allow(dead_code)]
pub struct OllamaEngine {
    models: [i32; 16],
    models_name: Vec<String>,
    client: Client,
    base_url: String,
    container_id: Option<String>,
    gpu_count: u32,
}

impl Default for OllamaEngine {
    fn default() -> Self {
        Self::new()
    }
}
#[allow(dead_code)]
pub enum AnyEngine {
    VLLM(VLLMEngine),
    Ollama(OllamaEngine),
    Llama(LlamaEngine),
}

impl Engine for AnyEngine {
    fn init(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            match self {
                AnyEngine::VLLM(engine) => engine.init().await,
                AnyEngine::Ollama(engine) => engine.init().await,
                AnyEngine::Llama(engine) => engine.init().await,
            }
        }
    }

    fn set_models(
        &mut self,
        models: Vec<String>,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            match self {
                AnyEngine::VLLM(engine) => engine.set_models(models).await,
                AnyEngine::Ollama(engine) => engine.set_models(models).await,
                AnyEngine::Llama(engine) => engine.set_models(models).await,
            }
        }
    }

    fn start_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            match self {
                AnyEngine::VLLM(engine) => engine.start_worker().await,
                AnyEngine::Ollama(engine) => engine.start_worker().await,
                AnyEngine::Llama(engine) => engine.start_worker().await,
            }
        }
    }

    fn stop_worker(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            match self {
                AnyEngine::VLLM(engine) => engine.stop_worker().await,
                AnyEngine::Ollama(engine) => engine.stop_worker().await,
                AnyEngine::Llama(engine) => engine.stop_worker().await,
            }
        }
    }
}

#[allow(dead_code)]
pub fn create_engine(
    engine_type: EngineType,
    hugging_face_hub_token: Option<String>,
    chat_template_path: Option<String>,
) -> AnyEngine {
    match engine_type {
        EngineType::VLLM => {
            AnyEngine::VLLM(VLLMEngine::new(hugging_face_hub_token, chat_template_path))
        }
        EngineType::OLLAMA => AnyEngine::Ollama(OllamaEngine::new()),
        EngineType::LLAMA => AnyEngine::Llama(LlamaEngine::new()),
    }
}
