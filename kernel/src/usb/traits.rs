extern crate alloc;
use alloc::boxed::Box;
use async_trait::async_trait;
use usb_host::{DeviceDescriptor, DriverError};

#[async_trait]
pub trait AsyncUSBHost {
    async fn control_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        bm_request_type: usb_host::RequestType,
        b_request: usb_host::RequestCode,
        w_value: usb_host::WValue,
        w_index: u16,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, usb_host::TransferError>;

    async fn in_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        buf: &mut [u8],
    ) -> Result<usize, usb_host::TransferError>;

    async fn out_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        buf: &[u8],
    ) -> Result<usize, usb_host::TransferError>;

    async fn register_hub(&mut self, hub_address: u8) -> Result<(), usb_host::TransferError>;

    async fn assign_address(
        &mut self,
        hub_address: u8,
        port_index: u8,
        device_is_low_speed: bool,
    ) -> Result<u8, usb_host::TransferError>;
}

pub trait AsyncDriver {
    fn want_device(&self, device: &DeviceDescriptor) -> bool;

    fn add_device(&mut self, device: DeviceDescriptor, address: u8) -> Result<(), DriverError>;

    fn remove_device(&mut self, address: u8);

    async fn tick(
        &mut self,
        millis: usize,
        host: &mut (dyn AsyncUSBHost + Send + Sync),
    ) -> Result<(), DriverError>;
}
