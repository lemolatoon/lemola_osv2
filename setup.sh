#!/bin/bash

# only for ubuntu

# NOTE* install rust before run this

sudo apt-get install qemu-kvm -y
sudo apt-get install ovmf -y
sudo apt-get install make -y

# setup ovmf files by yourself

# cp /usr/share/OVMF/OVMF_VARS.fd ./ovmf/lemola_os_ovmf_vars.fd
# cp /usr/share/OVMF/OVMF_CODE.fd ./ovmf/OVMF_CODE.fd
