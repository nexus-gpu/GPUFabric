// ============================================================================
// Android Remote Worker C API Test Program
// ============================================================================
//
// This program tests the C abstraction layer for GPUFabric Remote Worker
// Management functions on Android devices, including the new hot swapping
// model loading functionality. Compile with Android NDK.
//
// Compilation:
// ndk-build NDK_PROJECT_PATH=. APP_PLATFORM=android-21
// ============================================================================

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// Include the minimal C API header file (no JNI dependencies)
#include "gpuf_c_minimal.h"

// Model paths for testing (adjust these paths for your device)
#define MODEL_PATH_1 "/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf"
#define MODEL_PATH_2 "/data/local/tmp/models/llama-3.2-1b-instruct-q8_0.gguf"

int main() {
    printf("🔥 GPUFabric Android C API Test (with Hot Swapping)\n");
    printf("==================================================\n");
    
    // Test 1: Set remote worker model (new function)
    printf("\n🤖 Test 1: Loading initial model...\n");
    int result = set_remote_worker_model(MODEL_PATH_1);
    
    if (result == 0) {
        printf("✅ Model loaded successfully: %s\n", MODEL_PATH_1);
    } else {
        printf("❌ Failed to load model (error: %d)\n", result);
        printf("   Trying alternative approach...\n");
        return -1;
    }
    
    // Wait a bit for model initialization
    printf("⏳ Waiting for model initialization...\n");
    sleep(2);
    
    // Test 2: Start remote worker
    printf("\n📡 Test 2: Starting remote worker...\n");
    result = start_remote_worker(
        "8.140.251.142",  // server_addr (remote server in China)
        17000,        // control_port
        17001,        // proxy_port
        "TCP",        // worker_type
        "50ef7b5e7b5b4c79991087bb9f62cef1"  // client_id (32 hex chars)
    );
    
    if (result == 0) {
        printf("✅ Remote worker started successfully\n");
    } else {
        printf("❌ Failed to start remote worker (error: %d)\n", result);
        printf("   Continuing with other tests...\n");
        return -1;
    }
    
    // Wait a bit for initialization
    printf("⏳ Waiting for worker initialization...\n");
    sleep(3);
    
    // Test 3: Start background tasks
    printf("\n🚀 Test 3: Starting background tasks...\n");
    result = start_remote_worker_tasks();
    
    if (result == 0) {
        printf("✅ Background tasks started successfully\n");
    } else {
        printf("❌ Failed to start background tasks (error: %d)\n", result);
        return -1;
    }
    
    // Wait a bit for tasks to start
    printf("⏳ Waiting for task initialization...\n");
    sleep(2);
    
    // Test 4: Get worker status
    printf("\n📊 Test 4: Getting worker status...\n");
    char status_buffer[1024];
    result = get_remote_worker_status(status_buffer, sizeof(status_buffer));
    
    if (result == 0) {
        printf("✅ Worker status: %s\n", status_buffer);
    } else {
        printf("❌ Failed to get worker status (error: %d)\n", result);
        return -1;
    }
    
    // Test 5: Hot swapping models (new feature)
    printf("\n🔄 Test 5: Testing hot model swapping...\n");
    
    printf("   Loading second model...\n");
    result = set_remote_worker_model(MODEL_PATH_2);
    if (result == 0) {
        printf("   ✅ Hot swap to model 2 successful\n");
    } else {
        printf("   ⚠️  Hot swap test failed (error: %d) - expected for dummy paths\n", result);
        return -1;
    }
    
    // Test 6: Monitor status after hot swapping
    printf("\n🔍 Test 6: Monitoring status for 10 seconds...\n");
    for (int i = 0; i < 10; i++) {
        sleep(1);
        result = get_remote_worker_status(status_buffer, sizeof(status_buffer));
        if (result == 0) {
            printf("[%d/10] Status: %s\n", i + 1, status_buffer);
        } else {
            printf("[%d/10] ❌ Failed to get status\n", i + 1);
        }
    }
    
    // Test 7: Continuous monitoring for inference requests
    printf("\n� Test 7: Continuous monitoring for remote inference requests...\n");
    printf("📡 Android device is now ready to receive inference tasks!\n");
    printf("🌐 Send requests to: http://8.140.251.142:8081/v1/completions\n");
    printf("⏱️  Monitoring for 1 hour (3600 seconds)...\n");
    printf("📊 Status updates every 30 seconds:\n\n");
    
    // Monitor for 1 hour with status updates every 30 seconds
    for (int i = 0; i < 120; i++) { // 120 * 30 = 3600 seconds = 1 hour
        sleep(30);
        
        result = get_remote_worker_status(status_buffer, sizeof(status_buffer));
        if (result == 0) {
            printf("[%d/120] 🟢 Status: %s\n", i + 1, status_buffer);
        } else {
            printf("[%d/120] 🔴 Failed to get status (error: %d)\n", i + 1, result);
        }
        
        // Exit early if status indicates problems
        if (strstr(status_buffer, "stopped") != NULL || 
            strstr(status_buffer, "error") != NULL ||
            strstr(status_buffer, "disconnected") != NULL) {
            printf("❌ Device status indicates problems, exiting early\n");
            break;
        }
    }
    
    printf("\n� Test 8: Stopping remote worker after monitoring period...\n");
    result = stop_remote_worker();
    
    if (result == 0) {
        printf("✅ Remote worker stopped successfully\n");
    } else {
        printf("❌ Failed to stop remote worker (error: %d)\n", result);
        return -1;
    }
    
    printf("\n🎉 GPUFabric C API Test completed!\n");
    printf("✅ Device monitored for 1 hour and is now stopping\n");
    return 0;
}

// Error handling helper
void handle_error(const char* operation, int error_code) {
    printf("❌ Error in %s: code %d\n", operation, error_code);
    
    // Get detailed status if possible
    char buffer[512];
    if (get_remote_worker_status(buffer, sizeof(buffer)) == 0) {
        printf("   Status: %s\n", buffer);
    }
}

// Test with invalid parameters
void test_error_handling() {
    printf("\n🧪 Testing error handling...\n");
    
    // Test null server address
    int result = start_remote_worker(NULL, 17000, 17001, "TCP", "1234567890abcdef1234567890abcdef");
    handle_error("null server address", result);
    
    // Test invalid worker type
    result = start_remote_worker("127.0.0.1", 17000, 17001, "INVALID", "1234567890abcdef1234567890abcdef");
    handle_error("invalid worker type", result);
    
    // Test null buffer for status
    result = get_remote_worker_status(NULL, 1024);
    handle_error("null status buffer", result);
    
    // Test zero buffer size
    char buffer[100];
    result = get_remote_worker_status(buffer, 0);
    handle_error("zero buffer size", result);
}
