import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.widget.TextView;
import com.example.gpuf.GPUFabricClientSDK;

public class GPUFabricClientExample extends Activity {
    private static final String TAG = "GPUFabricExample";
    
    private GPUFabricClientSDK client;
    private TextView statusText;
    private TextView deviceInfoText;
    private TextView metricsText;
    private Handler handler;
    private boolean isMonitoring = false;
    
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        
        // 创建简单的UI布局
        statusText = new TextView(this);
        deviceInfoText = new TextView(this);
        metricsText = new TextView(this);
        
        // 设置布局
        android.widget.LinearLayout layout = new android.widget.LinearLayout(this);
        layout.setOrientation(android.widget.LinearLayout.VERTICAL);
        layout.addView(statusText);
        layout.addView(deviceInfoText);
        layout.addView(metricsText);
        setContentView(layout);
        
        handler = new Handler(Looper.getMainLooper());
        
        // 初始化客户端
        initializeClient();
    }
    
    private void initializeClient() {
        new Thread(() -> {
            try {
                // 创建客户端配置
                GPUFabricClientSDK.ClientConfig config = 
                    new GPUFabricClientSDK.ClientConfig("android-device-" + System.currentTimeMillis());
                config.serverAddr = "your-server-address"; // 替换为实际服务器地址
                config.deviceName = android.os.Build.MODEL + " (" + android.os.Build.VERSION.RELEASE + ")";
                config.heartbeatIntervalSecs = 30;
                config.enableMonitoring = true;
                
                // 初始化SDK
                client = new GPUFabricClientSDK();
                
                runOnUiThread(() -> statusText.setText("Initializing GPUFabric SDK..."));
                
                if (client.initialize(config)) {
                    runOnUiThread(() -> statusText.setText("SDK initialized successfully"));
                    
                    // 获取设备信息
                    updateDeviceInfo();
                    
                    // 尝试连接服务器
                    if (client.connect()) {
                        runOnUiThread(() -> statusText.setText("Connected to server successfully"));
                        
                        // 开始监控
                        startMonitoring();
                    } else {
                        runOnUiThread(() -> statusText.setText("Failed to connect to server (running in standalone mode)"));
                        // 即使连接失败，也可以继续监控本地状态
                        startMonitoring();
                    }
                } else {
                    runOnUiThread(() -> statusText.setText("Failed to initialize SDK"));
                }
                
            } catch (Exception e) {
                Log.e(TAG, "Error initializing client", e);
                runOnUiThread(() -> statusText.setText("Error: " + e.getMessage()));
            }
        }).start();
    }
    
    private void updateDeviceInfo() {
        if (client == null) return;
        
        try {
            GPUFabricClientSDK.DeviceInfo deviceInfo = client.getDeviceInfo();
            if (deviceInfo != null) {
                runOnUiThread(() -> {
                    deviceInfoText.setText("Device Info:\n" + deviceInfo.toString());
                    Log.i(TAG, "Device info updated: " + deviceInfo.toString());
                });
            }
        } catch (Exception e) {
            Log.e(TAG, "Error updating device info", e);
        }
    }
    
    private void updateMetrics() {
        if (client == null) return;
        
        try {
            GPUFabricClientSDK.ClientMetrics metrics = client.getMetrics();
            if (metrics != null) {
                runOnUiThread(() -> {
                    metricsText.setText("Metrics:\n" + metrics.toString());
                    Log.d(TAG, "Metrics updated: " + metrics.toString());
                });
            }
        } catch (Exception e) {
            Log.e(TAG, "Error updating metrics", e);
        }
    }
    
    private void startMonitoring() {
        if (isMonitoring) return;
        
        isMonitoring = true;
        
        // 定期更新状态
        handler.postDelayed(new Runnable() {
            @Override
            public void run() {
                if (!isMonitoring) return;
                
                // 更新设备信息（每分钟）
                updateDeviceInfo();
                
                // 更新指标（每10秒）
                updateMetrics();
                
                // 更新连接状态
                String status = client.getStatus();
                runOnUiThread(() -> {
                    String currentText = statusText.getText().toString();
                    if (!currentText.contains(status)) {
                        statusText.setText("Status: " + status);
                    }
                });
                
                // 继续监控
                handler.postDelayed(this, 10000); // 10秒间隔
            }
        }, 1000); // 1秒后开始
    }
    
    private void stopMonitoring() {
        isMonitoring = false;
        handler.removeCallbacksAndMessages(null);
    }
    
    @Override
    protected void onDestroy() {
        super.onDestroy();
        
        // 清理资源
        new Thread(() -> {
            try {
                stopMonitoring();
                
                if (client != null) {
                    client.disconnect();
                    client.cleanup();
                    Log.i(TAG, "Client cleaned up successfully");
                }
            } catch (Exception e) {
                Log.e(TAG, "Error during cleanup", e);
            }
        }).start();
    }
    
    // 示例：执行任务的静态方法
    public static void executeTaskExample() {
        new Thread(() -> {
            GPUFabricClientSDK client = new GPUFabricClientSDK();
            
            try {
                // 初始化
                GPUFabricClientSDK.ClientConfig config = 
                    new GPUFabricClientSDK.ClientConfig("task-executor-device");
                client.initialize(config);
                
                // 连接
                if (client.connect()) {
                    Log.i(TAG, "Connected successfully, ready to execute tasks");
                    
                    // 这里可以添加任务执行逻辑
                    // 实际的任务执行需要通过服务器分配
                    
                    // 获取执行指标
                    GPUFabricClientSDK.ClientMetrics metrics = client.getMetrics();
                    if (metrics != null) {
                        Log.i(TAG, "Task execution metrics: " + metrics.toString());
                    }
                }
                
            } catch (Exception e) {
                Log.e(TAG, "Error in task execution example", e);
            } finally {
                client.cleanup();
            }
        }).start();
    }
}
