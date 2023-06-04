extern crate alloc;
use alloc::vec::Vec;
use core::arch::global_asm;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PciConfigAddress(u32);

impl PciConfigAddress {
    pub fn new(bus: u8, device: u8, function: u8, register: u8) -> Self {
        let mut address: u32 = 0;
        address |= 1 << 31; // enable bit
        address |= (bus as u32) << 16;
        address |= (device as u32) << 11;
        address |= (function as u32) << 8;
        address |= (register & 0b1100) as u32;
        Self(address)
    }

    pub fn new_from_bar_index(bus: u8, device: u8, function: u8, bar_index: u8) -> Option<Self> {
        if bar_index >= 6 {
            return None;
        }
        Some(Self::new(bus, device, function, 0x10 + bar_index * 4))
    }
}
const CONFIG_ADDRESS: u16 = 0xcf8;
const CONFIG_DATA: u16 = 0xcfc;

unsafe fn write_address(address: PciConfigAddress) {
    io_out_32(CONFIG_ADDRESS, address.0);
}
unsafe fn read_date() -> u32 {
    io_in_32(CONFIG_DATA)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PciDevice {
    vendor_id: VendorId,
    device_id: DeviceId,
    class_code: ClassCode,
    header_type: HeaderType,
    bus: u8,
    device: u8,
    function: u8,
}

impl PciDevice {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        let raw_data = read_data(PciConfigAddress::new(bus, device, function, 0));
        let vendor_id = VendorId::from_raw(raw_data);
        let device_id = DeviceId::from_raw(raw_data);
        let header_type = HeaderType::new(bus, device, function);
        let class_code = ClassCode::new(bus, device, function);
        Self {
            vendor_id,
            device_id,
            class_code,
            header_type,
            bus,
            device,
            function,
        }
    }

    pub const fn is_standard_pci_pci_bridge(&self) -> bool {
        self.class_code.base() == 0x06 && self.class_code.sub() == 0x04
    }

    pub fn read_bar_32(&self, bar_index: u8) -> Option<u32> {
        let bar = read_data(PciConfigAddress::new_from_bar_index(
            self.bus,
            self.device,
            self.function,
            bar_index,
        )?);
        return Some(bar);
    }

    pub fn read_bar_64(&self, bar_index: u8) -> Option<u64> {
        let bar = read_data(PciConfigAddress::new_from_bar_index(
            self.bus,
            self.device,
            self.function,
            bar_index,
        )?);
        let bar_upper = read_data(PciConfigAddress::new_from_bar_index(
            self.bus,
            self.device,
            self.function,
            bar_index + 1,
        )?);
        return Some(bar as u64 | ((bar_upper as u64) << 32));
    }

    pub const fn vendor_id(&self) -> VendorId {
        self.vendor_id
    }
    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }
    pub const fn class_code(&self) -> ClassCode {
        self.class_code
    }
    pub const fn header_type(&self) -> HeaderType {
        self.header_type
    }
    pub const fn bus(&self) -> u8 {
        self.bus
    }
    pub const fn device(&self) -> u8 {
        self.device
    }
    pub const fn function(&self) -> u8 {
        self.function
    }
}

pub struct BusNumber(u32);

impl BusNumber {
    // TODO: change fn by HeaderType
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        let raw_data = read_data(PciConfigAddress::new(bus, device, function, 0x18));
        Self(raw_data)
    }
    pub fn secondary_bus_number(&self) -> u8 {
        (self.0 >> 8) as u8
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClassCode {
    base: u8,
    sub: u8,
    interface: u8,
}

impl ClassCode {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        let raw_data = read_data(PciConfigAddress::new(bus, device, function, 0x08));
        Self::from_raw(raw_data)
    }

    /// from register offset 0x08
    fn from_raw(raw_data: u32) -> Self {
        Self {
            base: ((raw_data >> 24) & 0xff) as u8,
            sub: ((raw_data >> 16) & 0xff) as u8,
            interface: ((raw_data >> 8) & 0xff) as u8,
        }
    }

    pub const fn base(&self) -> u8 {
        self.base
    }
    pub const fn sub(&self) -> u8 {
        self.sub
    }
    pub const fn interface(&self) -> u8 {
        self.interface
    }

    pub const fn matches(&self, base: u8, sub: u8, interface: u8) -> bool {
        self.base == base && self.sub == sub && self.interface == interface
    }

    pub const fn is_xhci(&self) -> bool {
        self.matches(0x0c, 0x03, 0x30)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HeaderType(u8);

impl HeaderType {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        let raw_data = read_data(PciConfigAddress::new(bus, device, function, 0x0c));
        Self::from_raw(raw_data)
    }

    /// from register offset 0x0c
    fn from_raw(raw_data: u32) -> Self {
        Self(((raw_data >> 16) & 0xff) as u8)
    }

    pub fn is_multi_function(&self) -> bool {
        self.0 & 0x80 != 0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DeviceId(u16);

impl DeviceId {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        let raw_data = read_data(PciConfigAddress::new(bus, device, function, 0));
        Self::from_raw(raw_data)
    }

    /// from register offset 0
    fn from_raw(raw_data: u32) -> Self {
        Self((raw_data >> 16) as u16)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VendorId(u16);

impl VendorId {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        let raw_data = read_data(PciConfigAddress::new(bus, device, function, 0));
        Self::from_raw(raw_data)
    }

    /// from register offset 0
    fn from_raw(raw_data: u32) -> Self {
        Self(raw_data as u16)
    }

    pub fn is_valid(&self) -> bool {
        self.0 != 0xffff
    }

    pub fn is_intel(&self) -> bool {
        self.0 == 0x8086
    }
}

pub fn read_data(address: PciConfigAddress) -> u32 {
    unsafe {
        write_address(address);
        read_date()
    }
}

extern "sysv64" {
    fn io_out_32(address: u16, data: u32);
    fn io_in_32(address: u16) -> u32;
}

global_asm!(
    ".global io_out_32",
    "io_out_32:",
    "  mov dx, di",   // dx = address
    "  mov eax, esi", // eax = data
    "  out dx, eax",
    "  ret",
);
global_asm!(
    ".global io_in_32",
    "  io_in_32:",
    "  mov dx, di", // dx = address
    "  in eax, dx", // eax = data (return value)
    "  ret",
);

pub fn scan_all_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    let header_type = HeaderType::new(0, 0, 0);
    if !header_type.is_multi_function() {
        scan_bus(0, &mut devices);
        return devices;
    }
    for function in 1..8 {
        let vendor_id = VendorId::new(0, 0, function);
        if !vendor_id.is_valid() {
            continue;
        }
        scan_bus(function, &mut devices);
    }

    return devices;
}

pub fn scan_bus(bus: u8, devices: &mut Vec<PciDevice>) {
    for device in 0..32 {
        // 実際にdeviceがあるか確認
        let vendor_id = VendorId::new(bus, device, 0);
        if !vendor_id.is_valid() {
            continue;
        }
        scan_device(bus, device, devices);
    }
}

pub fn scan_device(bus: u8, device: u8, devices: &mut Vec<PciDevice>) {
    let header_type = HeaderType::new(bus, device, 0);
    if header_type.is_multi_function() {
        for function in 0..8 {
            scan_function(bus, device, function, devices);
        }
    } else {
        scan_function(bus, device, 0, devices);
    }
}

pub fn scan_function(bus: u8, device: u8, function: u8, devices: &mut Vec<PciDevice>) {
    let pci_device = PciDevice::new(bus, device, function);

    if pci_device.is_standard_pci_pci_bridge() {
        let secondary_bus_number = BusNumber::new(bus, device, function).secondary_bus_number();
        scan_bus(secondary_bus_number, devices);
    }
    devices.push(pci_device);
}
