// 完整的 llama.cpp 使用示例
use gpuf_c::llama_engine::{LlamaEngine};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 GPUFabric Llama.cpp 使用示例");
    
    // 1. 检查 Android 兼容性
    #[cfg(target_os = "android")]
    {
        use gpuf_c::android_compat;
        
        let api_level = android_compat::get_android_api_level();
        let supports_posix = android_compat::supports_posix_madvise();
        let llama_available = android_compat::is_llama_available();
        
        println!("📱 Android API Level: {}", api_level);
        println!("✅ POSIX madvise 支持: {}", supports_posix);
        println!("🔧 Llama.cpp 可用: {}", llama_available);
        
        if !llama_available {
            return Err("Llama.cpp 不可用，请检查构建配置".into());
        }
    }
    
    // 2. 显示 llama.cpp 版本
    #[cfg(target_os = "android")]
    {
        let version = android_compat::get_llama_version();
        println!("📦 Llama.cpp 版本: {}", version);
    }
    
    // 3. 初始化引擎
    println!("🔧 正在初始化 LlamaEngine...");
    
    // 模型路径 - 在实际使用中，这应该是你的 GGUF 模型文件路径
    let model_path = "/data/local/tmp/model.gguf";
    
    // 如果模型文件不存在，创建一个模拟引擎
    let engine = if Path::new(model_path).exists() {
        println!("📁 找到模型文件: {}", model_path);
        LlamaEngine::new(model_path).await?
    } else {
        println!("⚠️  模型文件不存在，使用模拟模式");
        return simulate_usage();
    };
    
    // 4. 获取引擎信息
    let info = engine.get_info();
    println!("📊 引擎信息:");
    println!("  - API Level: {}", info.api_level);
    println!("  - MMap 支持: {}", info.supports_mmap);
    println!("  - POSIX madvise 支持: {}", info.supports_posix_madvise);
    println!("  - 模型已加载: {}", info.model_loaded);
    
    // 5. 生成文本示例
    println!("\n🎯 开始生成文本...");
    let prompt = "你好，请介绍一下人工智能";
    
    match engine.generate(prompt, 100).await {
        Ok(response) => {
            println!("✅ 生成成功:");
            println!("📝 {}", response);
        }
        Err(e) => {
            println!("❌ 生成失败: {}", e);
        }
    }
    
    println!("\n🎉 示例完成!");
    Ok(())
}

// 模拟使用函数（当没有真实模型时）
fn simulate_usage() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 模拟模式:");
    println!("  - 在实际使用中，请提供有效的 GGUF 模型文件");
    println!("  - 模型文件应该放置在应用可访问的目录中");
    println!("  - 推荐使用 Android 10+ (API 29+) 以获得最佳性能");
    
    Ok(())
}

// JNI 使用示例（在 Android 应用中）
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_pocketpal_LlamaExample_nativeTest(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
    model_path: jni::objects::JString,
) -> jni::sys::jstring {
    use jni::sys::{jstring, JNI_TRUE};
    
    // 获取模型路径
    let model_path_str = match env.get_string(model_path) {
        Ok(s) => s,
        Err(_) => {
            return env.new_string("Error: Invalid model path").unwrap().into_inner();
        }
    };
    
    // 在实际应用中，这里会创建并使用 LlamaEngine
    let result = format!("Model path received: {}", model_path_str.to_string_lossy());
    
    // 返回结果
    env.new_string(result).unwrap().into_inner()
}
