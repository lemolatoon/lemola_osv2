use core::mem::MaybeUninit;

use kernel_lib::{await_sync, futures::yield_pending};
use usb_host::{
    ConfigurationDescriptor, DescriptorType, Direction, DriverError, RequestCode, RequestDirection,
    RequestKind, RequestRecipient, RequestType, TransferError, TransferType, WValue,
};

use crate::usb::{
    descriptor::{DescriptorIter, DescriptorRef, HubDescriptor},
    traits::{AsyncDriver, AsyncUSBHost},
};

use super::Endpoint;

const MAX_DEVICES: usize = 127;

#[derive(Debug)]
pub struct HubDriver {
    devices: [Option<HubDevice>; MAX_DEVICES],
}

impl Default for HubDriver {
    fn default() -> Self {
        Self::new()
    }
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
    Addressed,
    GetConfig,
    SetConfig,
    GetHubDescriptor,
    InitPort(u8),
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitPortState {}

// The maximum size configuration descriptor we can handle.
const CONFIG_BUFFER_LEN: usize = 256;
#[derive(Debug)]
struct HubDevice {
    pub state: HubState,
    address: u8,
    ep0: Endpoint,
    config_descriptor: Option<ConfigurationDescriptor>,
    number_of_ports: u8,
    power_on_2_power_good: u8,
}
impl HubDevice {
    fn new(address: u8, max_packet_size: u8) -> HubDevice {
        Self {
            state: HubState::Addressed,
            address,
            ep0: Endpoint::new(
                address,
                0,
                0,
                TransferType::Control,
                Direction::In,
                u16::from(max_packet_size),
            ),
            config_descriptor: None,
            number_of_ports: 0,
            power_on_2_power_good: 0,
        }
    }

    async fn fsm(
        &mut self,
        _millis: usize,
        host: &mut (dyn AsyncUSBHost + Send + Sync),
    ) -> Result<(), TransferError> {
        // https://forum.osdev.org/viewtopic.php?f=1&t=37441

        // TODO: either we need another `control_transfer` that
        // doesn't take data, or this `none` value needs to be put in
        // the usb-host layer. None of these options are good.
        let none: Option<&mut [u8]> = None;
        unsafe {
            static mut LAST_STATE: HubState = HubState::Addressed;
            if LAST_STATE != self.state {
                log::info!("{:?} -> {:?}", LAST_STATE, self.state);
                LAST_STATE = self.state;
            }
        }
        match self.state {
            HubState::Addressed => {
                // do nothing first time
                self.state = HubState::GetConfig;
            }
            HubState::GetConfig => {
                let mut conf_desc: MaybeUninit<ConfigurationDescriptor> = MaybeUninit::uninit();
                let desc_buf = unsafe { to_slice_mut(&mut conf_desc) };
                let len = host
                    .control_transfer(
                        &mut self.ep0,
                        RequestType::from((
                            RequestDirection::DeviceToHost,
                            RequestKind::Standard,
                            RequestRecipient::Device,
                        )),
                        RequestCode::GetDescriptor,
                        WValue::from((0, DescriptorType::Configuration as u8)),
                        0,
                        Some(desc_buf),
                    )
                    .await?;
                assert!(len == core::mem::size_of::<ConfigurationDescriptor>());
                let conf_desc = unsafe { conf_desc.assume_init() };

                if (conf_desc.w_total_length as usize) > CONFIG_BUFFER_LEN {
                    log::trace!("config descriptor: {:?}", conf_desc);
                    return Err(TransferError::Permanent("config descriptor too large"));
                }

                // TODO: do a real allocation later. For now, keep a
                // large-ish static buffer and take an appropriately
                // sized slice into it for the transfer.
                #[allow(clippy::uninit_assumed_init)]
                #[allow(invalid_value)]
                let mut config =
                    unsafe { MaybeUninit::<[u8; CONFIG_BUFFER_LEN]>::uninit().assume_init() };
                if CONFIG_BUFFER_LEN < conf_desc.w_total_length as usize {
                    log::trace!("config descriptor: {:?}", conf_desc);
                    return Err(TransferError::Permanent("config descriptor too large"));
                }
                let config_buf = &mut config[..conf_desc.w_total_length as usize];
                let len = host
                    .control_transfer(
                        &mut self.ep0,
                        RequestType::from((
                            RequestDirection::DeviceToHost,
                            RequestKind::Standard,
                            RequestRecipient::Device,
                        )),
                        RequestCode::GetDescriptor,
                        WValue::from((0, DescriptorType::Configuration as u8)),
                        0,
                        Some(config_buf),
                    )
                    .await?;
                assert!(len == conf_desc.w_total_length as usize);

                for descriptor in DescriptorIter::new(config_buf) {
                    match descriptor {
                        DescriptorRef::Configuration(conf_desc) => {
                            log::debug!("config descriptor: {:?}", conf_desc);
                            self.config_descriptor = Some(*conf_desc);
                        }
                        DescriptorRef::Interface(_) => {}
                        DescriptorRef::Endpoint(_) => {}
                        DescriptorRef::Unknown => {}
                    }
                }

                self.state = HubState::SetConfig;
            }
            HubState::SetConfig => {
                // https://github.com/foliagecanine/tritium-os/blob/d8b78298f828c0745a480d309aceb4fd503c421f/kernel/usb/usbhub.c#L63
                log::debug!("Initializing hub at {}", self.address);
                let config_value = self
                    .config_descriptor
                    .as_ref()
                    .unwrap()
                    .b_configuration_value;
                let mut w_value = WValue::default();
                w_value.set_w_value_lo(config_value);

                host.control_transfer(
                    &mut self.ep0,
                    RequestType::from((
                        RequestDirection::HostToDevice,
                        RequestKind::Standard,
                        RequestRecipient::Device,
                    )),
                    RequestCode::SetConfiguration,
                    w_value,
                    0,
                    none,
                )
                .await?;

                host.register_hub(self.address).await.unwrap();

                self.state = HubState::GetHubDescriptor;
            }
            HubState::GetHubDescriptor => {
                // USB2.0 spec
                // 11.24.1 Standard Requests
                // 11.24.2.5 Get Hub Descriptor

                // 10100000B
                let type_ = RequestType::from((
                    RequestDirection::DeviceToHost,
                    RequestKind::Class,
                    RequestRecipient::Device,
                )); // 0xA0
                assert_eq!(unsafe { core::mem::transmute::<_, u8>(type_) }, 0xA0);

                // Descriptor Type and Descriptor Index
                // 11.22.2.1 Hub Descriptor
                // Descriptor Type: 29H for hub descriptor
                // All hubs are required to implement one hub descriptor, with descriptor index zero.
                let w_value = WValue::from((0, 0x29)); // 0x2900
                assert_eq!(unsafe { core::mem::transmute::<_, u16>(w_value) }, 0x2900);

                let mut hub_descriptor = HubDescriptor::default();
                let buf = unsafe { to_slice_mut(&mut hub_descriptor) };
                host.control_transfer(
                    &mut self.ep0,
                    type_,
                    RequestCode::GetDescriptor,
                    w_value,
                    0,
                    Some(buf),
                )
                .await?;

                log::debug!("hub descriptor: {:?}", hub_descriptor);
                self.number_of_ports = hub_descriptor.b_nbr_ports;
                self.power_on_2_power_good = hub_descriptor.b_pwr_on_2_pwr_good;
                self.state = HubState::InitPort(0);
            }
            HubState::InitPort(port_index) if port_index < self.number_of_ports => {
                sleep(5000);
                // 11.24.1 Standard Requests

                // 11.24.2.7.1.6 PORT_POWER
                // SET_FEATURE / PORT_POWER

                // 00100011B
                let request_type = RequestType::from((
                    RequestDirection::HostToDevice,
                    RequestKind::Class,
                    RequestRecipient::Other,
                )); // 0x23
                assert_eq!(unsafe { core::mem::transmute::<_, u8>(request_type) }, 0x23);

                // Feature Selector
                let mut w_value = WValue::default();
                w_value.set_w_value_lo(PortFeatureSelector::PortPower as u8);
                let w_index = port_index as u16 + 1;

                host.control_transfer(
                    &mut self.ep0,
                    request_type,
                    RequestCode::SetFeature,
                    w_value,
                    w_index,
                    none,
                )
                .await?;

                yield_pending().await;
                sleep(self.power_on_2_power_good as usize * 2);
                log::debug!("port[{}] powered on", port_index);

                // 11.24.2.2 Clear Port Feature
                // CLEAR_FEATURE / PORT_CONNECTION

                // 00100011B
                let request_type = RequestType::from((
                    RequestDirection::HostToDevice,
                    RequestKind::Class,
                    RequestRecipient::Other,
                )); // 0x23
                assert_eq!(unsafe { core::mem::transmute::<_, u8>(request_type) }, 0x23);

                // Feature Selector
                let mut w_value = WValue::default();
                // 11.24.2.7.2.1 C_PORT_CONNECTION
                // TODO:w_valueは selector | port じゃないのか？
                w_value.set_w_value_lo(PortFeatureSelector::CPortConnection as u8);
                let w_index = port_index as u16 + 1;

                host.control_transfer(
                    &mut self.ep0,
                    request_type,
                    RequestCode::ClearFeature,
                    w_value,
                    w_index,
                    None,
                )
                .await?;

                yield_pending().await;
                log::debug!("port[{}] CPortConnection cleared", port_index);

                // 11.24.2.7 Get Port Status
                // 10100011B
                let request_type = RequestType::from((
                    RequestDirection::DeviceToHost,
                    RequestKind::Class,
                    RequestRecipient::Other,
                )); // 0xa3
                assert_eq!(unsafe { core::mem::transmute::<_, u8>(request_type) }, 0xa3);

                let w_value = WValue::default();
                let w_index = port_index as u16 + 1;
                // 11.24.2.7.1 Port Status Bits
                let mut status = [0u8; 4];

                host.control_transfer(
                    &mut self.ep0,
                    request_type,
                    RequestCode::GetStatus,
                    w_value,
                    w_index,
                    Some(&mut status),
                )
                .await?;

                if status[0] & 0x01 == 0 {
                    log::debug!("port[{}] is not connected", port_index);
                    self.state = HubState::InitPort(port_index + 1);
                    return Ok(());
                } else {
                    log::debug!("port[{}] is connected!!", port_index);
                }

                yield_pending().await;

                self.state = HubState::InitPort(port_index + 1);
            }
            HubState::InitPort(_) => {
                // all ports initialized
                self.state = HubState::Running;
            }
            HubState::Running => {}
        }

        Ok(())
    }
}

unsafe fn to_slice_mut<T>(v: &mut T) -> &mut [u8] {
    let ptr = v as *mut T as *mut u8;
    let len = core::mem::size_of::<T>();
    core::slice::from_raw_parts_mut(ptr, len)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HubFeatureSelector {
    CHubLocalPower = 0,
    CHubOverCurrent = 1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortFeatureSelector {
    PortConnection = 0,
    PortEnable = 1,
    PortSuspend = 2,
    PortOverCurrent = 3,
    PortReset = 4,
    PortPower = 8,
    PortLowSpeed = 9,
    CPortConnection = 16,
    CPortEnable = 17,
    CPortSuspend = 18,
    CPortOverCurrent = 19,
    CPortReset = 20,
    PortTest = 21,
    PortIndicator = 22,
}

fn sleep(millis: usize) {
    for i in 0..(millis * 1000) {
        let mut count = 0usize;
        unsafe {
            (&mut count as *mut usize).write_volatile(i);
        }
    }
}
