// Complete llama.cpp usage example
use gpuf_c::llm_engine::{Engine, LlamaEngine};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 GPUFabric Llama.cpp Usage Example");

    // 1. Check Android compatibility
    #[cfg(target_os = "android")]
    {
        use gpuf_c::android_compat;

        let api_level = android_compat::get_android_api_level();
        let supports_posix = android_compat::supports_posix_madvise();
        let llama_available = android_compat::is_llama_available();

        println!("📱 Android API Level: {}", api_level);
        println!("✅ POSIX madvise support: {}", supports_posix);
        println!("🔧 Llama.cpp available: {}", llama_available);

        if !llama_available {
            return Err("Llama.cpp not available, please check build configuration".into());
        }
    }

    // 2. Display llama.cpp version
    #[cfg(target_os = "android")]
    {
        let version = android_compat::get_llama_version();
        println!("📦 Llama.cpp version: {}", version);
    }

    // 3. Initialize engine
    println!("🔧 Initializing LlamaEngine...");

    // Model path - in actual use, this should be your GGUF model file path
    let model_path = "/data/local/tmp/model.gguf";

    // If model file doesn't exist, create a simulated engine
    let mut engine = if Path::new(model_path).exists() {
        println!("📁 Model file found: {}", model_path);
        LlamaEngine::with_config(
            model_path.to_string(),
            2048,
            4096,
            0,
            gpuf_c::util::cmd::LlamaSplitModeArg::Layer,
            0,
            None,
        )
    } else {
        println!("⚠️  Model file doesn't exist, using simulation mode");
        return simulate_usage();
    };

    // Initialize the engine
    engine.init().await?;

    // 4. Display basic engine information
    println!("📊 Engine initialized successfully");
    println!("  - Model path: {:?}", engine.model_path);
    println!("  - Context size: {}", engine.n_ctx);
    println!("  - GPU layers: {}", engine.n_gpu_layers);
    println!("  - Initialized: {}", engine.is_initialized);

    // 5. Text generation example
    println!("\n🎯 Starting text generation...");
    let prompt = "Hello, please introduce artificial intelligence";

    match engine.generate(prompt, 100).await {
        Ok(response) => {
            println!("✅ Generation successful:");
            println!("📝 {}", response.0);
        }
        Err(e) => {
            println!("❌ Generation failed: {}", e);
        }
    }

    println!("\n🎉 Example completed!");
    Ok(())
}

// Simulation usage function (when no real model is available)
fn simulate_usage() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Simulation mode:");
    println!("  - In actual use, please provide a valid GGUF model file");
    println!("  - Model files should be placed in application-accessible directories");
    println!("  - Recommend using Android 10+ (API 29+) for best performance");

    Ok(())
}

// JNI usage example (in Android applications)
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_pocketpal_LlamaExample_nativeTest(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
    model_path: jni::objects::JString,
) -> jni::sys::jstring {
    use jni::sys::{jstring, JNI_TRUE};

    // Get model path
    let model_path_str = match env.get_string(model_path) {
        Ok(s) => s,
        Err(_) => {
            return env
                .new_string("Error: Invalid model path")
                .unwrap()
                .into_inner();
        }
    };

    // In actual applications, LlamaEngine would be created and used here
    let result = format!("Model path received: {}", model_path_str.to_string_lossy());

    // Return result
    env.new_string(result).unwrap().into_inner()
}
