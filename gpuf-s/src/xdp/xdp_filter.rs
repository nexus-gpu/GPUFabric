use anyhow::{Context, Result};
use aya::{maps::HashMap, Ebpf, EbpfLoader};
use std::sync::Arc;
use tokio::sync::Mutex;

#[allow(dead_code)] // Experimental XDP implementation for high-performance packet filtering
pub struct XdpFilter {
    bpf: Arc<Mutex<Ebpf>>,
}

impl XdpFilter {
    #[allow(dead_code)] // Experimental XDP implementation for high-performance packet filtering
    pub async fn new(interface: &str, obj_path: &str) -> Result<Self> {
        let mut bpf = EbpfLoader::new()
            .load_file(obj_path)
            .context(format!("Failed to load XDP eBPF object file: {}", obj_path))?;

        // get XDP program
        let program: &mut aya::programs::Xdp = bpf
            .program_mut("xdp_auth_filter")
            .ok_or(anyhow::anyhow!("Optional value was None"))? // eBPF program name
            .try_into()?;

        // TODO: if the interface has already attached eBPF program, need to detach
        program.load()?;
        program.attach(interface, aya::programs::XdpFlags::default())?;
        Ok(Self {
            bpf: Arc::new(Mutex::new(bpf)),
        })
    }

    #[allow(dead_code)] // Experimental XDP implementation for high-performance packet filtering
    pub async fn add_api_key(&self, key: &[u8]) -> Result<()> {
        let mut bpf = self.bpf.lock().await;
        // corresponding eBPF map key length is 16
        let mut api_keys: HashMap<_, [u8; 16], u8> = HashMap::try_from(
            bpf.map_mut("api_keys")
                .ok_or(anyhow::anyhow!("Failed to get map"))?,
        )?;

        let mut padded_key = [0u8; 16];
        let len = key.len().min(16);
        padded_key[..len].copy_from_slice(&key[..len]);

        api_keys.insert(&padded_key, &1, 0)?;
        println!("Added API key: {:?}", String::from_utf8_lossy(&padded_key));
        Ok(())
    }

    #[allow(dead_code)] // Experimental XDP implementation for high-performance packet filtering
    pub async fn remove_api_key(&self, key: &[u8]) -> Result<()> {
        let mut bpf = self.bpf.lock().await;
        let mut api_keys: HashMap<_, [u8; 16], u8> = HashMap::try_from(
            bpf.map_mut("api_keys")
                .ok_or(anyhow::anyhow!("Failed to get map"))?,
        )?;

        let mut padded_key = [0u8; 16];
        let len = key.len().min(16);
        padded_key[..len].copy_from_slice(&key[..len]);

        api_keys.remove(&padded_key)?;
        println!(
            "Removed API key: {:?}",
            String::from_utf8_lossy(&padded_key)
        );
        Ok(())
    }
}

#[tokio::test]
async fn test_xdp_filter() {
    let xdp_filter = XdpFilter::new("enp5s0", "./gpuf-s/src/xdp/xdp_auth_filter.o")
        .await
        .expect("Failed to create XDP filter");
    xdp_filter
        .add_api_key(b"1234567890")
        .await
        .expect("Failed to add API key");
    xdp_filter
        .remove_api_key(b"1234567890")
        .await
        .expect("Failed to remove API key");
}
