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
