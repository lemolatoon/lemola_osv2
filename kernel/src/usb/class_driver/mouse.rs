use crate::usb::descriptor::{DescriptorIter, DescriptorRef};

use super::{EndpointInfo, InputOnlyDriver};

// How long to wait before talking to the device again after setting
// its address. cf §9.2.6.3 of USB 2.0
const SETTLE_DELAY: usize = 2;

// How many total devices this driver can support.
const MAX_DEVICES: usize = 32;

// And how many endpoints we can support per-device.
const MAX_ENDPOINTS: usize = 2;

// The maximum size configuration descriptor we can handle.
const CONFIG_BUFFER_LEN: usize = 256;

const N_IN_TRANSFER_BYTES: usize = 3;

/// Boot protocol keyboard driver for USB hosts.
pub type MouseDriver<F> = InputOnlyDriver<
    F,
    MAX_ENDPOINTS,
    SETTLE_DELAY,
    CONFIG_BUFFER_LEN,
    N_IN_TRANSFER_BYTES,
    MAX_DEVICES,
    "MouseDriver",
>;

impl<F> MouseDriver<F>
where
    F: FnMut(u8, &[u8]),
{
    /// Create a new driver.
    pub fn new_mouse(callback: F) -> Self {
        Self::new(callback, ep_for_mouse)
    }
}

/// If a mouse is found, return its interface number
/// and endpoint.
fn ep_for_mouse(buf: &[u8]) -> Option<EndpointInfo<'_>> {
    let parser = DescriptorIter::new(buf);
    let mut interface_found = None;
    for desc in parser {
        if let DescriptorRef::Interface(idesc) = desc {
            if idesc.b_interface_class == 0x03
                && idesc.b_interface_sub_class == 0x01
                && idesc.b_interface_protocol == 0x02
            {
                interface_found = Some(idesc.b_interface_number);
            } else {
                interface_found = None;
            }
        } else if let DescriptorRef::Endpoint(edesc) = desc {
            if let Some(interface_num) = interface_found {
                return Some(EndpointInfo {
                    interface_num,
                    endpoint: edesc,
                });
            }
        }
    }
    None
}
