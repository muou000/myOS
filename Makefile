export A := $(PWD)
export NO_AXSTD := y
export AX_LIB := axfeat
export APP_FEATURES := qemu

export ARCH := riscv64
export LOG := info

build run justrun: defconfig
	@make -C arceos $@

clean defconfig:
	@make -C arceos $@