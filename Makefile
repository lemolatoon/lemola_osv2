.PHONY: FORCE
PROFILE=debug
QEMU=qemu-system-x86_64

ifeq ($(PROFILE),release)
	CARGO_FLAGS=--release
endif

kernel/target/x86_64-lemolaos-eabi/$(PROFILE)/kernel.elf: FORCE
	cd kernel && \
	cargo build $(CARGO_FLAGS)

bootloader/target/x86_64-unknown-uefi/$(PROFILE)/bootloader.efi: FORCE
	cd bootloader && \
	cargo build $(CARGO_FLAGS)

build: bootloader/target/x86_64-unknown-uefi/$(PROFILE)/bootloader.efi kernel/target/x86_64-lemolaos-eabi/$(PROFILE)/kernel.elf

disk.img: bootloader/target/x86_64-unknown-uefi/$(PROFILE)/bootloader.efi kernel/target/x86_64-lemolaos-eabi/$(PROFILE)/kernel.elf
#	qemu-img create [-f format] [-o options] filename [size][preallocation]
#	mkfs.fat [-n VOLUME-NAME] [-s SECTORS-PER-CLUSTER] [-f NUMBER-OF-FATS] [-R NUMBER-OF-RESERVED-SECTORS] [-F FAT-SIZE]
#	ref.) `man mkfs.fat`
#	mount [-o options] device dir
#	loop -> mount as loop device
	qemu-img create -f raw disk.img 200M && \
	mkfs.fat -n "lemola_osv2" -s 2 -f 2 -R 32 -F 32 disk.img && \
	mkdir -p mnt && \
	sudo mount -o loop disk.img mnt && \
	sudo mkdir -p mnt/EFI/BOOT && \
	sudo cp bootloader/target/x86_64-unknown-uefi/$(PROFILE)/bootloader.efi mnt/EFI/BOOT/BOOTX64.EFI && \
	sudo cp kernel/target/x86_64-lemolaos-eabi/$(PROFILE)/kernel.elf mnt/kernel.elf && \
	sudo umount mnt

run: disk.img
	$(QEMU) \
		-drive if=pflash,file=ovmf/OVMF_CODE.fd,format=raw \
		-drive if=pflash,file=ovmf/lemola_os_ovmf_vars.fd,format=raw,readonly \
		-drive file=disk.img,format=raw \
		-device nec-usb-xhci,id=xhci \
 		-device usb-mouse \
		-serial telnet::5555,server,nowait \
		-no-reboot \
		-no-shutdown \
		-monitor stdio
#		-device usb-kbd 
# for serial port
# telnet localhost 5555

run_gdb: disk.img
	$(QEMU) \
		-drive if=pflash,file=ovmf/OVMF_CODE.fd,format=raw \
		-drive if=pflash,file=ovmf/lemola_os_ovmf_vars.fd,format=raw \
		-drive file=disk.img,format=raw \
		-device nec-usb-xhci,id=xhci \
		-device usb-kbd \
		-device usb-mouse \
		-serial telnet::5555,server,nowait \
		-monitor stdio \
		-no-reboot \
		-no-shutdown \
		-gdb tcp::12345 -S
# on gdb
# target remote localhost:12345

telnet:
	while true; do \
		telnet localhost 5555 2> /dev/null | grep -v "Trying 127.0.0.1..."; \
		if [ "$$?" -eq 0 ]; then \
			echo "===========disconnected==========="; \
			fi; \
		sleep 1; \
	done;
	
test_kernel:
	cd kernel-lib && \
	cargo test --features "std"

test_font:
	cd gen_font && \
	cargo test

test_all: test_kernel test_font
	
fmt: 
	cd kernel && \
	cargo fmt 
	cd common && \
	cargo fmt 
	cd gen_font && \
	cargo fmt 
	cd bootloader && \
	cargo fmt 
	cd kernel-lib && \
	cargo fmt 

check: 
	cd kernel && \
	cargo check 
	cd common && \
	cargo check 
	cd gen_font && \
	cargo check 
	cd bootloader && \
	cargo check 
	cd kernel-lib && \
	cargo check 

fmt_ci: 
	cd kernel && \
	cargo fmt --all -- --check
	cd common && \
	cargo fmt --all -- --check
	cd gen_font && \
	cargo fmt --all -- --check
	cd bootloader && \
	cargo fmt --all -- --check
	cd kernel-lib && \
	cargo fmt --all -- --check

clippy_ci:
	cd kernel && \
	cargo clippy -- -D warnings
	cd common && \
	cargo clippy -- -D warnings
	cd gen_font && \
	cargo clippy -- -D warnings
	cd bootloader && \
	cargo clippy -- -D warnings
	cd kernel-lib && \
	cargo clippy -- -D warnings

clippy:
	cd kernel && \
	cargo clippy
	cd common && \
	cargo clippy
	cd gen_font && \
	cargo clippy
	cd bootloader && \
	cargo clippy
	cd kernel-lib && \
	cargo clippy

clean:
	cd kernel && \
	cargo clean
	cd common && \
	cargo clean
	cd gen_font && \
	cargo clean
	cd bootloader && \
	cargo clean
	cd kernel-lib && \
	cargo clean
	

kill:
	killall -9 qemu-system-x86_64
	git checkout f249e14540fc9ea6cabe4bfd932db2888e0d97ee -- ovmf/lemola_os_ovmf_vars.fd
