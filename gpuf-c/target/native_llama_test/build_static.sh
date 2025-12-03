#!/bin/bash

echo "🔧 方案1: 静态链接编译"

# 编译参数 - 强制静态链接
CFLAGS="-shared -fPIC -O2 -DANDROID"
CXXFLAGS="-shared -fPIC -O2 -DANDROID -static-libstdc++ -static-libgcc"
LDFLAGS="-static-libstdc++ -static-libgcc -lm -ldl -llog"

# 尝试编译一个简单的 C++ 测试
cat > test_static.cpp << 'CPP'
#include <iostream>
#include <string>
#include <mutex>

extern "C" {
    int test_cpp_function() {
        std::mutex mtx;
        std::string msg = "C++ static linking test";
        std::cout << msg << std::endl;
        return 42;
    }
    
    const char* get_cpp_version() {
        return "C++ Static Linked v1.0";
    }
}
