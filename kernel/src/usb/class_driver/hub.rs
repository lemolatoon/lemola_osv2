use core::mem::MaybeUninit;

use kernel_lib::await_sync;
use usb_host::{Driver, DriverError, TransferError, USBHost};

use crate::usb::traits::{AsyncDriver, AsyncUSBHost};

const MAX_DEVICES: usize = 127;

#[derive(Debug)]
pub struct HubDriver {
    devices: [Option<HubDevice>; MAX_DEVICES],
}

impl HubDriver {
    pub fn new() -> Self {
        #[allow(clippy::uninit_assumed_init)]
        #[allow(invalid_value)]
        let mut devices: [Option<_>; MAX_DEVICES] = unsafe { MaybeUninit::uninit().assume_init() };
        devices.iter_mut().for_each(|d| *d = None);
        Self { devices }
    }

    pub fn tick_until_running_state(
        &mut self,
        host: &mut (dyn AsyncUSBHost + Send + Sync),
    ) -> Result<(), DriverError> {
        let mut millis = 0;
        log::info!("tick_until_running_state");
        while self
            .devices
            .iter()
            .any(|d| d.as_ref().map_or(false, |dd| dd.state != HubState::Running))
        {
            millis += 1;
            if millis % 1_000_000 != 0 {
                continue;
            }
            for device in self.devices.iter_mut().filter_map(|d| d.as_mut()) {
                if device.state == HubState::Running {
                    continue;
                }
                if let Err(TransferError::Permanent(e)) = await_sync!(device.fsm(millis, host)) {
                    return Err(DriverError::Permanent(device.address, e));
                };
                millis += 1;
            }
        }
        Ok(())
    }
}

impl AsyncDriver for HubDriver {
    fn want_device(&self, _device: &usb_host::DeviceDescriptor) -> bool {
        true
    }

    fn add_device(
        &mut self,
        device: usb_host::DeviceDescriptor,
        address: u8,
    ) -> Result<(), usb_host::DriverError> {
        if let Some(ref mut d) = self.devices.iter_mut().find(|d| d.is_none()) {
            **d = Some(HubDevice::new(address, device.b_max_packet_size));
            Ok(())
        } else {
            Err(DriverError::Permanent(address, "out of devices"))
        }
    }

    fn remove_device(&mut self, address: u8) {
        if let Some(ref mut d) = self
            .devices
            .iter_mut()
            .find(|d| d.as_ref().map_or(false, |dd| dd.address == address))
        {
            **d = None;
        }
    }

    async fn tick(
        &mut self,
        millis: usize,
        usbhost: &mut (dyn AsyncUSBHost + Send + Sync),
    ) -> Result<(), usb_host::DriverError> {
        for dev in self.devices.iter_mut().filter_map(|d| d.as_mut()) {
            if let Err(TransferError::Permanent(e)) = dev.fsm(millis, usbhost).await {
                return Err(DriverError::Permanent(dev.address, e));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HubState {
    Init,
    Running,
}

#[derive(Debug)]
struct HubDevice {
    pub state: HubState,
    address: u8,
    max_packet_size: u8,
}
impl HubDevice {
    fn new(address: u8, b_max_packet_size: u8) -> HubDevice {
        Self {
            state: HubState::Init,
            address,
            max_packet_size: b_max_packet_size,
        }
    }

    async fn fsm(
        &mut self,
        millis: usize,
        usbhost: &mut (dyn AsyncUSBHost + Send + Sync),
    ) -> Result<(), TransferError> {
        log::warn!(
            "actual HubDevice::fsm implementation is not implemented, just set state to Running"
        );

        self.state = HubState::Running;

        Ok(())
    }
}
