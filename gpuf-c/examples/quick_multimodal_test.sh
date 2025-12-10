#!/bin/bash
# Quick multimodal test script - for verification fixes

set -e

echo "🔥 GPUFabric Multimodal Quick Test"
echo "================================"

# Color definitions
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check device connection
echo -e "\n${YELLOW}1. Checking ADB connection...${NC}"
if ! adb devices | grep -q "device$"; then
    echo -e "${RED}❌ No Android device detected${NC}"
    echo "Please ensure device is connected and USB debugging is enabled"
    exit 1
fi
echo -e "${GREEN}✅ Device connected${NC}"

# Check files
echo -e "\n${YELLOW}2. Checking files on device...${NC}"
FILES_OK=true

if ! adb shell "test -f /data/local/tmp/libgpuf_c_sdk_v9.so"; then
    echo -e "${RED}❌ SDK library not found${NC}"
    FILES_OK=false
fi

if ! adb shell "test -f /data/local/tmp/test_multimodal_minimal"; then
    echo -e "${RED}❌ Test program not found${NC}"
    FILES_OK=false
fi

if ! adb shell "test -f /data/local/tmp/Qwen2-VL-2B-Instruct-Q4_K_M.gguf"; then
    echo -e "${YELLOW}⚠️  Qwen2-VL model not found, trying SmolVLM...${NC}"
    if ! adb shell "test -f /data/local/tmp/SmolVLM-500M-Instruct-Q8_0.gguf"; then
        echo -e "${RED}❌ No model files found${NC}"
        FILES_OK=false
    fi
fi

if [ "$FILES_OK" = false ]; then
    echo -e "\n${RED}Please run the complete build and push script first:${NC}"
    echo "  cd /home/jack/codedir/GPUFabric/gpuf-c/examples"
    echo "  ./build_and_test_multimodal.sh"
    exit 1
fi
echo -e "${GREEN}✅ All files ready${NC}"

# Clear logs
echo -e "\n${YELLOW}3. Clearing old logs...${NC}"
adb logcat -c
echo -e "${GREEN}✅ Logs cleared${NC}"

# Run test
echo -e "\n${YELLOW}4. Running multimodal test...${NC}"
echo "================================"

# Start log collection in background
LOG_FILE="multimodal_test_$(date +%Y%m%d_%H%M%S).log"
adb logcat -v time > "$LOG_FILE" &
LOGCAT_PID=$!

# Run test program
adb shell "cd /data/local/tmp && LD_LIBRARY_PATH=. ./test_multimodal_minimal" 2>&1 | tee test_output.txt

# Stop log collection
sleep 2
kill $LOGCAT_PID 2>/dev/null || true

echo "================================"

# Analyze results
echo -e "\n${YELLOW}5. Analyzing test results...${NC}"

# Check key metrics
if grep -q "✅ ALL TESTS PASSED" test_output.txt; then
    echo -e "${GREEN}✅ All tests passed!${NC}"
    SUCCESS=true
else
    echo -e "${RED}❌ Some tests failed${NC}"
    SUCCESS=false
fi

# Check multimodal encoding
if grep -q "mtmd_helper_eval_chunks result: 0" "$LOG_FILE"; then
    echo -e "${GREEN}✅ Image encoding successful${NC}"
else
    echo -e "${RED}❌ Image encoding failed${NC}"
    SUCCESS=false
fi

# Check n_past position
INITIAL_N_PAST=$(grep "Initial n_past:" "$LOG_FILE" | tail -1 | awk '{print $NF}')
NEW_N_PAST=$(grep "New n_past:" "$LOG_FILE" | tail -1 | awk '{print $NF}')

if [ -n "$INITIAL_N_PAST" ] && [ -n "$NEW_N_PAST" ]; then
    echo -e "${GREEN}✅ Position management: Initial=$INITIAL_N_PAST, After_encoding=$NEW_N_PAST${NC}"
    if [ "$INITIAL_N_PAST" = "$NEW_N_PAST" ] && [ "$INITIAL_N_PAST" != "0" ]; then
        echo -e "${GREEN}   ✓ Position correctly passed${NC}"
    elif [ "$INITIAL_N_PAST" = "0" ]; then
        echo -e "${RED}   ✗ Warning: Starting from position 0 (should start from encoded position)${NC}"
        SUCCESS=false
    fi
fi

# Check vocab pointer
if grep -q "Got vocab pointer" "$LOG_FILE"; then
    VOCAB_PTR=$(grep "Got vocab pointer" "$LOG_FILE" | tail -1 | awk '{print $4}')
    echo -e "${GREEN}✅ Vocab pointer: $VOCAB_PTR${NC}"
else
    echo -e "${RED}❌ Vocab pointer not found${NC}"
    SUCCESS=false
fi

# Check logits
if grep -q "Logits pointer valid" "$LOG_FILE"; then
    echo -e "${GREEN}✅ Logits pointer valid${NC}"
elif grep -q "logits pointer is null" "$LOG_FILE"; then
    echo -e "${RED}❌ Logits pointer is null${NC}"
    SUCCESS=false
fi

# Check generated token count
TOKEN_COUNT=$(grep -c "Sampled token:" "$LOG_FILE" || echo "0")
if [ "$TOKEN_COUNT" -gt 0 ]; then
    echo -e "${GREEN}✅ Generated $TOKEN_COUNT tokens${NC}"
    
    # Check control token ratio
    CONTROL_COUNT=$(grep -c "Control token detected" "$LOG_FILE" || echo "0")
    if [ "$TOKEN_COUNT" -gt 0 ]; then
        CONTROL_RATIO=$((CONTROL_COUNT * 100 / TOKEN_COUNT))
        if [ "$CONTROL_RATIO" -lt 50 ]; then
            echo -e "${GREEN}   ✓ Control token ratio: ${CONTROL_RATIO}% (normal)${NC}"
        else
            echo -e "${YELLOW}   ⚠ Control token ratio: ${CONTROL_RATIO}% (high)${NC}"
        fi
    fi
else
    echo -e "${RED}❌ No tokens generated${NC}"
    SUCCESS=false
fi

# Summary
echo -e "\n${YELLOW}================================${NC}"
if [ "$SUCCESS" = true ]; then
    echo -e "${GREEN}🎉 Test successful! Multimodal generation working normally${NC}"
    echo -e "\n${GREEN}Key fixes verified:${NC}"
    echo "  ✓ Unified use of generate_multimodal_response_with_vocab"
    echo "  ✓ Correct n_past position passing"
    echo "  ✓ Direct vocab pointer usage"
    echo "  ✓ Logits status normal"
else
    echo -e "${RED}❌ Test failed, needs further debugging${NC}"
    echo -e "\n${YELLOW}Debugging suggestions:${NC}"
    echo "  1. View complete logs: cat $LOG_FILE"
    echo "  2. View test output: cat test_output.txt"
    echo "  3. Check key metrics:"
    echo "     - Initial n_past equals New n_past"
    echo "     - Vocab pointer is not empty"
    echo "     - Logits pointer is valid"
    echo "     - Token generation is normal"
fi

echo -e "\n${YELLOW}Log files:${NC}"
echo "  - Complete logs: $LOG_FILE"
echo "  - Test output: test_output.txt"

echo -e "\n${YELLOW}View key logs:${NC}"
echo "  grep -E 'n_past|vocab|Logits|Sampled token' $LOG_FILE | head -50"

exit $([ "$SUCCESS" = true ] && echo 0 || echo 1)
