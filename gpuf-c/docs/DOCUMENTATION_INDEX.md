# GPUFabric Documentation Index

## 📚 Overview

This directory contains all technical documentation for the GPUFabric project.

## 📁 Document Categories

### 🎯 Platform Support & GPU Monitoring

#### [platform/AMD_GPU_ROCM_SUPPORT.md](./platform/AMD_GPU_ROCM_SUPPORT.md)
- **Content**: AMD GPU monitoring using ROCm SMI
- **Topics**: Architecture, implementation details, priority order
- **Relevance**: High - Core GPU support documentation

#### [platform/GPU_METRICS_IMPROVEMENT.md](./platform/GPU_METRICS_IMPROVEMENT.md)
- **Content**: GPU metrics accuracy improvements
- **Topics**: Power usage, memory usage, temperature monitoring
- **Relevance**: High - Performance optimization records

### 📱 Mobile Development

#### [mobile/ANDROID_DEVELOPMENT_GUIDE.md](./mobile/ANDROID_DEVELOPMENT_GUIDE.md)
- **Content**: Complete Android development guide (merged)
- **Topics**: Integration, Vulkan enhancement, SDK usage, build guide
- **Relevance**: High - Mobile platform comprehensive guide

#### [mobile/MOBILE_SDK_CHECKLIST.md](./mobile/MOBILE_SDK_CHECKLIST.md)
- **Content**: Mobile SDK implementation checklist
- **Topics**: Android/iOS support, project structure, phases
- **Relevance**: Medium - Mobile development planning

### 📖 API & Reference

#### [api/API_REFERENCE.md](./api/API_REFERENCE.md)
- **Content**: Complete API reference documentation
- **Topics**: Function signatures, data structures, usage examples
- **Relevance**: High - Developer reference

#### [BUILD_GUIDE.md](./BUILD_GUIDE.md)
- **Content**: Build instructions for all platforms
- **Topics**: Compilation, dependencies, troubleshooting
- **Relevance**: High - Setup and deployment

### 📁 Platform Guides

#### [PLATFORM_GUIDES/](./PLATFORM_GUIDES/)
- Platform-specific implementation guides

#### [README.md](./README.md)
- Main project documentation and overview

## 🔍 Quick Reference

| Topic | Document | Priority |
|-------|----------|----------|
| AMD GPU Support | [platform/AMD_GPU_ROCM_SUPPORT.md](./platform/AMD_GPU_ROCM_SUPPORT.md) | 🔴 High |
| GPU Metrics | [platform/GPU_METRICS_IMPROVEMENT.md](./platform/GPU_METRICS_IMPROVEMENT.md) | 🔴 High |
| Android Development | [mobile/ANDROID_DEVELOPMENT_GUIDE.md](./mobile/ANDROID_DEVELOPMENT_GUIDE.md) | 🔴 High |
| API Reference | [api/API_REFERENCE.md](./api/API_REFERENCE.md) | 🔴 High |
| Build Instructions | [BUILD_GUIDE.md](./BUILD_GUIDE.md) | 🟡 Medium |
| Mobile SDK Checklist | [mobile/MOBILE_SDK_CHECKLIST.md](./mobile/MOBILE_SDK_CHECKLIST.md) | 🟡 Medium |

## 📂 Directory Structure

```
docs/
├── 📋 DOCUMENTATION_INDEX.md          # This file
├── 📖 README.md                        # Main project docs
├── 🔨 BUILD_GUIDE.md                   # Build instructions
├── 🎯 platform/                        # Platform-specific docs
│   ├── AMD_GPU_ROCM_SUPPORT.md         # AMD GPU support
│   └── GPU_METRICS_IMPROVEMENT.md      # GPU metrics improvements
├── 📱 mobile/                          # Mobile development
│   ├── ANDROID_DEVELOPMENT_GUIDE.md    # Complete Android guide
│   └── MOBILE_SDK_CHECKLIST.md         # Mobile SDK checklist
├── 📚 api/                            # API documentation
│   └── API_REFERENCE.md                # Complete API reference
└── 📁 PLATFORM_GUIDES/                # Additional platform guides
```

## 📝 Documentation Standards

### File Naming Convention
- Use `UPPER_CASE_WITH_UNDERSCORES.md`
- Be descriptive and clear about content
- Group related documents in subdirectories

### Content Guidelines
- Include overview/problem statement
- Provide implementation details
- Add code examples where relevant
- Keep technical accuracy current
- Use consistent formatting and structure

### Maintenance
- Review and update outdated content quarterly
- Remove obsolete documents immediately
- Add new documentation for major features
- Keep this index updated
- Ensure all links are working

## 🗂️ Recent Changes

### 2025-11-21 Major Reorganization
- ✅ **Merged Android Documentation**: Combined `ANDROID_INTEGRATION_GUIDE.md` and `ANDROID_VULKAN_ENHANCEMENT.md` into comprehensive `ANDROID_DEVELOPMENT_GUIDE.md`
- ✅ **Created Subdirectories**: Organized documents into `platform/`, `mobile/`, and `api/` categories
- ✅ **Updated Index**: Restructured documentation index for better navigation
- ✅ **Removed Duplicates**: Eliminated redundant and outdated documents

### Archive History
Previously removed obsolete documents:
- ~~DEVICE_INFO_CACHE_SOLUTION.md~~ - Caching solution deprecated (now using real-time)
- ~~BUILD_SUCCESS_SUMMARY.md~~ - Temporary build issue resolved
- ~~ANDROID_INTEGRATION_GUIDE.md~~ - Merged into comprehensive guide
- ~~ANDROID_VULKAN_ENHANCEMENT.md~~ - Merged into comprehensive guide

## 🚀 Getting Started

1. **New to GPUFabric?** Start with [README.md](./README.md)
2. **Setting up development?** Follow [BUILD_GUIDE.md](./BUILD_GUIDE.md)
3. **Mobile development?** See [mobile/ANDROID_DEVELOPMENT_GUIDE.md](./mobile/ANDROID_DEVELOPMENT_GUIDE.md)
4. **GPU support issues?** Check [platform/AMD_GPU_ROCM_SUPPORT.md](./platform/AMD_GPU_ROCM_SUPPORT.md)
5. **API questions?** Reference [api/API_REFERENCE.md](./api/API_REFERENCE.md)

## 🤝 Contributing to Documentation

When adding or updating documentation:

1. **Choose the right category** - platform/, mobile/, api/, or root level
2. **Follow naming conventions** - Use `UPPER_CASE_WITH_UNDERSCORES.md`
3. **Update this index** - Add new documents to the appropriate section
4. **Test links** - Ensure all internal links work correctly
5. **Review consistency** - Match formatting with existing docs

---

*Last updated: 2025-11-21*
*For questions or contributions, see the main project README or create an issue*
