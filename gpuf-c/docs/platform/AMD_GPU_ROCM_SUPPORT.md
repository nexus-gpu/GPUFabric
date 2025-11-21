# AMD GPU ROCm Support Implementation

## Overview

This document describes the implementation of AMD GPU monitoring support using ROCm SMI (System Management Interface) in the GPUFabric project.

## Architecture

### Feature System
```toml
# ROCm feature for AMD GPU monitoring
rocm = ["rocm_smi_lib"]
```

### Priority Order
1. **NVML** (NVIDIA GPUs) - Most accurate
2. **ROCm SMI** (AMD GPUs on Linux) - Accurate for AMD
3. **WMI** (Windows) - Limited support
4. **PowerMetrics** (macOS) - Planned
5. **sysfs** (Linux) - Planned
6. **Fallback** - Conservative estimation

## Implementation Details

### Dependencies
- `rocm_smi_lib = { version = "0.1", optional = true }`
- Only available on Linux (`target_os = "linux"`)

### Code Structure
```rust
#[cfg(all(feature = "rocm", target_os = "linux"))]
fn try_rocm_metrics() -> Result<(u64, u64, u64, u64), Box<dyn std::error::Error>> {
    use rocm_smi_lib::*;
    
    // Initialize ROCm SMI
    let rocm = RocmSmi::new()?;
    
    // Get first AMD GPU
    if let Some(device) = rocm.devices().first() {
        // GPU utilization
        let utilization = device.get_utilization_rate()?;
        
        // Memory usage
        let memory_info = device.get_memory_info()?;
        let mem_usage_percent = (memory_info.used as f32 / memory_info.total as f32 * 100.0) as u64;
        
        // Power usage (microwatts -> watts)
        let power_usage = device.get_power_usage()? / 1000000;
        
        // Temperature
        let temp = device.get_temperature(TemperatureSensor::Gpu)?;
        
        return Ok((utilization as u64, mem_usage_percent, power_usage, temp as u64));
    }
    
    Err("No AMD GPU with ROCm support found".into())
}
```

## Usage

### Building with ROCm Support
```bash
cargo build --features "vulkan,rocm"
```

### Running Tests
```bash
cargo run --example test_rocm_gpu_metrics --features "vulkan,rocm"
```

### Requirements
1. **Linux OS** - ROCm is Linux-only
2. **AMD GPU** - RDNA2/RDNA3 or GCN architecture
3. **ROCm Drivers** - Install ROCm toolkit
4. **Permissions** - User needs access to GPU devices

## Installation

### Ubuntu/Debian
```bash
# Add AMD repository
wget https://repo.radeon.com/amdgpu-install/6.0.2/ubuntu/jammy/amdgpu-install_6.0.60200-1_all.deb
sudo apt install ./amdgpu-install_6.0.60200-1_all.deb

# Install ROCm
sudo amdgpu-install --usecase=rocm --no-dkms

# Add user to render group
sudo usermod -a -G render,video $LOGOUT
```

### RHEL/CentOS
```bash
# Install ROCm
sudo yum install -y rocm-dkms
```

## Testing

### Verify ROCm Installation
```bash
rocm-smi --showproductname
rocm-smi --showuse
rocm-smi --showtemp
```

### Test with GPUFabric
```rust
// Enable ROCm feature
#[cfg(feature = "rocm")]
use gpuf_c::util::system_info_vulkan::collect_device_info_vulkan_cross_platform;

// Collect metrics
let (device_info, device_count) = collect_device_info_vulkan_cross_platform().await?;
```

## Output Example

```
🎯 Using ROCm SMI for accurate GPU metrics
📊 Device Count: 1
🎮 GPU Usage: 45%
💾 GPU Memory Usage: 62%
🔌 GPU Power: 180W
🌡️ GPU Temperature: 72°C
⚡ Total TFLOPS: 16
💰 Total Memory: 16GB
```

## Limitations

1. **Linux Only** - ROCm is not available on Windows/macOS
2. **AMD GPUs Only** - Only works with AMD graphics cards
3. **Driver Dependencies** - Requires ROCm toolkit installation
4. **Root/User Access** - May need specific permissions

## Future Enhancements

1. **Multi-GPU Support** - Support for multiple AMD GPUs
2. **Windows Support** - Investigate ADL SDK for Windows
3. **Advanced Metrics** - Clock speeds, fan speeds, voltages
4. **Error Handling** - Better fallback for driver issues
5. **Performance** - Optimize for frequent polling

## Comparison with NVML

| Feature | NVML (NVIDIA) | ROCm SMI (AMD) |
|---------|---------------|----------------|
| Platform | Linux/Windows | Linux only |
| Accuracy | Excellent | Excellent |
| Metrics | Comprehensive | Good |
| Installation | CUDA Toolkit | ROCm Toolkit |
| Documentation | Extensive | Growing |

## Troubleshooting

### Common Issues

1. **"No AMD GPU with ROCm support found"**
   - Verify ROCm drivers are installed
   - Check GPU compatibility
   - Ensure user permissions

2. **"Permission denied"**
   - Add user to render/video groups
   - Check device permissions in /dev/dri/

3. **Compilation errors**
   - Ensure Linux target
   - Install ROCm development libraries

### Debug Commands
```bash
# Check ROCm status
rocm-smi -d

# Verify GPU detection
lspci | grep -i amd

# Check kernel modules
lsmod | grep amdgpu
```

## Conclusion

The ROCm SMI integration provides accurate AMD GPU monitoring capabilities for Linux systems, complementing the existing NVML support for NVIDIA GPUs and bringing GPUFabric closer to comprehensive cross-platform GPU monitoring.
