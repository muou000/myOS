export A := $(PWD)
export ARCH := riscv64
export LOG := info
# Do not set `APP_FEATURES` globally; apps may not define `qemu` feature
# APP_FEATURES := qemu

defconfig:
	@make -C arceos $@

build run justrun: defconfig
	@make -C arceos $@

clean:
	@make -C arceos $@