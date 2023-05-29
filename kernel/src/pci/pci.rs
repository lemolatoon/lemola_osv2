use core::arch::global_asm;

const CONFIG_ADDRESS: u16 = 0xcf8;
const CONFIG_DATA: u16 = 0xcfc;
const fn make_address(bus: u8, device: u8, function: u8, register: u8) -> u32 {
    let mut address: u32 = 0;
    address |= 1 << 31; // enable bit
    address |= (bus as u32) << 16;
    address |= (device as u32) << 11;
    address |= (function as u32) << 8;
    address |= (register & 0b1100) as u32;
    address
}

extern "C" {
    fn io_out_32(address: u16, data: u32);
    fn io_in_32(address: u16) -> u32;
}

global_asm!(
    ".global io_out_32",
    "io_out_32:",
    "mov dx, di",   // dx = address
    "mov eax, esi", // eax = data
    "out dx, eax",
    "ret",
    ".global io_in_32",
    "io_in_32:",
    "mov dx, di", // dx = address
    "in eax, dx", // eax = data (return value)
    "ret"
);
