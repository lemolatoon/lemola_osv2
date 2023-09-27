use usb_host::{ConfigurationDescriptor, DescriptorType, EndpointDescriptor, InterfaceDescriptor};

pub struct DescriptorIter<'a> {
    pub data: &'a [u8],
    read_bytes: usize,
}

impl<'a> DescriptorIter<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            read_bytes: 0,
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub enum DescriptorRef<'a> {
    Configuration(&'a ConfigurationDescriptor),
    Interface(&'a InterfaceDescriptor),
    Endpoint(&'a EndpointDescriptor),
    Unknown,
}

impl<'a> From<DescriptorRef<'a>> for Descriptor {
    fn from(value: DescriptorRef<'a>) -> Self {
        match value {
            DescriptorRef::Configuration(configuration) => Self::Configuration(*configuration),
            DescriptorRef::Interface(interface) => Self::Interface(*interface),
            DescriptorRef::Endpoint(endpoint) => Self::Endpoint(*endpoint),
            DescriptorRef::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub enum Descriptor {
    Configuration(ConfigurationDescriptor),
    Interface(InterfaceDescriptor),
    Endpoint(EndpointDescriptor),
    Unknown,
}

impl<'a> DescriptorRef<'a> {
    /// # Safety
    /// `data` must be a valid descriptor.
    pub unsafe fn new(data: &[u8]) -> Self {
        match DescriptorType::try_from(data[1]) {
            Ok(DescriptorType::Configuration) => Self::Configuration(unsafe {
                data.as_ptr()
                    .cast::<ConfigurationDescriptor>()
                    .as_ref()
                    .unwrap_unchecked()
            }),
            Ok(DescriptorType::Interface) => Self::Interface(unsafe {
                data.as_ptr()
                    .cast::<InterfaceDescriptor>()
                    .as_ref()
                    .unwrap_unchecked()
            }),
            Ok(DescriptorType::Endpoint) => Self::Endpoint(unsafe {
                data.as_ptr()
                    .cast::<EndpointDescriptor>()
                    .as_ref()
                    .unwrap_unchecked()
            }),
            desc_ty => {
                log::debug!("Unknown descriptor type: {:?}", desc_ty);
                Self::Unknown
            }
        }
    }
}

impl<'a> Iterator for DescriptorIter<'a> {
    type Item = DescriptorRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.read_bytes >= self.data.len() {
            return None;
        }
        let next_descriptor_length = self.data[self.read_bytes] as usize;
        let descriptor = unsafe {
            DescriptorRef::new(
                &self.data[self.read_bytes..self.read_bytes + next_descriptor_length],
            )
        };
        self.read_bytes += next_descriptor_length;
        Some(descriptor)
    }
}

// USB 2.0 Spec
// 11.23.2.1 Hub Descriptor
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HubDescriptor {
    ///  Number of bytes in this descriptor, including this byte
    pub b_desc_length: u8,
    ///  Descriptor Type, value: 29H for hub descriptor
    pub b_descriptor_type: u8,
    /// Number of downstream facing ports that this hub supports
    pub b_nbr_ports: u8,
    /// D1...D0: Logical Power Switching Mode
    ///
    ///     00: Ganged power switching (all ports’ power at once)
    ///
    ///     01: Individual port power switching
    ///
    ///     1X: Reserved. Used only on 1.0 compliant hubs that implement no power switching
    ///
    /// D2: Identifies a Compound Device
    ///
    ///     0: Hub is not part of a compound device.
    ///
    ///     1: Hub is part of a compound device.
    ///
    /// D4...D3: Over-current Protection Mode
    ///
    ///     00: Global Over-current Protection. The hub reports over-current as a summation of all ports’ current draw, without a breakdown of individual port over-current status.
    ///
    ///     01: Individual Port Over-current Protection. The hub reports over-current on a per-port basis. Each port has an over-current status.
    ///
    ///     1X: No Over-current Protection. This option is allowed only for bus-powered hubs that do not implement over-current protection.
    ///
    ///D6...D5: TT Think TIme
    ///
    ///     00: TT requires at most 8 FS bit times of inter transaction gap on a full-/low-speed downstream bus.
    ///
    ///     01: TT requires at most 16 FS bit times.
    ///
    ///     10: TT requires at most 24 FS bit times.
    ///
    ///     11: TT requires at most 32 FS bit times.
    ///
    /// D7: Port Indicators Supported
    ///
    ///     0: Port Indicators are not supported on its downstream facing ports and the PORT_INDICATOR request has no effect.
    ///
    ///     1: Port Indicators are supported on its downstream facing ports and the PORT_INDICATOR request controls the indicators. See Section 11.5.3.
    ///
    /// D15...D8: Reserved
    pub w_hub_characteristics: u16,
    /// Time (in 2 ms intervals) from the time the power-on
    /// sequence begins on a port until power is good on that
    /// port. The USB System Software uses this value to
    /// determine how long to wait before accessing a
    /// powered-on port.
    pub b_pwr_on_2_pwr_good: u8,
    /// Maximum current requirements of the Hub Controller electronics in mA.
    pub b_hub_contr_current: u8,
    // DeviceRemovable: Variable depending on number of ports on hub
    // PortPwrCtrlMask: Variable depending on number of ports on hub
}

#[allow(clippy::derivable_impls)]
impl Default for HubDescriptor {
    /// 0 cleared HubDescriptor
    fn default() -> Self {
        Self {
            b_desc_length: 0,
            b_descriptor_type: 0,
            b_nbr_ports: 0,
            w_hub_characteristics: 0,
            b_pwr_on_2_pwr_good: 0,
            b_hub_contr_current: 0,
        }
    }
}
