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
