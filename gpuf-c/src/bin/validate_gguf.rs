//! GGUF Model Format Validator
//! 
//! This tool validates the format and integrity of downloaded GGUF model files

use anyhow::Result;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use clap::{Arg, Command};

fn main() -> Result<()> {
    let matches = Command::new("validate_gguf")
        .version("1.0")
        .about("Validate GGUF model file format and integrity")
        .arg(
            Arg::new("file")
                .help("Path to the GGUF file to validate")
                .required(true)
                .index(1),
        )
        .get_matches();

    let file_path = PathBuf::from(matches.get_one::<String>("file").unwrap());
    
    println!("🔍 GGUF Model Validator");
    println!("📁 File: {:?}", file_path);
    println!();

    validate_gguf_file(&file_path)?;

    Ok(())
}

fn validate_gguf_file(file_path: &PathBuf) -> Result<()> {
    // Check if file exists
    if !file_path.exists() {
        return Err(anyhow::anyhow!("File does not exist: {:?}", file_path));
    }

    // Get file size
    let metadata = std::fs::metadata(file_path)?;
    let file_size = metadata.len();
    println!("📊 File size: {} bytes ({:.2} MB)", file_size, file_size as f64 / (1024.0 * 1024.0));

    // Open and read file
    let mut file = File::open(file_path)?;
    
    // Read magic number (first 4 bytes)
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    
    // Check GGUF magic number
    let expected_magic = b"GGUF";
    if magic != *expected_magic {
        println!("❌ Invalid GGUF magic number: {:?}", magic);
        println!("🔢 Expected magic: {:?}", expected_magic);
        return Err(anyhow::anyhow!("This is not a valid GGUF file"));
    }
    
    println!("✅ Valid GGUF magic number");

    // Read version (next 4 bytes)
    let mut version_bytes = [0u8; 4];
    file.read_exact(&mut version_bytes)?;
    let version = u32::from_le_bytes(version_bytes);
    println!("📋 GGUF version: {}", version);

    // Read tensor count (next 8 bytes)
    let mut tensor_count_bytes = [0u8; 8];
    file.read_exact(&mut tensor_count_bytes)?;
    let tensor_count = u64::from_le_bytes(tensor_count_bytes);
    println!("🧩 Tensor count: {}", tensor_count);

    // Read KV count (next 8 bytes)
    let mut kv_count_bytes = [0u8; 8];
    file.read_exact(&mut kv_count_bytes)?;
    let kv_count = u64::from_le_bytes(kv_count_bytes);
    println!("🔑 KV count: {}", kv_count);

    // Basic validation checks
    if version < 1 || version > 3 {
        println!("⚠️  Warning: Unusual GGUF version: {}", version);
    }

    if tensor_count == 0 {
        return Err(anyhow::anyhow!("Invalid tensor count: 0"));
    }

    if kv_count == 0 {
        return Err(anyhow::anyhow!("Invalid KV count: 0"));
    }

    // Check if file size is reasonable for the reported content
    let minimum_size = 4 + 4 + 8 + 8; // magic + version + tensor_count + kv_count
    if file_size < minimum_size {
        return Err(anyhow::anyhow!("File too small for valid GGUF: {} < {}", file_size, minimum_size));
    }

    println!("✅ Basic GGUF structure validation passed");
    
    // Try to read some metadata
    println!();
    println!("📖 Reading metadata...");
    
    // Read KV pairs (simplified - just show first few)
    for i in 0..std::cmp::min(kv_count, 10) {
        let mut key_len_bytes = [0u8; 8];
        file.read_exact(&mut key_len_bytes)?;
        let key_len = u64::from_le_bytes(key_len_bytes);
        
        if key_len > 1024 { // Reasonable limit
            return Err(anyhow::anyhow!("Key length too large: {}", key_len));
        }
        
        let mut key_bytes = vec![0u8; key_len as usize];
        file.read_exact(&mut key_bytes)?;
        let key = String::from_utf8_lossy(&key_bytes);
        
        let mut value_type_bytes = [0u8; 4];
        file.read_exact(&mut value_type_bytes)?;
        let value_type = u32::from_le_bytes(value_type_bytes);
        
        println!("  📝 {}: type {}", key, value_type);
        
        // Skip value for simplicity
        match value_type {
            0 => { // uint8
                let mut value = [0u8; 1];
                file.read_exact(&mut value)?;
            }
            1 => { // uint8
                let mut value = [0u8; 1];
                file.read_exact(&mut value)?;
            }
            2 | 3 => { // uint16, uint32
                let mut value = [0u8; 4];
                file.read_exact(&mut value)?;
            }
            4 | 5 => { // uint64, float32
                let mut value = [0u8; 8];
                file.read_exact(&mut value)?;
            }
            6 => { // bool
                let mut value = [0u8; 1];
                file.read_exact(&mut value)?;
            }
            7 | 8 => { // string
                let mut value_len_bytes = [0u8; 8];
                file.read_exact(&mut value_len_bytes)?;
                let value_len = u64::from_le_bytes(value_len_bytes);
                if value_len > 1000000 { // Reasonable limit
                    return Err(anyhow::anyhow!("String value too large: {}", value_len));
                }
                let mut value_bytes = vec![0u8; value_len as usize];
                file.read_exact(&mut value_bytes)?;
                let value = String::from_utf8_lossy(&value_bytes);
                if key.contains("name") || key.contains("type") || key.contains("architecture") {
                    println!("    📄 Value: {}", value);
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown value type: {}", value_type));
            }
        }
    }
    
    if kv_count > 10 {
        println!("  ... and {} more key-value pairs", kv_count - 10);
    }

    println!();
    println!("🎉 GGUF file validation completed successfully!");
    println!("✅ This appears to be a valid GGUF model file");

    Ok(())
}
