use bit_field::BitField;

use crate::interrupts::InterruptVector;

use self::register::PciDevice;

pub mod register;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum MSITriggerMode {
    Edge = 0,
    Level = 1,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum MSIDeliveryMode {
    Fixed = 0b000,
    LowestPriority = 0b001,
    SMI = 0b010,
    NMI = 0b100,
    INIT = 0b101,
    ExtINT = 0b111,
}

pub fn configure_msi_fixed_destination(
    pci_device: &PciDevice,
    apic_id: u8,
    trigger_mode: MSITriggerMode,
    delivery_mode: MSIDeliveryMode,
    interrup_vector: InterruptVector,
    num_vector_exponent: usize,
) {
    let msg_addr = 0xfee00000 | ((apic_id as u32) << 12);
    let mut msg_data = ((delivery_mode as u32) << 8) | interrup_vector as u32;
    if let MSITriggerMode::Level = trigger_mode {
        msg_data |= 0xc000;
    }

    configure_msi(pci_device, msg_addr, msg_data, num_vector_exponent);
}

pub fn configure_msi(
    pci_device: &PciDevice,
    msg_addr: u32,
    msg_data: u32,
    num_vector_exponent: usize,
) {
    let cap_addr = pci_device.read_capabilities_pointer();
    let msi_cap_addr: u8 = 0;
    let msix_cap_addr: u8 = 0;
    todo!()
}

pub struct MsiCapability([u32; 4]);

pub struct MessageControl(u16);

impl MessageControl {
    pub fn read_at(cap_addr: u8) -> Self {
        unsafe { Self((cap_addr as *const u16).read_volatile()) }
    }

    pub fn write_at(&self, cap_addr: u8) {
        unsafe { (cap_addr as *mut u16).write_volatile(self.0) }
    }

    pub fn per_vector_masking(&self) -> bool {
        self.0.get_bit(8)
    }

    pub fn set_per_vector_masking(&mut self, value: bool) {
        self.0.set_bit(8, value);
    }

    pub fn multiple_message_enable(&self) -> u16 {
        self.0.get_bits(4..7)
    }

    pub fn set_multiple_message_enable(&mut self, value: u16) {
        self.0.set_bits(4..7, value);
    }

    pub fn multiple_message_capable(&self) -> u16 {
        self.0.get_bits(1..4)
    }

    pub fn set_multiple_message_capable(&mut self, value: u16) {
        self.0.set_bits(1..4, value);
    }

    pub fn enable(&self) -> bool {
        self.0.get_bit(0)
    }

    pub fn set_enable(&mut self, value: bool) {
        self.0.set_bit(0, value);
    }
}

impl MsiCapability {
    pub unsafe fn new(cap_addr: u8) -> Self {
        (cap_addr as *const Self).read_volatile()
    }

    pub fn message_control(&self) -> MessageControl {
        MessageControl(self.0[0].get_bits(16..32) as u16)
    }

    pub fn set_message_control(&mut self, control: MessageControl) {
        self.0[0].set_bits(16..32, control.0 as u32);
    }

    pub fn next_pointer(&self) -> u8 {
        self.0[0].get_bits(8..16) as u8
    }

    pub fn capability_id(&self) -> u8 {
        self.0[0].get_bits(0..8) as u8
    }

    pub fn message_address(&self) -> u64 {
        let l = self.0[1] as u64;
        let u = self.0[2] as u64;

        u << 32 | l
    }

    pub fn set_message_address(&mut self, address: u64) {
        self.0[1] = address as u32;
        self.0[2] = (address >> 32) as u32;
    }

    pub fn message_data(&self) -> u16 {
        self.0[3].get_bits(0..16) as u16
    }

    pub fn set_message_data(&mut self, data: u16) {
        self.0[3].set_bits(0..16, data as u32);
    }
}
