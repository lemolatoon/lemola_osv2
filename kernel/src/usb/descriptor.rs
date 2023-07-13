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
pub enum Descriptor {
    Configuration(ConfigurationDescriptor),
    Interface(InterfaceDescriptor),
    Endpoint(EndpointDescriptor),
    Unknown,
}

impl Descriptor {
    /// # Safety
    /// `data` must be a valid descriptor.
    pub unsafe fn new(data: &[u8]) -> Self {
        match DescriptorType::try_from(data[1]) {
            Ok(DescriptorType::Configuration) => Self::Configuration(unsafe {
                data.as_ptr().cast::<ConfigurationDescriptor>().read()
            }),
            Ok(DescriptorType::Interface) => {
                Self::Interface(unsafe { data.as_ptr().cast::<InterfaceDescriptor>().read() })
            }
            Ok(DescriptorType::Endpoint) => {
                Self::Endpoint(unsafe { data.as_ptr().cast::<EndpointDescriptor>().read() })
            }
            desc_ty => {
                log::debug!("Unknown descriptor type: {:?}", desc_ty);
                Self::Unknown
            }
        }
    }
}

impl<'a> Iterator for DescriptorIter<'a> {
    type Item = Descriptor;

    fn next(&mut self) -> Option<Self::Item> {
        if self.read_bytes >= self.data.len() {
            return None;
        }
        let next_descriptor_length = self.data[self.read_bytes] as usize;
        let descriptor = unsafe {
            Descriptor::new(&self.data[self.read_bytes..self.read_bytes + next_descriptor_length])
        };
        self.read_bytes += next_descriptor_length as usize;
        Some(descriptor)
    }
}
