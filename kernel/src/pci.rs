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
    interrupt_vector: InterruptVector,
    num_vector_exponent: usize,
) {
    let msg_addr = 0xfee0_0000 | ((apic_id as u32) << 12);
    log::debug!("msg_addr: {:#x}", msg_addr);
    let mut msg_data = ((delivery_mode as u32) << 8) | interrupt_vector as u32;
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
    let iter = MsiCapabilityIterator::new(pci_device, cap_addr);
    let mut written = false;
    for (cap_addr, mut msi_cap) in iter {
        log::debug!("MSI capability found at {:#x}\n{:x?}", cap_addr, &msi_cap);
        let mut message_control = msi_cap.message_control();
        if message_control.multiple_message_capable() <= num_vector_exponent as u16 {
            message_control.set_multiple_message_enable(message_control.multiple_message_capable());
        } else {
            message_control.set_multiple_message_enable(num_vector_exponent as u16);
        }

        message_control.set_enable(true);
        msi_cap.set_message_control(message_control);
        msi_cap.set_message_address(msg_addr as u64);
        log::debug!("msg_data: {:#x}", msg_data);
        msi_cap.set_message_data(msg_data as u16);

        log::debug!("MSI capability updated@0x{:x}\n{:x?}", cap_addr, &msi_cap);
        log::debug!("MSI capability raw: {:x?}", &msi_cap.0);
        write_msi_capability(pci_device, cap_addr, msi_cap);
        written = true;
    }

    if !written {
        panic!("MSI capability not found");
    }
}

pub fn write_msi_capability(device: &PciDevice, cap_addr: u8, msi_cap: MsiCapability) {
    device.write_conf_reg(cap_addr, msi_cap.header());
    device.write_conf_reg(cap_addr + 4, msi_cap.message_address() as u32);

    let mut msg_data_addr = cap_addr + 8;
    if msi_cap.message_control().address_64_bit_capable() {
        device.write_conf_reg(
            cap_addr + 8,
            msi_cap.message_address().get_bits(32..64) as u32,
        );
        msg_data_addr = cap_addr + 12;
    }

    device.write_conf_reg(msg_data_addr, msi_cap.message_data() as u32);

    if msi_cap.message_control().per_vector_masking() {
        device.write_conf_reg(msg_data_addr + 4, msi_cap.mask_bits());
        device.write_conf_reg(msg_data_addr + 8, msi_cap.pending_bits());
    }
}

// https://wiki.osdev.org/PCI#Enabling_MSI

pub struct MessageControl(u16);

impl core::fmt::Debug for MessageControl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MessageControl")
            .field("per_vector_masking", &self.per_vector_masking())
            .field("address_64_bit_capable", &self.address_64_bit_capable())
            .field("multiple_message_enable", &self.multiple_message_enable())
            .field("multiple_message_capable", &self.multiple_message_capable())
            .field("enable", &self.enable())
            .finish()
    }
}

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

    pub fn address_64_bit_capable(&self) -> bool {
        self.0.get_bit(7)
    }

    pub fn set_address_64_bit_capable(&mut self, value: bool) {
        self.0.set_bit(7, value);
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

    pub fn data(&self) -> u16 {
        self.0
    }
}

pub struct MsiCapability([u32; 6]);

impl core::fmt::Debug for MsiCapability {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MsiCapability")
            .field("message_control", &self.message_control())
            .field("next_pointer", &self.next_pointer())
            .field("capability_id", &self.capability_id())
            .field("message_address", &self.message_address())
            .field("message_data", &self.message_data())
            .field("mask_bits", &self.mask_bits())
            .field("pending_bits", &self.pending_bits())
            .finish()
    }
}

impl MsiCapability {
    pub fn new(device: &PciDevice, cap_addr: u8) -> Self {
        let mut cap = Self([0; 6]);

        // cap + 0x0
        let header = device.read_configuration_space(cap_addr);
        let message_control = MessageControl(header.get_bits(16..32) as u16);
        cap.set_message_control(message_control);
        cap.set_next_pointer(header.get_bits(8..16) as u8);
        cap.set_capability_id(header.get_bits(0..8) as u8);

        // cap + 0x4
        let message_address_lower = device.read_configuration_space(cap_addr + 4) as u64;

        let mut msg_data_addr = cap_addr + 8;

        let message_address = if cap.message_control().address_64_bit_capable() {
            let message_address_upper = device.read_configuration_space(cap_addr + 8) as u64;
            msg_data_addr = cap_addr + 12;
            message_address_upper << 32 | message_address_lower
        } else {
            message_address_lower
        };
        cap.set_message_address(message_address);

        let message_data = device
            .read_configuration_space(msg_data_addr)
            .get_bits(0..16) as u16;
        cap.set_message_data(message_data);

        if cap.message_control().per_vector_masking() {
            let mask_bits = device.read_configuration_space(msg_data_addr + 4);
            let pending_bits = device.read_configuration_space(msg_data_addr + 8);
            cap.set_mask_bits(mask_bits);
            cap.set_pending_bits(pending_bits);
        }

        cap
    }

    pub fn write_at(&self, cap_addr: u8) {
        unsafe { (cap_addr as *mut [u32; 6]).write_volatile(self.0) }
    }

    pub fn header(&self) -> u32 {
        self.0[0]
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

    pub fn set_next_pointer(&mut self, value: u8) {
        self.0[0].set_bits(8..16, value as u32);
    }

    pub fn capability_id(&self) -> u8 {
        self.0[0].get_bits(0..8) as u8
    }

    pub fn set_capability_id(&mut self, value: u8) {
        self.0[0].set_bits(0..8, value as u32);
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

    pub fn mask_bits(&self) -> u32 {
        self.0[4]
    }

    pub fn set_mask_bits(&mut self, mask_bits: u32) {
        self.0[4] = mask_bits;
    }

    pub fn pending_bits(&self) -> u32 {
        self.0[5]
    }

    pub fn set_pending_bits(&mut self, pending_bits: u32) {
        self.0[5] = pending_bits;
    }
}

#[derive(Debug)]
pub struct MsiCapabilityIterator<'a> {
    device: &'a PciDevice,
    current_cap_addr: u8,
}

impl<'a> MsiCapabilityIterator<'a> {
    pub fn new(pci_device: &'a PciDevice, cap_addr: u8) -> Self {
        Self {
            device: pci_device,
            current_cap_addr: cap_addr,
        }
    }
}

impl<'a> Iterator for MsiCapabilityIterator<'a> {
    type Item = (u8, MsiCapability);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_cap_addr == 0 {
            return None;
        }
        log::debug!("reading msi cap at 0x{:x}", self.current_cap_addr);
        let mut cap = MsiCapability::new(self.device, self.current_cap_addr);
        while cap.capability_id() != 0x05 {
            // MSIでない
            log::debug!("not msi cap: {:x?} @ {:x}", &cap, self.current_cap_addr);
            self.current_cap_addr = cap.next_pointer();
            if self.current_cap_addr == 0 {
                return None;
            }
            cap = MsiCapability::new(self.device, self.current_cap_addr);
        }

        let current_cap_addr = self.current_cap_addr;
        self.current_cap_addr = cap.next_pointer();
        Some((current_cap_addr, cap))
    }
}
