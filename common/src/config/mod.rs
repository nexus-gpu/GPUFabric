use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct GpuModelConfig {
    pub model_to_id: HashMap<String, u16>,
    pub id_to_tflops: HashMap<u16, f32>,
}

impl GpuModelConfig {
    pub fn load() -> Result<Self> {
        let model_to_id = include_str!("model_to_id.json");
        let id_to_tflops = include_str!("id_to_tflops.json");

        let model_to_id: HashMap<String, u16> = serde_json::from_str(model_to_id)?;
        let id_to_tflops: HashMap<u16, f32> = serde_json::from_str(id_to_tflops)?;

        Ok(Self {
            model_to_id,
            id_to_tflops,
        })
    }

    pub fn get_id(&self, model: &str) -> Option<u16> {
        self.model_to_id.get(model).copied()
    }

    pub fn get_tflops(&self, id: u16) -> Option<f32> {
        self.id_to_tflops.get(&id).copied()
    }
}
