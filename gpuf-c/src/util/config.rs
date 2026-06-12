#[cfg(not(target_os = "ios"))]
use crate::llm_engine::{OLLAMA_DEFAULT_IMAGE, VLLM_DEFAULT_IMAGE};
use crate::util::cmd::EngineType;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "ios")]
const OLLAMA_DEFAULT_IMAGE: &str = "ollama/ollama:0.5.7";
#[cfg(target_os = "ios")]
const VLLM_DEFAULT_IMAGE: &str = "vllm/vllm-openai:v0.8.5";

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
    #[serde(rename = "control_tls")]
    pub control_tls: Option<bool>,
    #[serde(rename = "control_tls_server_name")]
    pub control_tls_server_name: Option<String>,
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
    #[serde(rename = "n_ctx")]
    pub n_ctx: u32,
    #[serde(rename = "n_gpu_layers")]
    pub n_gpu_layers: u32,

    #[serde(rename = "llama_split_mode")]
    pub llama_split_mode: Option<String>,

    #[serde(rename = "llama_main_gpu")]
    pub llama_main_gpu: Option<i32>,

    #[serde(rename = "llama_devices")]
    pub llama_devices: Option<String>,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_str = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        toml::from_str(&config_str).with_context(|| "Failed to parse config file")
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
    #[serde(rename = "security_opt")]
    security_opt: Option<Vec<String>>,
    #[serde(rename = "cap_drop")]
    cap_drop: Option<Vec<String>>,
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
                image: VLLM_DEFAULT_IMAGE.to_string(),
                container_name: "vllm_engine_container".to_string(),
                ports: vec!["127.0.0.1:8000:8000".to_string()],
                volumes: vec![
                    "~/.vllm/models:/root/.cache/huggingface/hub".to_string(),
                    "${PWD}/configs:/app/configs".to_string(),
                ],
                environment: Some(HashMap::from([(
                    "MODEL".to_string(),
                    "facebook/opt-125m".to_string(),
                )])),
                shm_size: Some("2g".to_string()),
                runtime: None,
                devices: None,
                security_opt: Some(vec!["no-new-privileges:true".to_string()]),
                cap_drop: Some(vec!["ALL".to_string()]),
            },
            EngineType::LLAMA => Service {
                image: "ghcr.io/ggerganov/llama.cpp:server".to_string(),
                container_name: "llama_engine_container".to_string(),
                ports: vec!["127.0.0.1:8080:8080".to_string()],
                volumes: vec!["~/.llama/models:/models".to_string()],
                environment: Some(HashMap::from([
                    (
                        "LLAMA_ARG_MODEL".to_string(),
                        "/models/model.gguf".to_string(),
                    ),
                    ("LLAMA_ARG_CTX_SIZE".to_string(), "2048".to_string()),
                    ("LLAMA_ARG_N_GPU_LAYERS".to_string(), "99".to_string()),
                ])),
                shm_size: Some("2g".to_string()),
                runtime: Some("nvidia".to_string()),
                devices: None,
                security_opt: Some(vec!["no-new-privileges:true".to_string()]),
                cap_drop: Some(vec!["ALL".to_string()]),
            },

            EngineType::OLLAMA => Service {
                image: OLLAMA_DEFAULT_IMAGE.to_string(),
                container_name: "ollama_engine_container".to_string(),
                ports: vec!["127.0.0.1:11434:11434".to_string()],
                volumes: vec!["~/.ollama/models:/root/.ollama/models".to_string()],
                environment: Some(HashMap::from([
                    ("OLLAMA_HOST".to_string(), "127.0.0.1".to_string()),
                    ("OLLAMA_GPU_LAYERS".to_string(), "all".to_string()),
                ])),
                shm_size: Some("2g".to_string()),
                runtime: Some("nvidia".to_string()),
                devices: Some(vec!["/dev/kfd".to_string(), "/dev/dri".to_string()]),
                security_opt: Some(vec!["no-new-privileges:true".to_string()]),
                cap_drop: Some(vec!["ALL".to_string()]),
            },
        };

        services.insert(service_name.to_string(), service);

        let mut volumes = HashMap::new();
        volumes.insert(
            "model_data".to_string(),
            Volume {
                driver: "local".to_string(),
            },
        );

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

        fs::write(path, self.to_compose_yaml())?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let yaml = fs::read_to_string(path)?;
        Self::from_generated_compose_yaml(&yaml)
    }

    fn to_compose_yaml(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "version: '{}'
",
            self.version
        ));
        out.push_str(
            "services:
",
        );

        let mut service_names: Vec<_> = self.services.keys().collect();
        service_names.sort();
        for name in service_names {
            let service = &self.services[name];
            out.push_str(&format!(
                "  {}:
",
                name
            ));
            out.push_str(&format!(
                "    image: '{}'
",
                service.image
            ));
            out.push_str(&format!(
                "    container_name: '{}'
",
                service.container_name
            ));
            if !service.ports.is_empty() {
                out.push_str(
                    "    ports:
",
                );
                for port in &service.ports {
                    out.push_str(&format!(
                        "      - '{}'
",
                        port
                    ));
                }
            }
            if !service.volumes.is_empty() {
                out.push_str(
                    "    volumes:
",
                );
                for volume in &service.volumes {
                    out.push_str(&format!(
                        "      - '{}'
",
                        volume
                    ));
                }
            }
            if let Some(environment) = &service.environment {
                if !environment.is_empty() {
                    out.push_str(
                        "    environment:
",
                    );
                    let mut keys: Vec<_> = environment.keys().collect();
                    keys.sort();
                    for key in keys {
                        out.push_str(&format!(
                            "      {}: '{}'
",
                            key, environment[key]
                        ));
                    }
                }
            }
            if let Some(shm_size) = &service.shm_size {
                out.push_str(&format!(
                    "    shm_size: '{}'
",
                    shm_size
                ));
            }
            if let Some(runtime) = &service.runtime {
                out.push_str(&format!(
                    "    runtime: '{}'
",
                    runtime
                ));
            }
            if let Some(devices) = &service.devices {
                if !devices.is_empty() {
                    out.push_str(
                        "    devices:
",
                    );
                    for device in devices {
                        out.push_str(&format!(
                            "      - '{}'
",
                            device
                        ));
                    }
                }
            }
            if let Some(security_opt) = &service.security_opt {
                if !security_opt.is_empty() {
                    out.push_str(
                        "    security_opt:
",
                    );
                    for opt in security_opt {
                        out.push_str(&format!(
                            "      - '{}'
",
                            opt
                        ));
                    }
                }
            }
            if let Some(cap_drop) = &service.cap_drop {
                if !cap_drop.is_empty() {
                    out.push_str(
                        "    cap_drop:
",
                    );
                    for cap in cap_drop {
                        out.push_str(&format!(
                            "      - '{}'
",
                            cap
                        ));
                    }
                }
            }
        }

        out.push_str(
            "volumes:
",
        );
        let mut volume_names: Vec<_> = self.volumes.keys().collect();
        volume_names.sort();
        for name in volume_names {
            out.push_str(&format!(
                "  {}:
",
                name
            ));
            out.push_str(&format!(
                "    driver: '{}'
",
                self.volumes[name].driver
            ));
        }
        out
    }

    fn from_generated_compose_yaml(yaml: &str) -> Result<Self> {
        let version = yaml
            .lines()
            .find_map(|line| line.trim().strip_prefix("version:"))
            .map(|v| v.trim().trim_matches('\'').trim_matches('"').to_string())
            .unwrap_or_else(|| "3.8".to_string());

        let mut services = HashMap::new();
        let mut in_services = false;
        for line in yaml.lines() {
            let trimmed = line.trim_end();
            if trimmed == "services:" {
                in_services = true;
                continue;
            }
            if trimmed == "volumes:" {
                break;
            }
            if in_services && line.starts_with("  ") && !line.starts_with("    ") {
                let name = trimmed.trim().trim_end_matches(':');
                if !name.is_empty() {
                    let engine = match name {
                        "vllm" => EngineType::VLLM,
                        "ollama" => EngineType::OLLAMA,
                        "llama" => EngineType::LLAMA,
                        _ => continue,
                    };
                    let generated = DockerConfig::new(engine);
                    if let Some(service) = generated.services.get(name) {
                        services.insert(name.to_string(), service.clone());
                    }
                }
            }
        }

        if services.is_empty() {
            return Err(anyhow::anyhow!(
                "Unsupported docker compose file: no known GPUFabric services found"
            ));
        }

        let mut volumes = HashMap::new();
        volumes.insert(
            "model_data".to_string(),
            Volume {
                driver: "local".to_string(),
            },
        );

        Ok(Self {
            version,
            services,
            volumes,
        })
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
