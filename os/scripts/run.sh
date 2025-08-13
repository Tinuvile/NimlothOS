# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置变量
TARGET="riscv64gc-unknown-none-elf"
MODE="release"
KERNEL_ELF="target/${TARGET}/${MODE}/os"
KERNEL_BIN="${KERNEL_ELF}.bin"
BOOTLOADER="../bootloader/rustsbi-qemu.bin"
KERNEL_ENTRY_PA="0x80200000"

# 构建内核
if ! cargo build --release; then
    echo -e "${RED}错误: 内核构建失败${NC}"
    exit 1
fi

# 生成二进制文件
if ! rust-objcopy --strip-all ${KERNEL_ELF} -O binary ${KERNEL_BIN}; then
    echo -e "${RED}错误: 二进制文件生成失败${NC}"
    exit 1
fi

# 启动 QEMU
# 执行 QEMU 命令
qemu-system-riscv64 \
    -machine virt \
    -nographic \
    -bios ${BOOTLOADER} \
    -device loader,file=${KERNEL_BIN},addr=${KERNEL_ENTRY_PA}