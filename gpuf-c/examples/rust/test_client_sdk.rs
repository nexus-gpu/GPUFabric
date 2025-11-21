use gpuf_c::{init, gpuf_client_init, gpuf_client_connect, gpuf_client_get_status, 
              gpuf_client_get_device_info, gpuf_client_get_metrics, gpuf_client_update_device_info,
              gpuf_client_disconnect, gpuf_client_cleanup, gpuf_get_last_error,
              gpuf_llm_init, gpuf_llm_generate, gpuf_llm_is_initialized, gpuf_llm_unload};
use std::ffi::{CString, CStr};

// Allow unused_unsafe because these are required for FFI calls even though 
// the functions themselves are safe in their implementation
#[allow(unused_unsafe)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the library
    init()?;
    println!("GPUFabric library initialized successfully");

    // Create client configuration
    let config = r#"{
        "server_addr": "127.0.0.1",
        "control_port": 17000,
        "proxy_port": 17001,
        "client_id": "test-device-12345",
        "device_name": "Test Device",
        "auto_register": true,
        "heartbeat_interval_secs": 30,
        "enable_monitoring": true
    }"#;

    // Initialize client
    let config_c = CString::new(config)?;
    let result = unsafe { gpuf_client_init(config_c.as_ptr()) }; // Required for FFI call
    
    if result != 0 {
        let error_ptr = unsafe { gpuf_get_last_error() }; // Required for FFI call
        let error_msg = unsafe { CString::from_raw(error_ptr) }; // Required for FFI call
        println!("Failed to initialize client: {}", error_msg.to_string_lossy());
        return Err("Client initialization failed".into());
    }
    
    println!("Client initialized successfully");

    // Get device information
    let device_info_ptr = unsafe { gpuf_client_get_device_info() }; // Required for FFI call
    if !device_info_ptr.is_null() {
        let device_info = unsafe { CString::from_raw(device_info_ptr) }; // Required for FFI call
        println!("Device info: {}", device_info.to_string_lossy());
    }

    // Get client status
    let status_ptr = unsafe { gpuf_client_get_status() }; // Required for FFI call
    if !status_ptr.is_null() {
        let status = unsafe { CString::from_raw(status_ptr) }; // Required for FFI call
        println!("Initial status: {}", status.to_string_lossy());
    }

    // Try to connect (this will likely fail without a running server)
    println!("Attempting to connect to server...");
    let connect_result = unsafe { gpuf_client_connect() };
    if connect_result == 0 {
        println!("Connected successfully!");
        
        // Get updated status
        let status_ptr = unsafe { gpuf_client_get_status() }; // Required for FFI call
        if !status_ptr.is_null() {
            let status = unsafe { CString::from_raw(status_ptr) }; // Required for FFI call
            println!("Connected status: {}", status.to_string_lossy());
        }

        // Wait a bit and get metrics
        std::thread::sleep(std::time::Duration::from_secs(2));
        
        // Get client metrics
        let metrics_ptr = unsafe { gpuf_client_get_metrics() }; // Required for FFI call
        if !metrics_ptr.is_null() {
            let metrics = unsafe { CString::from_raw(metrics_ptr) }; // Required for FFI call
            println!("Client metrics: {}", metrics.to_string_lossy());
        }
    } else {
        let error_ptr = unsafe { gpuf_get_last_error() }; // Required for FFI call
        let error_msg = unsafe { CString::from_raw(error_ptr) }; // Required for FFI call
        println!("Connection failed (expected without server): {}", error_msg.to_string_lossy());
    }

    // Update device information
    println!("Updating device information...");
    let update_result = unsafe { gpuf_client_update_device_info() }; // Required for FFI call
    if update_result != 0 {
        let error_ptr = unsafe { gpuf_get_last_error() }; // Required for FFI call
        let error_msg = unsafe { CString::from_raw(error_ptr) }; // Required for FFI call
        println!("Device info update failed: {}", error_msg.to_string_lossy());
    }

    // Disconnect client
    println!("Disconnecting...");
    let disconnect_result = unsafe { gpuf_client_disconnect() }; // Required for FFI call
    if disconnect_result != 0 {
        let error_ptr = unsafe { gpuf_get_last_error() }; // Required for FFI call
        let error_msg = unsafe { CString::from_raw(error_ptr) }; // Required for FFI call
        println!("Disconnect failed: {}", error_msg.to_string_lossy());
    }

    // Cleanup client
    println!("Cleaning up...");
    let cleanup_result = unsafe { gpuf_client_cleanup() }; // Required for FFI call
    if cleanup_result != 0 {
        let error_ptr = unsafe { gpuf_get_last_error() }; // Required for FFI call
        let error_msg = unsafe { CString::from_raw(error_ptr) }; // Required for FFI call
        println!("Cleanup failed: {}", error_msg.to_string_lossy());
    }

    println!("\n========== LLM Function Test ==========");
    
    // Test LLM functionality (requires valid model path)
    test_llm_functionality()?;
    
    println!("\n✅ All tests completed!");
    
    Ok(())
}

/// Test LLM functionality
fn test_llm_functionality() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting LLM function test...");
    
    // Note: Please replace with actual model path
    let model_path = "/path/to/your/model.gguf"; // Placeholder path
    println!("Note: Currently using placeholder model path: {}", model_path);
    
    // Initialize LLM engine
    println!("Initializing LLM engine...");
    let init_result = {
        // FFI call requires unsafe block
        unsafe {
            gpuf_llm_init(
                std::ffi::CString::new(model_path).unwrap().as_ptr(),
                2048, // Context size
                0     // GPU layers (0 = CPU only)
            )
        }
    };
    
    if init_result != 0 {
        // FFI call requires unsafe block
        let error_msg = unsafe { gpuf_get_last_error() };
        let error_str = unsafe { CString::from_raw(error_msg) };
        println!("LLM engine initialization failed: {}", error_str.to_string_lossy());
        println!("Note: This is expected because placeholder path is used");
        return Ok(()); // Don't return error as this is expected
    }
    
    println!("LLM engine initialized successfully");
    
    // Check initialization status
    let is_init = {
        // FFI call requires unsafe block
        unsafe { gpuf_llm_is_initialized() }
    };
    println!("LLM engine initialization status check: {}", is_init != 0);
    
    if is_init != 0 {
        // Generate text
        let prompt = "Hello, how are you?";
        println!("Generating text with prompt: {}", prompt);
        
        let response = {
            // FFI call requires unsafe block
            unsafe { gpuf_llm_generate(std::ffi::CString::new(prompt).unwrap().as_ptr(), 100) }
        };
        
        if response.is_null() {
            // FFI call requires unsafe block
            let error_msg = unsafe { gpuf_get_last_error() };
            let error_str = unsafe { CString::from_raw(error_msg) };
            println!("Text generation failed: {}", error_str.to_string_lossy());
        } else {
            let response_str = unsafe { CStr::from_ptr(response) };
            println!("Generated text: {}", response_str.to_string_lossy());
        }
    }
    
    // Unload engine
    println!("Unloading LLM engine...");
    let unload_result = {
        // FFI call requires unsafe block
        unsafe { gpuf_llm_unload() }
    };
    
    if unload_result != 0 {
        // FFI call requires unsafe block
        let error_msg = unsafe { gpuf_get_last_error() };
        let error_str = unsafe { CString::from_raw(error_msg) };
        println!("LLM engine unload failed: {}", error_str.to_string_lossy());
    } else {
        println!("LLM engine unloaded successfully");
    }
    
    println!("LLM function test completed");
    Ok(())
}
