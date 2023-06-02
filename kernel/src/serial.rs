use core::arch::asm;

use spin::Mutex;

const PORT: u16 = 0x3f8;

fn is_transmit_empty() -> bool {
    let ret: u8;
    return inb(PORT + 5) & 0x20 != 0;
}

fn inb(port: u16) -> u8 {
    let ret: u8;
    unsafe {
        asm!("in al, dx", out("al") ret, in("dx") port, options(nomem, nostack));
    };
    return ret;
}

fn outb(port: u16, data: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") data, options(nomem, nostack));
    };
}

fn write_serial(byte: u8) {
    while !is_transmit_empty() {}
    outb(PORT, byte);
}

pub fn write_serial_str(string: &str) {
    for byte in string.bytes() {
        write_serial(byte);
    }
}

struct SerialWriter(Mutex<()>);

impl core::fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_serial_str(s);
        Ok(())
    }
}

#[doc(hidden)]
pub fn _serial_print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    SerialWriter(Mutex::new(())).write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::serial::_serial_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}
