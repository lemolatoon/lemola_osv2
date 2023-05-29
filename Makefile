.PHONY: FORCE
PROFILE=debug

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
	qemu-system-x86_64 \
		-drive if=pflash,file=ovmf/OVMF_CODE.fd,format=raw \
		-drive if=pflash,file=ovmf/lemola_os_ovmf_vars.fd,format=raw \
		-drive file=disk.img,format=raw \
		-monitor stdio

run_gdb: disk.img
	qemu-system-x86_64 \
		-drive if=pflash,file=ovmf/OVMF_CODE.fd,format=raw \
		-drive if=pflash,file=ovmf/lemola_os_ovmf_vars.fd,format=raw \
		-drive file=disk.img,format=raw \
		-monitor stdio \
		-gdb tcp::12345 -S
# on gdb
# target remote localhost:12345
	
test_kernel:
	cd kernel-lib && \
	cargo test --features "std"

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

kill:
	killall -9 qemu-system-x86_64