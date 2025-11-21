package com.example.gpuf;

import org.json.JSONObject;
import org.json.JSONException;

/**
 * GPUFabric Android Client SDK
 * 提供设备注册、监控和状态管理功能
 */
public class GPUFabricClientSDK {
    static {
        System.loadLibrary("gpuf_c");
    }
    
    // Core library functions
    public native int init();
    public native String getVersion();
    public native String getLastError();
    public native void freeString(String s);
    
    // Client SDK functions
    /**
     * 初始化GPUFabric客户端
     * @param configJson 客户端配置JSON字符串
     * @return 0=成功, -1=失败
     */
    public native int clientInit(String configJson);
    
    /**
     * 连接并注册到服务器
     * @return 0=成功, -1=失败
     */
    public native int clientConnect();
    
    /**
     * 获取客户端状态
     * @return 状态JSON字符串，失败返回null
     */
    public native String clientGetStatus();
    
    /**
     * 获取设备信息
     * @return 设备信息JSON字符串，失败返回null
     */
    public native String clientGetDeviceInfo();
    
    /**
     * 获取客户端指标
     * @return 指标JSON字符串，失败返回null
     */
    public native String clientGetMetrics();
    
    /**
     * 更新设备信息
     * @return 0=成功, -1=失败
     */
    public native int clientUpdateDeviceInfo();
    
    /**
     * 断开连接
     * @return 0=成功, -1=失败
     */
    public native int clientDisconnect();
    
    /**
     * 清理客户端资源
     * @return 0=成功, -1=失败
     */
    public native int clientCleanup();
    
    // 客户端配置类
    public static class ClientConfig {
        public String serverAddr = "127.0.0.1";
        public int controlPort = 17000;
        public int proxyPort = 17001;
        public String clientId;
        public String deviceName;
        public boolean autoRegister = true;
        public int heartbeatIntervalSecs = 30;
        public boolean enableMonitoring = true;
        
        public ClientConfig(String clientId) {
            this.clientId = clientId;
        }
        
        public String toJson() throws JSONException {
            JSONObject json = new JSONObject();
            json.put("server_addr", serverAddr);
            json.put("control_port", controlPort);
            json.put("proxy_port", proxyPort);
            json.put("client_id", clientId);
            if (deviceName != null) {
                json.put("device_name", deviceName);
            }
            json.put("auto_register", autoRegister);
            json.put("heartbeat_interval_secs", heartbeatIntervalSecs);
            json.put("enable_monitoring", enableMonitoring);
            return json.toString();
        }
    }
    
    // 设备信息类
    public static class DeviceInfo {
        public String deviceId;
        public String name;
        public String osType;
        public String cpuInfo;
        public int memoryGb;
        public String gpuInfo;
        public int totalTflops;
        public String lastSeen;
        public String status;
        
        public static DeviceInfo fromJson(String jsonStr) throws JSONException {
            JSONObject json = new JSONObject(jsonStr);
            DeviceInfo info = new DeviceInfo();
            info.deviceId = json.getString("device_id");
            info.name = json.getString("name");
            info.osType = json.getString("os_type");
            info.cpuInfo = json.getString("cpu_info");
            info.memoryGb = json.getInt("memory_gb");
            
            // GPU信息可能是数组
            if (json.has("gpu_info")) {
                info.gpuInfo = json.getJSONArray("gpu_info").toString();
            }
            
            info.totalTflops = json.getInt("total_tflops");
            info.lastSeen = json.getString("last_seen");
            info.status = json.getString("status");
            return info;
        }
        
        @Override
        public String toString() {
            return String.format(
                "DeviceInfo{id=%s, name=%s, os=%s, cpu=%s, memory=%dGB, tflops=%d, status=%s}",
                deviceId, name, osType, cpuInfo, memoryGb, totalTflops, status
            );
        }
    }
    
    // 客户端指标类
    public static class ClientMetrics {
        public long uptimeSeconds;
        public long totalRequests;
        public long successfulRequests;
        public long failedRequests;
        public double avgResponseTimeMs;
        public long networkBytesSent;
        public long networkBytesReceived;
        public String lastHeartbeat;
        
        public static ClientMetrics fromJson(String jsonStr) throws JSONException {
            JSONObject json = new JSONObject(jsonStr);
            ClientMetrics metrics = new ClientMetrics();
            metrics.uptimeSeconds = json.getLong("uptime_seconds");
            metrics.totalRequests = json.getLong("total_requests");
            metrics.successfulRequests = json.getLong("successful_requests");
            metrics.failedRequests = json.getLong("failed_requests");
            metrics.avgResponseTimeMs = json.getDouble("avg_response_time_ms");
            metrics.networkBytesSent = json.getLong("network_bytes_sent");
            metrics.networkBytesReceived = json.getLong("network_bytes_received");
            
            if (json.has("last_heartbeat") && !json.isNull("last_heartbeat")) {
                metrics.lastHeartbeat = json.getString("last_heartbeat");
            }
            
            return metrics;
        }
        
        @Override
        public String toString() {
            return String.format(
                "Metrics{uptime=%ds, requests=%d, success=%d, failed=%d, avgTime=%.2fms, sent=%dB, recv=%dB}",
                uptimeSeconds, totalRequests, successfulRequests, failedRequests, 
                avgResponseTimeMs, networkBytesSent, networkBytesReceived
            );
        }
    }
    
    // 高级API封装
    private boolean initialized = false;
    private ClientConfig config;
    
    /**
     * 初始化SDK
     */
    public boolean initialize(ClientConfig config) {
        if (initialized) {
            return true;
        }
        
        // 初始化底层库
        if (init() != 0) {
            System.err.println("Failed to initialize GPUFabric library: " + getLastError());
            return false;
        }
        
        // 初始化客户端
        try {
            String configJson = config.toJson();
            if (clientInit(configJson) != 0) {
                System.err.println("Failed to initialize client: " + getLastError());
                return false;
            }
        } catch (JSONException e) {
            System.err.println("Failed to serialize config: " + e.getMessage());
            return false;
        }
        
        this.config = config;
        this.initialized = true;
        return true;
    }
    
    /**
     * 连接到服务器
     */
    public boolean connect() {
        if (!initialized) {
            System.err.println("Client not initialized");
            return false;
        }
        
        if (clientConnect() != 0) {
            System.err.println("Failed to connect: " + getLastError());
            return false;
        }
        
        return true;
    }
    
    /**
     * 获取设备信息
     */
    public DeviceInfo getDeviceInfo() {
        if (!initialized) {
            return null;
        }
        
        String jsonStr = clientGetDeviceInfo();
        if (jsonStr == null) {
            System.err.println("Failed to get device info: " + getLastError());
            return null;
        }
        
        try {
            return DeviceInfo.fromJson(jsonStr);
        } catch (JSONException e) {
            System.err.println("Failed to parse device info: " + e.getMessage());
            return null;
        }
    }
    
    /**
     * 获取客户端指标
     */
    public ClientMetrics getMetrics() {
        if (!initialized) {
            return null;
        }
        
        String jsonStr = clientGetMetrics();
        if (jsonStr == null) {
            System.err.println("Failed to get metrics: " + getLastError());
            return null;
        }
        
        try {
            return ClientMetrics.fromJson(jsonStr);
        } catch (JSONException e) {
            System.err.println("Failed to parse metrics: " + e.getMessage());
            return null;
        }
    }
    
    /**
     * 获取连接状态
     */
    public String getStatus() {
        if (!initialized) {
            return "Not initialized";
        }
        
        String jsonStr = clientGetStatus();
        if (jsonStr == null) {
            return "Error: " + getLastError();
        }
        
        try {
            JSONObject json = new JSONObject(jsonStr);
            return json.getString("status");
        } catch (JSONException e) {
            return "Parse error: " + e.getMessage();
        }
    }
    
    /**
     * 更新设备信息
     */
    public boolean updateDeviceInfo() {
        if (!initialized) {
            return false;
        }
        
        if (clientUpdateDeviceInfo() != 0) {
            System.err.println("Failed to update device info: " + getLastError());
            return false;
        }
        
        return true;
    }
    
    /**
     * 断开连接
     */
    public boolean disconnect() {
        if (!initialized) {
            return false;
        }
        
        if (clientDisconnect() != 0) {
            System.err.println("Failed to disconnect: " + getLastError());
            return false;
        }
        
        return true;
    }
    
    /**
     * 清理资源
     */
    public boolean cleanup() {
        if (!initialized) {
            return true;
        }
        
        if (clientCleanup() != 0) {
            System.err.println("Failed to cleanup: " + getLastError());
            return false;
        }
        
        initialized = false;
        return true;
    }
    
    // ========== LLM 功能 ==========
    
    /**
     * 初始化LLM引擎
     * @param modelPath GGUF模型文件路径
     * @param contextSize 上下文大小 (如2048)
     * @param gpuLayers GPU层数 (0表示仅CPU)
     * @return 0成功，-1失败
     */
    public native int llmInit(String modelPath, int contextSize, int gpuLayers);
    
    /**
     * 使用初始化的模型生成文本
     * @param prompt 输入提示词
     * @param maxTokens 最大生成token数
     * @return 生成的文本，失败返回null
     */
    public native String llmGenerate(String prompt, int maxTokens);
    
    /**
     * 检查LLM引擎是否已初始化
     * @return 已初始化返回true
     */
    public native boolean llmIsInitialized();
    
    /**
     * 卸载LLM引擎并释放资源
     * @return 0成功，-1失败
     */
    public native int llmUnload();
    
    // LLM 便捷方法
    
    /**
     * 使用默认参数初始化模型
     * @param modelPath 模型路径
     * @return 成功返回true
     */
    public boolean initializeModel(String modelPath) {
        return initializeModel(modelPath, 2048, 0); // 默认：2048上下文，仅CPU
    }
    
    /**
     * 初始化模型
     * @param modelPath 模型路径
     * @param contextSize 上下文大小
     * @param gpuLayers GPU层数
     * @return 成功返回true
     */
    public boolean initializeModel(String modelPath, int contextSize, int gpuLayers) {
        int result = llmInit(modelPath, contextSize, gpuLayers);
        if (result != 0) {
            System.err.println("Failed to initialize model: " + getLastError());
            return false;
        }
        return true;
    }
    
    /**
     * 生成响应 (默认100 tokens)
     * @param prompt 提示词
     * @return 生成的文本
     */
    public String generateResponse(String prompt) {
        return generateResponse(prompt, 100); // 默认：100 tokens
    }
    
    /**
     * 生成响应
     * @param prompt 提示词
     * @param maxTokens 最大tokens
     * @return 生成的文本
     */
    public String generateResponse(String prompt, int maxTokens) {
        if (!llmIsInitialized()) {
            System.err.println("LLM engine not initialized");
            return null;
        }
        
        String response = llmGenerate(prompt, maxTokens);
        if (response == null) {
            System.err.println("Generation failed: " + getLastError());
        }
        return response;
    }
    
    /**
     * 关闭LLM引擎
     * @return 成功返回true
     */
    public boolean shutdownLLM() {
        int result = llmUnload();
        return result == 0;
    }
}
