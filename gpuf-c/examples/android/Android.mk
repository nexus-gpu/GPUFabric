LOCAL_PATH := $(call my-dir)

include $(CLEAR_VARS)

LOCAL_MODULE    := android_test
LOCAL_SRC_FILES := ../android_test.c
LOCAL_LDLIBS    := -llog -landroid
LOCAL_SHARED_LIBRARIES := gpuf_c_sdk

include $(BUILD_EXECUTABLE)

$(call import-module,libgpuf_c_sdk)
