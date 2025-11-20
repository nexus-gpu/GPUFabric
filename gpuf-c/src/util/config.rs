use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use crate::util::cmd::EngineType;

const DOCKER_COMPOSE_FILENAME: &str = "docker-compose.yml";
const CONFIG_DIR: &str = ".gpuf";


#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub client: ClientConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub addr: String,
  #[serde(rename = "control_port")]
    pub control_port: u16,
    #[serde(rename = "proxy_port")]
    pub proxy_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClientConfig {
    #[serde(rename = "client_id")]
    pub client_id: String,
    #[serde(rename = "worker_type")]
    pub worker_type: String,
    #[serde(rename = "engine_type")]
    pub engine_type: String,
    #[serde(rename = "cert_chain_path")]
    pub cert_chain_path: String,
    #[serde(rename = "local_addr")]
    pub local_addr: String,
    #[serde(rename = "local_port")]
    pub local_port: u16,
    #[serde(rename = "auto_models")]
    pub auto_models: bool,
    #[serde(rename = "hugging_face_hub_token")]
    pub hugging_face_hub_token: Option<String>,
    #[serde(rename = "chat_template_path")]
    pub chat_template_path: Option<String>,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_str = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;
        
        toml::from_str(&config_str)
            .with_context(|| "Failed to parse config file")
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DockerConfig {
    version: String,
    services: HashMap<String, Service>,
    volumes: HashMap<String, Volume>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Service {
    image: String,
    container_name: String,
    ports: Vec<String>,
    volumes: Vec<String>,
    environment: Option<HashMap<String, String>>,
    #[serde(rename = "shm_size")]
    shm_size: Option<String>,
    #[serde(rename = "runtime")]
    runtime: Option<String>,
    #[serde(rename = "devices")]
    devices: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Volume {
    driver: String,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Deploy {
    resources: Resources,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Resources {
    reservations: Reservations,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Reservations {
    devices: Vec<Device>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Device {
    capabilities: Vec<String>,
    driver: String,
    count: u32,
}

impl DockerConfig {
    pub fn new(engine_type: EngineType) -> Self {
        let mut services = HashMap::new();
        let service_name = match engine_type {
            EngineType::VLLM => "vllm",
            EngineType::OLLAMA => "ollama",
            EngineType::LLAMA => "llama",
        };

        let service = match engine_type {
            EngineType::VLLM => Service {
                image: "vllm/vllm-openai:latest".to_string(),
                container_name: "vllm_engine_container".to_string(),
                ports: vec!["8000:8000".to_string()],
                volumes: vec![
                    "~/.vllm/models:/root/.cache/huggingface/hub".to_string(),
                    "${PWD}/configs:/app/configs".to_string(),
                ],
                environment: Some(HashMap::from([
                    ("MODEL".to_string(), "facebook/opt-125m".to_string()),
                ])),
                shm_size: Some("2g".to_string()),
                runtime: None,
                devices: None,
            },
            EngineType::LLAMA => Service {
                image: "ghcr.io/ggerganov/llama.cpp:server".to_string(),
                container_name: "llama_engine_container".to_string(),
                ports: vec!["8080:8080".to_string()],
                volumes: vec![
                    "~/.llama/models:/models".to_string(),
                ],
                environment: Some(HashMap::from([
                    ("LLAMA_ARG_MODEL".to_string(), "/models/model.gguf".to_string()),
                    ("LLAMA_ARG_CTX_SIZE".to_string(), "2048".to_string()),
                    ("LLAMA_ARG_N_GPU_LAYERS".to_string(), "99".to_string()),
                ])),
                shm_size: Some("2g".to_string()),
                runtime: Some("nvidia".to_string()),
                devices: None,
            },

            EngineType::OLLAMA => Service {
                image: "ollama/ollama:latest".to_string(),
                container_name: "ollama_engine_container".to_string(),
                ports: vec!["11434:11434".to_string()],
                volumes: vec![
                    "~/.ollama/models:/root/.ollama/models".to_string(),
                ],
                environment: Some(HashMap::from([
                    ("OLLAMA_HOST".to_string(), "0.0.0.0".to_string()),
                    ("OLLAMA_GPU_LAYERS".to_string(), "all".to_string()),
                ])),
                shm_size: Some("2g".to_string()),
                runtime: Some("nvidia".to_string()),
                devices: Some(vec![
                    "/dev/kfd".to_string(),
                    "/dev/dri".to_string(),
                ]),
            },
        };

        services.insert(service_name.to_string(), service);

        let mut volumes = HashMap::new();
        volumes.insert("model_data".to_string(), Volume {
            driver: "local".to_string(),
        });

        DockerConfig {
            version: "3.8".to_string(),
            services,
            volumes,
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let config_dir = path.parent().expect("Invalid config directory");
        if !config_dir.exists() {
            fs::create_dir_all(config_dir)?;
        }

        let yaml = serde_yaml::to_string(self)?;
        fs::write(path, yaml)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let yaml = fs::read_to_string(path)?;
        let config: DockerConfig = serde_yaml::from_str(&yaml)?;
        Ok(config)
    }
}

pub fn get_config_path() -> PathBuf {
    let home_dir = dirs::home_dir().expect("Could not find home directory");
    home_dir.join(CONFIG_DIR).join(DOCKER_COMPOSE_FILENAME)
}

#[allow(dead_code)]
pub fn ensure_config(engine_type: EngineType) -> Result<DockerConfig> {
    let config_path = get_config_path();
    
    if config_path.exists() {
        DockerConfig::load_from_file(&config_path)
    } else {
        let config = DockerConfig::new(engine_type);
        config.save_to_file(&config_path)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_docker_config_creation() {
        let config = DockerConfig::new(EngineType::VLLM);
        assert_eq!(config.version, "3.8");
        assert!(config.services.contains_key("vllm"));

        let config = DockerConfig::new(EngineType::OLLAMA);
        assert_eq!(config.version, "3.8");
        assert!(config.services.contains_key("ollama"));
    }

    #[test]
    fn test_save_and_load_config() -> Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("docker-compose.yml");
        
        let config = DockerConfig::new(EngineType::VLLM);
        config.save_to_file(&config_path)?;
        
        let loaded_config = DockerConfig::load_from_file(&config_path)?;
        assert_eq!(config.version, loaded_config.version);
        
        Ok(())
    }
}