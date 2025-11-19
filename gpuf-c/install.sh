#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# init log file
LOG_FILE="/tmp/ollama_install_$(date +%Y%m%d_%H%M%S).log"
echo "Installation started at $(date)" > "$LOG_FILE"

# log function
log() {
    echo -e "$1"
    echo -e "[$(date +'%Y-%m-%d %H:%M:%S')] $1" >> "$LOG_FILE"
}

# check command exists
check_command() {
    if ! command -v "$1" &> /dev/null; then
        log "${RED}error: need $1 but not installed${NC}"
        exit 1
    fi
}

# detect os and architecture
detect_system() {
    echo "=== system detect ==="
    
    # detect os
    if [[ "$(uname)" == "Darwin" ]]; then
        OS="darwin"
        ARCH="$(uname -m)"
        echo "OS: macOS ($ARCH)"
        
        # get Mac chip info
        if [[ "$ARCH" == "arm64" ]]; then
            CHIP_MODEL=$(sysctl -n machdep.cpu.brand_string)
            echo "Chip model: $CHIP_MODEL"
            
            # get GPU info
            echo "GPU info:"
            system_profiler SPDisplaysDataType | grep -A 5 "Chipset Model" || echo "  未检测到独立GPU"
            
            # get GPU count
            GPU_COUNT=$(system_profiler SPDisplaysDataType | grep -c "Chipset Model" || echo "0")
            echo "GPU count: $GPU_COUNT"
        else
            echo "Chip architecture: Intel"
            # for Intel Mac, also initialize GPU_COUNT
            GPU_COUNT=0
        fi
    else
        OS="linux"
        ARCH="$(uname -m)"
        echo "OS: Linux ($ARCH)"
        
        # only detect NVIDIA/AMD GPU on Linux
        if command -v nvidia-smi &> /dev/null; then
            echo "detect NVIDIA GPU"
            GPU_COUNT=$(nvidia-smi --query-gpu=count --format=csv,noheader | head -n 1)
            echo "GPU count: $GPU_COUNT"
        elif command -v rocm-smi &> /dev/null; then
            echo "detect AMD GPU (ROCm)"
            GPU_COUNT=$(rocm-smi --showproductname | wc -l)
            echo "GPU count: $GPU_COUNT"
        else
            echo "not detect NVIDIA/AMD GPU or driver not installed"
            GPU_COUNT=0
        fi
    fi

    # set environment variables
    export OS
    export ARCH
    export GPU_COUNT
    # set NUM_GPUS to GPU_COUNT, ensure subsequent use will not report error
    export NUM_GPUS=$GPU_COUNT
}
setup_environment() {
    log "${YELLOW}setting environment variables...${NC}"
    
    case "$OS" in
        linux*)
            # Linux environment variables
            if ! grep -q "OLLAMA_HOST" /etc/environment 2>/dev/null; then
                echo "OLLAMA_HOST=0.0.0.0" | sudo tee -a /etc/environment
            fi
            if [ -n "$NUM_GPUS" ] && [ "$NUM_GPUS" -gt 0 ] && ! grep -q "OLLAMA_NUM_GPU" /etc/environment 2>/dev/null; then
                echo "OLLAMA_NUM_GPU=$NUM_GPUS" | sudo tee -a /etc/environment
            fi
            ;;
        darwin*)
            # macOS environment variables
            local shell_config=""
            if [ -n "$BASH_VERSION" ]; then
                shell_config="$HOME/.bash_profile"
            elif [ -n "$ZSH_VERSION" ]; then
                shell_config="$HOME/.zshrc"
            else
                shell_config="$HOME/.bash_profile"
            fi
            
            # check if already set
            if ! grep -q "OLLAMA_HOST" "$shell_config" 2>/dev/null; then
                echo "export OLLAMA_HOST=0.0.0.0" >> "$shell_config"
                log "${GREEN}added OLLAMA_HOST to $shell_config${NC}"
            fi
            
            if [ -n "$NUM_GPUS" ] && [ "$NUM_GPUS" -gt 0 ]; then
                if ! grep -q "OLLAMA_NUM_GPU" "$shell_config" 2>/dev/null; then
                    echo "export OLLAMA_NUM_GPU=$NUM_GPUS" >> "$shell_config"
                    log "${GREEN}added OLLAMA_NUM_GPU to $shell_config${NC}"
                fi
            fi
            ;;
        cygwin*|mingw*|msys*|nt|win*)
            # Windows environment variables are set in PowerShell script
            ;;
    esac
    
    # set current session environment variables
    export OLLAMA_HOST=0.0.0.0
    if [ -n "$NUM_GPUS" ] && [ "$NUM_GPUS" -gt 0 ]; then
        export OLLAMA_NUM_GPU=$NUM_GPUS
    fi
}

# Installation directory
get_install_dir() {
    if [ "$OS" = "darwin" ]; then
        echo "/usr/local/bin"
    elif [ "$OS" = "linux" ]; then
        echo "/usr/local/bin"
    else
        echo "/usr/local/bin"
    fi
}

# use official script install Ollama
install_ollama_official() {
    log "${YELLOW}use official script install Ollama...${NC}"
    
    case "$OS" in
    
        darwin*)
            # Linux and macOS use official install script
            log "${YELLOW}download and execute official install script...${NC}"
            if curl -fsSL https://ollama.com/install.sh | sh >> "$LOG_FILE" 2>&1; then
                log "${GREEN}Ollama official script install success!${NC}"
                return 0
            else
                log "${RED}official script install failed, try fallback method...${NC}"
                return 1
            fi
            ;;
        *)
            log "${RED}unsupported OS: $OS${NC}"
            return 1
            ;;
    esac
}


install_docker_official() {
    log "${YELLOW}install docker...${NC}"
    
    # check if docker already installed
    if command -v docker &> /dev/null; then
        log "${YELLOW}docker already installed${NC}"
    else
        # use official script install docker
        log "${YELLOW}downloading and executing docker official install script...${NC}"
        if ! curl -fsSL https://get.docker.com | sh >> "$LOG_FILE" 2>&1; then
            log "${RED}docker install failed${NC}"
            return 1
        fi
    fi

    # add current user to docker group
    if ! id -nG "$USER" | grep -qw docker; then
        log "${YELLOW}add $USER to docker group...${NC}"
        sudo usermod -aG docker "$USER" >> "$LOG_FILE" 2>&1
    fi
    
    # start and enable docker service
    if ! systemctl is-active --quiet docker; then
        log "${YELLOW}start docker service...${NC}"
        sudo systemctl start docker >> "$LOG_FILE" 2>&1
        sudo systemctl enable docker >> "$LOG_FILE" 2>&1
    fi
    
    # check and install NVIDIA Container Toolkit
    if [ "$NUM_GPUS" -gt 0 ] && [ "$OS" = "linux" ]; then
        log "${YELLOW}detect GPU, install NVIDIA Container Toolkit...${NC}"
        
        # Check if nvidia-container-toolkit is already installed
        if dpkg -l | grep -q nvidia-container-toolkit; then
            log "${YELLOW}NVIDIA Container Toolkit already installed, skipping installation...${NC}"
        else
            # Backup existing sources file if it exists
            if [ -f "/etc/apt/sources.list.d/nvidia-container-toolkit.list" ]; then
                sudo cp /etc/apt/sources.list.d/nvidia-container-toolkit.list /etc/apt/sources.list.d/nvidia-container-toolkit.list.backup
                log "${YELLOW}Backing up existing nvidia-container-toolkit.list...${NC}"
            fi
            
            # add NVIDIA Container Toolkit repository
            distribution=$(. /etc/os-release;echo $ID$VERSION_ID) \
            && curl -s -L https://nvidia.github.io/libnvidia-container/gpgkey | sudo gpg --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg \
            && curl -s -L https://nvidia.github.io/libnvidia-container/$distribution/libnvidia-container.list | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list
            
            # Check if the repository file was created correctly
            if [ ! -s "/etc/apt/sources.list.d/nvidia-container-toolkit.list" ] || grep -q "<!doctype" "/etc/apt/sources.list.d/nvidia-container-toolkit.list"; then
                log "${YELLOW}Distribution-specific repository not found, using generic Ubuntu repository...${NC}"
                # Remove the corrupted file
                sudo rm -f /etc/apt/sources.list.d/nvidia-container-toolkit.list
                # Use generic Ubuntu repository as fallback with new keyring format
                echo "deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://nvidia.github.io/libnvidia-container/stable/deb/ /" | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list
            fi
            
            # update package list and install nvidia-container-toolkit
            sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
        fi
        
        # configure Docker to use nvidia runtime
        sudo nvidia-ctk runtime configure --runtime=docker --set-as-default
        
        # restart Docker service
        sudo systemctl restart docker
        
        # verify installation
        if docker run --rm --gpus all nvidia/cuda:11.0-base nvidia-smi >/dev/null 2>&1; then
            log "${GREEN}NVIDIA Container Toolkit install success!${NC}"
        else
            log "${YELLOW}NVIDIA Container Toolkit install may have problems, please check log${NC}"
        fi
    fi
    
    log "${GREEN}Docker install and configure completed!${NC}"
    return 0
}

install_docker_fallback() {
    log "${YELLOW}try fallback method install Docker...${NC}"
    
    case "$OS" in
        linux*)
            # check system distribution
            if [ -f /etc/os-release ]; then
                . /etc/os-release
                case $ID in
                    debian|ubuntu)
                        log "${YELLOW}detect Debian/Ubuntu system, use apt install Docker...${NC}"
                        sudo apt-get update >> "$LOG_FILE" 2>&1
                        sudo apt-get install -y \
                            apt-transport-https \
                            ca-certificates \
                            curl \
                            gnupg \
                            lsb-release >> "$LOG_FILE" 2>&1
                        
                        # add Docker official GPG key
                        curl -fsSL https://download.docker.com/linux/$ID/gpg | sudo gpg --dearmor -o /usr/share/keyrings/docker-archive-keyring.gpg
                        
                        # set stable repository
                        echo \
                          "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker-archive-keyring.gpg] https://download.docker.com/linux/$ID \
                          $(lsb_release -cs) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
                        
                        sudo apt-get update >> "$LOG_FILE" 2>&1
                        sudo apt-get install -y docker-ce docker-ce-cli containerd.io >> "$LOG_FILE" 2>&1
                        ;;
                        
                    centos|rhel|fedora)
                        log "${YELLOW}detect RHEL/CentOS/Fedora system, use yum install Docker...${NC}"
                        sudo yum install -y yum-utils >> "$LOG_FILE" 2>&1
                        sudo yum-config-manager \
                            --add-repo \
                            https://download.docker.com/linux/centos/docker-ce.repo >> "$LOG_FILE" 2>&1
                        sudo yum install -y docker-ce docker-ce-cli containerd.io >> "$LOG_FILE" 2>&1
                        ;;
                        
                    *)
                        log "${RED}unsupported Linux distribution: $ID${NC}"
                        return 1
                        ;;
                esac
                
                # start and enable Docker service
                sudo systemctl start docker >> "$LOG_FILE" 2>&1
                sudo systemctl enable docker >> "$LOG_FILE" 2>&1
                
                # add user to docker group
                if ! id -nG "$USER" | grep -qw docker; then
                    sudo usermod -aG docker "$USER" >> "$LOG_FILE" 2>&1
                fi
                
                log "${GREEN}Docker installed successfully! Please re-login or reboot to make changes take effect${NC}"
                return 0
            else
                log "${RED}failed to determine Linux distribution${NC}"
                return 1
            fi
            ;;
            
        *)
            log "${RED}unsupported OS: $OS${NC}"
            return 1
            ;;
    esac
}

install_ollama_fallback() {
    
    case "$OS" in
        linux*)
            install_linux_fallback
            ;;
        darwin*)
            install_macos_fallback
            ;;
        *)
            return 1
            ;;
    esac
}

# Linux fallback install method
install_linux_fallback() {
    log "${YELLOW}in Linux ($ARCH) using fallback method to install Ollama...${NC}"
    
    # install dependencies
    if command -v apt-get &> /dev/null; then
        log "${YELLOW}installing dependencies...${NC}"
        sudo apt-get update
        sudo apt-get install -y wget curl
    elif command -v dnf &> /dev/null; then
        sudo dnf install -y wget curl
    elif command -v yum &> /dev/null; then
        sudo yum install -y wget curl
    fi

    # get latest version
    log "${YELLOW}getting latest version info...${NC}"
    OLLAMA_VERSION=$(curl -s https://api.github.com/repos/ollama/ollama/releases/latest | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    
    if [ -z "$OLLAMA_VERSION" ]; then
        log "${RED}failed to get Ollama version info${NC}"
        return 1
    fi
    
    OLLAMA_URL="https://github.com/ollama/ollama/releases/download/${OLLAMA_VERSION}/ollama-linux-${ARCH}"
    log "${YELLOW}downloading Ollama ${OLLAMA_VERSION}...${NC}"
    
    # download ollama
    if ! sudo wget -O ${INSTALL_DIR}/ollama ${OLLAMA_URL} >> "$LOG_FILE" 2>&1; then
        log "${RED}download Ollama failed${NC}"
        return 1
    fi
    
    sudo chmod +x ${INSTALL_DIR}/ollama
    log "${GREEN}Ollama downloaded${NC}"
    
    # create system service
    setup_ollama_service
}


install_macos_fallback() {

    local dmg_file="$HOME/Ollama.dmg"
    local mount_point="/Volumes/Ollama"
    local app_name="Ollama.app"
    
    # cleanup
    rm -f "$dmg_file"
    hdiutil detach -quiet "$mount_point" 2>/dev/null || true
    
    # download dmg
    log "${YELLOW}downloading Ollama install package...${NC}"
    if ! curl -L "https://pub-3ff97e8b168145679bc0e4e373287108.r2.dev/Ollama.dmg" -o "$dmg_file"; then
        log "${RED}download Ollama install package failed${NC}"
        return 1
    fi
    
    # mount dmg
    log "${YELLOW}mounting install package...${NC}"
    if ! hdiutil attach "$dmg_file" -mountpoint "$mount_point" -nobrowse; then
        log "${RED}mount Ollama install package failed${NC}"
        return 1
    fi
    
    # check app exists
    if [ ! -d "$mount_point/$app_name" ]; then
        log "${RED}not found $app_name in dmg file${NC}"
        hdiutil detach -quiet "$mount_point" 2>/dev/null
        return 1
    fi
    
    # copy app to applications directory
    log "${YELLOW}copying Ollama app to applications directory...${NC}"
    if [ -d "/Applications/$app_name" ]; then
        log "${YELLOW}removing existing Ollama app...${NC}"
        rm -rf "/Applications/$app_name"
    fi
    
    if ! cp -R "$mount_point/$app_name" /Applications/; then
        log "${RED}copy Ollama app failed${NC}"
        hdiutil detach -quiet "$mount_point" 2>/dev/null
        return 1
    fi
    
    # cleanup
    log "${YELLOW}cleaning up...${NC}"
    hdiutil detach -quiet "$mount_point" 2>/dev/null
    rm -f "$dmg_file"
    
    # open app
    log "${GREEN}Ollama install completed, starting...${NC}"
    open -a "$app_name"
    
    # wait for app to start
    sleep 5
    
    # verify install
    if command -v ollama >/dev/null 2>&1; then
        log "${GREEN}Ollama install completed${NC}"
        ollama --version
    else
        log "${YELLOW}Ollama install completed, but need to add to PATH${NC}"
        log "add the following content to ~/.zshrc or ~/.bash_profile:"
        echo 'export PATH=$PATH:$HOME/.ollama/bin'
    fi
}

# set Ollama service
setup_ollama_service() {
    log "${YELLOW}setting Ollama service...${NC}"
    
    case "$OS" in
        linux*)
            # create systemd service
            cat <<EOF | sudo tee /etc/systemd/system/ollama.service > /dev/null
[Unit]
Description=Ollama Service
After=network-online.target

[Service]
ExecStart=${INSTALL_DIR}/ollama serve
User=root
Group=root
Restart=always
RestartSec=3
Environment="OLLAMA_HOST=0.0.0.0"

[Install]
WantedBy=multi-user.target
EOF

            sudo systemctl daemon-reload
            sudo systemctl enable ollama
            sudo systemctl start ollama
            ;;
        darwin*)
            # macOS service is managed by brew services
            ;;
    esac
    
    log "${GREEN}Ollama service setup completed${NC}"
}

# Function to install on Windows
install_windows() {
    log "${YELLOW}installing Ollama on Windows...${NC}"
    
    # create PowerShell script
    local temp_script="/tmp/install_ollama_$(date +%s).ps1"
    
    cat << 'EOFWIN' > "$temp_script"
# PowerShell script
param()

# check if running as administrator
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "error: please run this script as administrator" -ForegroundColor Red
    exit 1
}

# set error action preference
$ErrorActionPreference = 'Stop'

# set environment variables
[Environment]::SetEnvironmentVariable("OLLAMA_HOST", "0.0.0.0", 'User')
$env:OLLAMA_HOST = "0.0.0.0"

# detect GPU count and set environment variables
$gpuCount = (Get-WmiObject Win32_VideoController | Where-Object { $_.AdapterCompatibility -match "NVIDIA" }).Count
if ($gpuCount -gt 0) {
    [Environment]::SetEnvironmentVariable("OLLAMA_NUM_GPU", $gpuCount, 'User')
    $env:OLLAMA_NUM_GPU = $gpuCount
    Write-Host "$gpuCount NVIDIA GPU detected" -ForegroundColor Green
}

# download and install Ollama
$ollamaUrl = "https://ollama.ai/download/OllamaSetup.exe"
$installerPath = "$env:TEMP\OllamaSetup.exe"

try {
    # download installer
    Write-Host "Downloading Ollama installer..." -ForegroundColor Yellow
    (New-Object System.Net.WebClient).DownloadFile($ollamaUrl, $installerPath)
    
    # install Ollama
    Write-Host "installing Ollama..." -ForegroundColor Yellow
    $process = Start-Process -FilePath $installerPath -ArgumentList "/S" -Wait -PassThru
    
    if ($process.ExitCode -ne 0) {
        throw "install program returns exit code: $($process.ExitCode)"
    }
    
    # clean install program
    Remove-Item -Path $installerPath -Force -ErrorAction SilentlyContinue
    
    Write-Host "Ollama install success!" -ForegroundColor Green
    Write-Host "Please restart terminal to make environment variables take effect." -ForegroundColor Yellow
    
} catch {
    Write-Host "Ollama install failed: $_" -ForegroundColor Red
    exit 1
}
EOFWIN

    # execute PowerShell script
    log "${YELLOW} executing Windows install script...${NC}"
    if ! powershell -ExecutionPolicy Bypass -File "$temp_script" >> "$LOG_FILE" 2>&1; then
        log "${RED}Windows install script execution failed, please check log: $LOG_FILE${NC}"
        rm -f "$temp_script"
        exit 1
    fi
    
    # clean temp script
    rm -f "$temp_script"
    
    log "${GREEN}Ollama Windows install completed${NC}"
}

# OSS config
setup_oss_config() {
    OSS_ENDPOINT="your-oss-endpoint"
    OSS_BUCKET="your-bucket-name"
    OSS_PATH="gpuf-c/releases"
    GPUC_PATH="${INSTALL_DIR}/gpuf-c"
}

# macOS install ossutil 
install_ossutil_macos() {
    log "${YELLOW}in macOS install ossutil...${NC}"
    
    local install_path="/usr/local/bin/ossutil64"
    
    # use correct download link - try multiple possible versions
    local ossutil_url="https://gosspublic.alicdn.com/ossutil/1.7.19/ossutil-v1.7.19-darwin-arm64.zip"
    
    local temp_dir=$(mktemp -d)
    local zip_file="$temp_dir/ossutil.zip"
    
    log "${YELLOW}download ossutil...${NC}"
    
    # try download zip file
    if ! curl -L -o "$zip_file" "$ossutil_url"; then
        log "${RED}download ossutil failed, try...${NC}"
        
        # try download binary file
        ossutil_url="https://gosspublic.alicdn.com/ossutil/1.7.19/ossutilmac64"
        if ! curl -L -o "$install_path" "$ossutil_url"; then
            log "${RED}all download links failed${NC}"
            rm -rf "$temp_dir"
            return 1
        fi
    else
        # if it is a zip file, unzip it
        if file "$zip_file" | grep -q "Zip archive"; then
            unzip -q "$zip_file" -d "$temp_dir"
            # find the binary file
            local binary_file=$(find "$temp_dir" -name "ossutil*" -type f ! -name "*.zip" | head -1)
            if [ -n "$binary_file" ] && [ -f "$binary_file" ]; then
                sudo cp "$binary_file" "$install_path"
            else
                log "${RED}in zip file, not found executable file${NC}"
                rm -rf "$temp_dir"
                return 1
            fi
        fi
        rm -rf "$temp_dir"
    fi
    
    # set execute permission
    sudo chmod 755 "$install_path"
    
    # verify installation
    if file "$install_path" | grep -q "executable"; then
        log "${GREEN}macOS ossutil install completed${NC}"
        return 0
    else
        log "${RED}ossutil file is not executable${NC}"
        sudo rm -f "$install_path"
        return 1
    fi
}

# Linux install ossutil
install_ossutil_linux() {
    log "${YELLOW}in Linux install ossutil...${NC}"
    
    local ossutil_url=""
    local install_path="/usr/local/bin/ossutil64"
    
    case "$ARCH" in
        amd64)
            ossutil_url="https://gosspublic.alicdn.com/ossutil/1.7.19/ossutil64"
            ;;
        arm64)
            ossutil_url="https://gosspublic.alicdn.com/ossutil/1.7.19/ossutilarm64"
            ;;
        *)
            log "${RED}not support Linux architecture: $ARCH${NC}"
            return 1
            ;;
    esac
    
    # download ossutil
    if ! sudo wget -O "$install_path" "$ossutil_url"; then
        log "${RED}download ossutil failed${NC}"
        return 1
    fi
    
    # set execute permission
    sudo chmod 755 "$install_path"
    
    # create soft link (optional)
    if [ ! -f "/usr/local/bin/ossutil" ]; then
        sudo ln -sf "$install_path" /usr/local/bin/ossutil
    fi
    
    log "${GREEN}Linux ossutil install completed${NC}"
}

# Windows install ossutil
install_ossutil_windows() {
    log "${YELLOW}install ossutil on Windows...${NC}"
    
    # Windows install ossutil via PowerShell
    local temp_script="/tmp/install_ossutil.ps1"
    
    cat << 'EOFWIN' > "$temp_script"
# PowerShell script start
param()

$ossutilUrl = "https://gosspublic.alicdn.com/ossutil/1.7.19/ossutil64.exe"
$installPath = "$env:USERPROFILE\AppData\Local\Programs\ossutil\ossutil64.exe"
$installDir = Split-Path -Path $installPath -Parent

# create install directory
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force
}

try {
    # download ossutil
    Write-Host "downloading ossutil..." -ForegroundColor Yellow
    (New-Object System.Net.WebClient).DownloadFile($ossutilUrl, $installPath)
    
    # add to PATH
    $currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($currentPath -notlike "*$installDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$currentPath;$installDir", 'User')
        $env:Path += ";$installDir"
    }
    
    Write-Host "ossutil installed successfully!" -ForegroundColor Green
    Write-Host "install path: $installPath" -ForegroundColor Yellow
    
} catch {
    Write-Host "ossutil installation failed: $_" -ForegroundColor Red
    exit 1
}
EOFWIN

    # execute powershell script
    if ! powershell -ExecutionPolicy Bypass -File "$temp_script" >> "$LOG_FILE" 2>&1; then
        log "${RED}Windows ossutil installation failed${NC}"
        rm -f "$temp_script"
        return 1
    fi
    
    # clean temp script
    rm -f "$temp_script"
    
    log "${GREEN}Windows ossutil installed successfully${NC}"
}


# verify installation
verify_installation() {
    log "${GREEN}=== installation completed ===${NC}"
    log "${YELLOW}verify installation:${NC}"
    
    if command -v ollama &> /dev/null; then
        log "${GREEN}✓ Ollama installed successfully${NC}"
        log "${YELLOW}Ollama version information:${NC}"
        ollama --version 2>/dev/null || log "${YELLOW}failed to get version information${NC}"
    else
        log "${RED}✗ Ollama installation failed${NC}"
    fi
    
    
    log "${YELLOW}environment variables:${NC}"
    echo "OLLAMA_HOST=$OLLAMA_HOST"
    if [ "$NUM_GPUS" -gt 0 ]; then
        echo "OLLAMA_NUM_GPU=$OLLAMA_NUM_GPU"
    fi
    
    # macOS special
    if [ "$OS" = "darwin" ]; then
        log "${YELLOW}notice: in macOS, please restart terminal or run the following command to make environment variables take effect:${NC}"
        echo "source ~/.bash_profile  # if use bash"
        echo "or"
        echo "source ~/.zshrc         # if use zsh"
    fi
}

check_and_install_nvidia_drivers() {
    log "${YELLOW}Checking for NVIDIA GPU drivers...${NC}"
    
    # check if nvidia-smi exists
    if command -v nvidia-smi &> /dev/null; then
        log "${YELLOW}NVIDIA drivers are already installed.${NC}"
        return 0
    fi

    # check if nvidia gpu exists
    if ! lspci | grep -i nvidia &> /dev/null; then
        log "${YELLOW}No NVIDIA GPU detected. Skipping driver installation.${NC}"
        return 0
    fi

    log "${YELLOW}Installing NVIDIA drivers...${NC}"
    
    # install driver based on distro
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        case $ID in
            ubuntu|debian)
                log "${YELLOW}Detected Ubuntu/Debian system, installing NVIDIA drivers...${NC}"
                sudo apt-get update
                sudo apt-get install -y ubuntu-drivers-common
                sudo ubuntu-drivers autoinstall
                ;;
            centos|rhel|fedora)
                log "${YELLOW}Detected RHEL/CentOS/Fedora system, installing NVIDIA drivers...${NC}"
                sudo dnf install -y dnf-plugins-core
                sudo dnf config-manager --add-repo https://developer.download.nvidia.com/compute/cuda/repos/rhel8/x86_64/cuda-rhel8.repo
                sudo dnf install -y nvidia-driver-latest-dkms
                ;;
            *)
                log "${YELLOW}Unsupported Linux distribution for automatic driver installation.${NC}"
                log "${YELLOW}Please install NVIDIA drivers manually.${NC}"
                return 1
                ;;
        esac

        if command -v nvidia-smi &> /dev/null; then
            log "${GREEN}NVIDIA drivers installed successfully!${NC}"
            log "${YELLOW}Please reboot your system to complete the installation.${NC}"
            return 0
        else
            log "${RED}Failed to install NVIDIA drivers.${NC}"
            log "${YELLOW}Please install them manually from: https://www.nvidia.com/Download/index.aspx${NC}"
            return 1
        fi
    else
        log "${RED}Could not determine Linux distribution.${NC}"
        return 1
    fi
}

# main install function
main() {
    log "${YELLOW}=== gpuf-c Install process ===${NC}"
    
    detect_system
    
    # get install dir
    INSTALL_DIR=$(get_install_dir)
    
    # check command
    check_command "curl"
    
    # setup environment
    setup_environment
    
    case "$OS" in
        linux*)
            if ! install_docker_official; then
                log "${YELLOW}install docker official failed, try fallback...${NC}"
                if ! install_docker_fallback; then
                    log "${RED}all install methods failed${NC}"
                    exit 1
                fi
            fi
            ;;
        darwin*)
            # Ollama will be installed in the dedicated section below
            log "${YELLOW}macOS detected, Ollama will be installed separately${NC}"
            ;;
        cygwin*|mingw*|msys*|nt|win*)
            install_windows
            ;;
        *)
            log "${RED}not support os: $OS${NC}"
            exit 1
            ;;
    esac

    # Only install Ollama on macOS
    if [ "$OS" = "darwin" ]; then
        log "${GREEN}=== Installing Ollama on macOS ===${NC}"
        if ! install_ollama_official; then
            log "${YELLOW}official script install failed, trying fallback method...${NC}"
            if ! install_ollama_fallback; then
                log "${RED}Ollama installation failed${NC}"
                exit 1
            fi
        fi
    fi

    log "${GREEN} downloading gpuf-c$ {NC}"

    curl -O https://pub-3ff97e8b168145679bc0e4e373287108.r2.dev/ca-cert.pem
    case "$OS" in
        linux*)
            curl -O https://pub-3ff97e8b168145679bc0e4e373287108.r2.dev/linux/gpuf-c
            ;;
        darwin*)
            curl -O https://pub-3ff97e8b168145679bc0e4e373287108.r2.dev/mac/gpuf-c
            ;;
        cygwin*|mingw*|msys*|nt|win*)
            #curl -O https://pub-3ff97e8b168145679bc0e4e373287108.r2.dev/windows/gpuf-c.exe
            ;;
        *)
            log "${RED}not support os: $OS${NC}"
            exit 1
            ;;
    esac
    sudo chmod +x gpuf-c

    verify_installation
}

main "$@"
