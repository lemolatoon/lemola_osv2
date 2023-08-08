pub mod callbacks;
pub mod keyboard;
pub mod mouse;

use core::mem::MaybeUninit;

use spin::Mutex;
use usb_host::{
    ConfigurationDescriptor, DescriptorType, DeviceDescriptor, Direction, Driver, DriverError,
    EndpointDescriptor, RequestCode, RequestDirection, RequestKind, RequestRecipient, RequestType,
    TransferError, TransferType, WValue,
};
use usb_host::{Endpoint as EndpointTrait, USBHost};

use self::keyboard::BootKeyboardDriver;
use self::mouse::MouseDriver;

use super::traits::{AsyncDriver, AsyncUSBHost};

type EndpointSearcher = fn(&[u8]) -> Option<EndpointInfo<'_>>;
pub struct InputOnlyDriver<
    F,
    const MAX_ENDPOINTS: usize,
    const SETTLE_DELAY: usize,
    const CONFIG_BUFFER_LEN: usize,
    const N_IN_TRANSFER_BYTES: usize,
    const MAX_DEVICES: usize,
    const NAME: usize,
> {
    devices: [Option<
        InputOnlyDevice<MAX_ENDPOINTS, SETTLE_DELAY, CONFIG_BUFFER_LEN, N_IN_TRANSFER_BYTES>,
    >; MAX_DEVICES],
    callback: F,
    endpoint_searcher: EndpointSearcher,
}
impl<
        F,
        const MAX_ENDPOINTS: usize,
        const SETTLE_DELAY: usize,
        const CONFIG_BUFFER_LEN: usize,
        const N_IN_TRANSFER_BYTES: usize,
        const MAX_DEVICES: usize,
        const NAME: usize,
    > core::fmt::Debug
    for InputOnlyDriver<
        F,
        MAX_ENDPOINTS,
        SETTLE_DELAY,
        CONFIG_BUFFER_LEN,
        N_IN_TRANSFER_BYTES,
        MAX_DEVICES,
        NAME,
    >
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match NAME {
            0 => "BootKeyboardDriver",
            1 => "MouseDriver",
            _ => "Unknown",
        };
        f.debug_struct(name).finish()
    }
}

impl<
        F,
        const MAX_ENDPOINTS: usize,
        const SETTLE_DELAY: usize,
        const CONFIG_BUFFER_LEN: usize,
        const N_IN_TRANSFER_BYTES: usize,
        const MAX_DEVICES: usize,
        const NAME: usize,
    >
    InputOnlyDriver<
        F,
        MAX_ENDPOINTS,
        SETTLE_DELAY,
        CONFIG_BUFFER_LEN,
        N_IN_TRANSFER_BYTES,
        MAX_DEVICES,
        NAME,
    >
where
    F: FnMut(u8, &[u8]),
{
    /// Create a new driver instance which will call
    /// `callback(address: u8, buffer: &[u8])` when a new keyboard
    /// report is received.
    ///
    /// `address` is the address of the USB device which received the
    /// report and `buffer` is the contents of the report itself.
    pub fn new(callback: F, endpoint_searcher: EndpointSearcher) -> Self {
        #[allow(clippy::uninit_assumed_init)]
        let mut devices: [Option<_>; MAX_DEVICES] = unsafe { MaybeUninit::uninit().assume_init() };
        devices.iter_mut().for_each(|d| *d = None);
        Self {
            devices,
            callback,
            endpoint_searcher,
        }
    }

    pub fn tick_until_running_state(
        &mut self,
        host: &mut dyn usb_host::USBHost,
    ) -> Result<(), DriverError> {
        let mut millis = 0;
        while self.devices.iter().any(|d| {
            d.as_ref()
                .map_or(false, |dd| dd.state != DeviceState::Running)
        }) {
            for device in self.devices.iter_mut().filter_map(|d| d.as_mut()) {
                if device.state == DeviceState::Running {
                    continue;
                }
                if let Err(TransferError::Permanent(e)) =
                    device.fsm(millis, host, &mut self.callback)
                {
                    return Err(DriverError::Permanent(device.addr, e));
                };
                millis += 1;
            }
        }
        Ok(())
    }

    pub fn call_callback_at(&mut self, address: u8, buffer: &[u8]) {
        (self.callback)(address, buffer)
    }

    pub fn endpoints_mut(&mut self, address: u8) -> &mut [Option<Endpoint>; MAX_ENDPOINTS] {
        let device = self
            .devices
            .iter_mut()
            .find(|d| d.as_ref().map_or(false, |dd| dd.addr == address))
            .unwrap()
            .as_mut()
            .unwrap();
        &mut device.endpoints
    }
}

impl<
        F,
        const MAX_ENDPOINTS: usize,
        const SETTLE_DELAY: usize,
        const CONFIG_BUFFER_LEN: usize,
        const N_IN_TRANSFER_BYTES: usize,
        const MAX_DEVICES: usize,
        const NAME: usize,
    > Driver
    for InputOnlyDriver<
        F,
        MAX_ENDPOINTS,
        SETTLE_DELAY,
        CONFIG_BUFFER_LEN,
        N_IN_TRANSFER_BYTES,
        MAX_DEVICES,
        NAME,
    >
where
    F: FnMut(u8, &[u8]),
{
    fn want_device(&self, _device: &DeviceDescriptor) -> bool {
        true
    }

    fn add_device(&mut self, device: DeviceDescriptor, address: u8) -> Result<(), DriverError> {
        if let Some(ref mut d) = self.devices.iter_mut().find(|d| d.is_none()) {
            **d = Some(InputOnlyDevice::new(
                address,
                device.b_max_packet_size,
                self.endpoint_searcher,
            ));
            Ok(())
        } else {
            Err(DriverError::Permanent(address, "out of devices"))
        }
    }

    fn remove_device(&mut self, address: u8) {
        if let Some(ref mut d) = self
            .devices
            .iter_mut()
            .find(|d| d.as_ref().map_or(false, |dd| dd.addr == address))
        {
            **d = None;
        }
    }

    fn tick(&mut self, millis: usize, host: &mut dyn USBHost) -> Result<(), DriverError> {
        for dev in self.devices.iter_mut().filter_map(|d| d.as_mut()) {
            if let Err(TransferError::Permanent(e)) = dev.fsm(millis, host, &mut self.callback) {
                return Err(DriverError::Permanent(dev.addr, e));
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum DeviceState {
    Addressed,
    WaitForSettle(usize),
    GetConfig,
    SetConfig(u8),
    SetProtocol,
    SetIdle,
    Running,
}

pub struct InputOnlyDevice<
    const MAX_ENDPOINTS: usize,
    const SETTLE_DELAY: usize,
    const CONFIG_BUFFER_LEN: usize,
    const N_IN_TRANSFER_BYTES: usize,
> {
    addr: u8,
    ep0: Endpoint,
    endpoints: [Option<Endpoint>; MAX_ENDPOINTS],
    state: DeviceState,
    endpoint_searcher: EndpointSearcher,
}

pub struct EndpointInfo<'a> {
    pub interface_num: u8,
    pub endpoint: &'a EndpointDescriptor,
}

impl<
        const MAX_ENDPOINTS: usize,
        const SETTLE_DELAY: usize,
        const CONFIG_BUFFER_LEN: usize,
        const N_IN_TRANSFER_BYTES: usize,
    > InputOnlyDevice<MAX_ENDPOINTS, SETTLE_DELAY, CONFIG_BUFFER_LEN, N_IN_TRANSFER_BYTES>
{
    fn new(addr: u8, max_packet_size: u8, endpoint_searcher: EndpointSearcher) -> Self {
        const NONE: Option<Endpoint> = None;
        let endpoints: [Option<Endpoint>; MAX_ENDPOINTS] = [NONE; MAX_ENDPOINTS];

        Self {
            addr,
            ep0: Endpoint::new(
                addr,
                0,
                0,
                TransferType::Control,
                Direction::In,
                u16::from(max_packet_size),
            ),
            endpoints,
            state: DeviceState::Addressed,
            endpoint_searcher,
        }
    }

    fn fsm(
        &mut self,
        millis: usize,
        host: &mut dyn USBHost,
        callback: &mut dyn FnMut(u8, &[u8]),
    ) -> Result<(), TransferError> {
        // TODO: either we need another `control_transfer` that
        // doesn't take data, or this `none` value needs to be put in
        // the usb-host layer. None of these options are good.
        let none: Option<&mut [u8]> = None;
        unsafe {
            static mut LAST_STATE: DeviceState = DeviceState::Addressed;
            if LAST_STATE != self.state {
                log::info!("{:?} -> {:?}", LAST_STATE, self.state);
                LAST_STATE = self.state;
            }
        }

        match self.state {
            DeviceState::Addressed => {
                self.state = DeviceState::WaitForSettle(millis + SETTLE_DELAY)
            }

            DeviceState::WaitForSettle(until) => {
                // TODO: This seems unnecessary. We're not using the
                // device descriptor at all.
                if millis > until {
                    let mut dev_desc: MaybeUninit<DeviceDescriptor> = MaybeUninit::uninit();
                    let buf = unsafe { to_slice_mut(&mut dev_desc) };
                    let len = host.control_transfer(
                        &mut self.ep0,
                        RequestType::from((
                            RequestDirection::DeviceToHost,
                            RequestKind::Standard,
                            RequestRecipient::Device,
                        )),
                        RequestCode::GetDescriptor,
                        WValue::from((0, DescriptorType::Device as u8)),
                        0,
                        Some(buf),
                    )?;
                    assert!(len == core::mem::size_of::<DeviceDescriptor>());
                    self.state = DeviceState::GetConfig
                }
            }

            DeviceState::GetConfig => {
                let mut conf_desc: MaybeUninit<ConfigurationDescriptor> = MaybeUninit::uninit();
                let desc_buf = unsafe { to_slice_mut(&mut conf_desc) };
                let len = host.control_transfer(
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
                )?;
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
                let mut config =
                    unsafe { MaybeUninit::<[u8; CONFIG_BUFFER_LEN]>::uninit().assume_init() };
                if CONFIG_BUFFER_LEN < conf_desc.w_total_length as usize {
                    log::trace!("config descriptor: {:?}", conf_desc);
                    return Err(TransferError::Permanent("config descriptor too large"));
                }
                let config_buf = &mut config[..conf_desc.w_total_length as usize];
                let len = host.control_transfer(
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
                )?;
                assert!(len == conf_desc.w_total_length as usize);
                let EndpointInfo {
                    interface_num,
                    endpoint,
                } = (self.endpoint_searcher)(config_buf).expect("no boot keyboard found");
                log::info!("Boot keyboard found on {:?}", endpoint);

                log::debug!(
                    "dci: {}",
                    (endpoint.b_endpoint_address & 0x7f) * 2 + (endpoint.b_endpoint_address >> 7)
                );
                self.endpoints[0] = Some(Endpoint::new(
                    self.addr,
                    endpoint.b_endpoint_address & 0x7f,
                    interface_num,
                    TransferType::Interrupt,
                    Direction::In,
                    endpoint.w_max_packet_size,
                ));

                // TODO: browse configs and pick the "best" one. But
                // this should always be ok, at least.
                self.state = DeviceState::SetConfig(1)
            }

            DeviceState::SetConfig(config_index) => {
                host.control_transfer(
                    &mut self.ep0,
                    RequestType::from((
                        RequestDirection::HostToDevice,
                        RequestKind::Standard,
                        RequestRecipient::Device,
                    )),
                    RequestCode::SetConfiguration,
                    WValue::from((config_index, 0)),
                    0,
                    none,
                )?;

                self.state = DeviceState::SetProtocol;
            }

            DeviceState::SetProtocol => {
                if let Some(ref ep) = self.endpoints[0] {
                    host.control_transfer(
                        &mut self.ep0,
                        RequestType::from((
                            RequestDirection::HostToDevice,
                            RequestKind::Class,
                            RequestRecipient::Interface,
                        )),
                        RequestCode::SetInterface,
                        WValue::from((0, 0)),
                        u16::from(ep.interface_num),
                        None,
                    )?;

                    self.state = DeviceState::SetIdle;
                } else {
                    return Err(TransferError::Permanent("no boot keyboard"));
                }
            }

            DeviceState::SetIdle => {
                host.control_transfer(
                    &mut self.ep0,
                    RequestType::from((
                        RequestDirection::HostToDevice,
                        RequestKind::Class,
                        RequestRecipient::Interface,
                    )),
                    RequestCode::GetInterface,
                    WValue::from((0, 0)),
                    0,
                    none,
                )?;
                self.state = DeviceState::Running;
            }

            DeviceState::Running => {
                if let Some(ref mut ep) = self.endpoints[0] {
                    let mut b: [u8; N_IN_TRANSFER_BYTES] = [0; N_IN_TRANSFER_BYTES];
                    match host.in_transfer(ep, &mut b) {
                        Err(TransferError::Permanent(msg)) => {
                            log::error!("reading report: {}", msg);
                            return Err(TransferError::Permanent(msg));
                        }
                        Err(TransferError::Retry(_)) => return Ok(()),
                        Ok(_) => {
                            callback(self.addr, &b);
                        }
                    }
                } else {
                    return Err(TransferError::Permanent("no boot keyboard"));
                }
            }
        }

        Ok(())
    }

    async fn async_fsm(
        &mut self,
        millis: usize,
        host: &mut dyn AsyncUSBHost,
        callback: &mut dyn FnMut(u8, &[u8]),
    ) -> Result<(), TransferError> {
        // TODO: either we need another `control_transfer` that
        // doesn't take data, or this `none` value needs to be put in
        // the usb-host layer. None of these options are good.
        let none: Option<&mut [u8]> = None;
        unsafe {
            static mut LAST_STATE: DeviceState = DeviceState::Addressed;
            if LAST_STATE != self.state {
                log::info!("{:?} -> {:?}", LAST_STATE, self.state);
                LAST_STATE = self.state;
            }
        }

        match self.state {
            DeviceState::Addressed => {
                self.state = DeviceState::WaitForSettle(millis + SETTLE_DELAY)
            }

            DeviceState::WaitForSettle(until) => {
                // TODO: This seems unnecessary. We're not using the
                // device descriptor at all.
                if millis > until {
                    let mut dev_desc: MaybeUninit<DeviceDescriptor> = MaybeUninit::uninit();
                    let buf = unsafe { to_slice_mut(&mut dev_desc) };
                    let len = host
                        .control_transfer(
                            &mut self.ep0,
                            RequestType::from((
                                RequestDirection::DeviceToHost,
                                RequestKind::Standard,
                                RequestRecipient::Device,
                            )),
                            RequestCode::GetDescriptor,
                            WValue::from((0, DescriptorType::Device as u8)),
                            0,
                            Some(buf),
                        )
                        .await?;
                    assert!(len == core::mem::size_of::<DeviceDescriptor>());
                    self.state = DeviceState::GetConfig
                }
            }

            DeviceState::GetConfig => {
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
                let EndpointInfo {
                    interface_num,
                    endpoint,
                } = (self.endpoint_searcher)(config_buf).expect("no boot keyboard found");
                log::info!("Boot keyboard found on {:?}", endpoint);

                log::debug!(
                    "dci: {}",
                    (endpoint.b_endpoint_address & 0x7f) * 2 + (endpoint.b_endpoint_address >> 7)
                );
                self.endpoints[0] = Some(Endpoint::new(
                    self.addr,
                    endpoint.b_endpoint_address & 0x7f,
                    interface_num,
                    TransferType::Interrupt,
                    Direction::In,
                    endpoint.w_max_packet_size,
                ));

                // TODO: browse configs and pick the "best" one. But
                // this should always be ok, at least.
                self.state = DeviceState::SetConfig(1)
            }

            DeviceState::SetConfig(config_index) => {
                host.control_transfer(
                    &mut self.ep0,
                    RequestType::from((
                        RequestDirection::HostToDevice,
                        RequestKind::Standard,
                        RequestRecipient::Device,
                    )),
                    RequestCode::SetConfiguration,
                    WValue::from((config_index, 0)),
                    0,
                    none,
                )
                .await?;

                self.state = DeviceState::SetProtocol;
            }

            DeviceState::SetProtocol => {
                if let Some(ref ep) = self.endpoints[0] {
                    host.control_transfer(
                        &mut self.ep0,
                        RequestType::from((
                            RequestDirection::HostToDevice,
                            RequestKind::Class,
                            RequestRecipient::Interface,
                        )),
                        RequestCode::SetInterface,
                        WValue::from((0, 0)),
                        u16::from(ep.interface_num),
                        None,
                    )
                    .await?;

                    self.state = DeviceState::SetIdle;
                } else {
                    return Err(TransferError::Permanent("no boot keyboard"));
                }
            }

            DeviceState::SetIdle => {
                host.control_transfer(
                    &mut self.ep0,
                    RequestType::from((
                        RequestDirection::HostToDevice,
                        RequestKind::Class,
                        RequestRecipient::Interface,
                    )),
                    RequestCode::GetInterface,
                    WValue::from((0, 0)),
                    0,
                    none,
                )
                .await?;
                self.state = DeviceState::Running;
            }

            DeviceState::Running => {
                if let Some(ref mut ep) = self.endpoints[0] {
                    let mut b: [u8; N_IN_TRANSFER_BYTES] = [0; N_IN_TRANSFER_BYTES];
                    match host.in_transfer(ep, &mut b).await {
                        Err(TransferError::Permanent(msg)) => {
                            log::error!("reading report: {}", msg);
                            return Err(TransferError::Permanent(msg));
                        }
                        Err(TransferError::Retry(_)) => return Ok(()),
                        Ok(_) => {
                            callback(self.addr, &b);
                        }
                    }
                } else {
                    return Err(TransferError::Permanent("no boot keyboard"));
                }
            }
        }

        Ok(())
    }

    pub fn endpoints(&self) -> &[Option<Endpoint>] {
        &self.endpoints
    }
}

impl<
        F,
        const MAX_ENDPOINTS: usize,
        const SETTLE_DELAY: usize,
        const CONFIG_BUFFER_LEN: usize,
        const N_IN_TRANSFER_BYTES: usize,
        const MAX_DEVICES: usize,
        const NAME: usize,
    > AsyncDriver
    for InputOnlyDriver<
        F,
        MAX_ENDPOINTS,
        SETTLE_DELAY,
        CONFIG_BUFFER_LEN,
        N_IN_TRANSFER_BYTES,
        MAX_DEVICES,
        NAME,
    >
where
    F: FnMut(u8, &[u8]),
{
    fn want_device(&self, device: &DeviceDescriptor) -> bool {
        Driver::want_device(self, device)
    }

    fn add_device(&mut self, device: DeviceDescriptor, address: u8) -> Result<(), DriverError> {
        Driver::add_device(self, device, address)
    }

    fn remove_device(&mut self, address: u8) {
        Driver::remove_device(self, address)
    }

    async fn tick(
        &mut self,
        millis: usize,
        host: &mut (dyn AsyncUSBHost + Send + Sync),
    ) -> Result<(), DriverError> {
        for dev in self.devices.iter_mut().filter_map(|d| d.as_mut()) {
            if let Err(TransferError::Permanent(e)) =
                dev.async_fsm(millis, host, &mut self.callback).await
            {
                return Err(DriverError::Permanent(dev.addr, e));
            }
        }
        Ok(())
    }
}

unsafe fn to_slice_mut<T>(v: &mut T) -> &mut [u8] {
    let ptr = v as *mut T as *mut u8;
    let len = core::mem::size_of::<T>();
    core::slice::from_raw_parts_mut(ptr, len)
}

pub struct Endpoint {
    address: u8,
    endpoint_num: u8,
    interface_num: u8,
    transfer_type: TransferType,
    direction: Direction,
    max_packet_size: u16,
    in_toggle: bool,
    out_toggle: bool,
}

impl Endpoint {
    fn new(
        address: u8,
        endpoint_num: u8,
        interface_num: u8,
        transfer_type: TransferType,
        direction: Direction,
        max_packet_size: u16,
    ) -> Self {
        Self {
            address,
            endpoint_num,
            interface_num,
            transfer_type,
            direction,
            max_packet_size,
            in_toggle: false,
            out_toggle: false,
        }
    }
}

impl EndpointTrait for Endpoint {
    fn address(&self) -> u8 {
        self.address
    }

    fn endpoint_num(&self) -> u8 {
        self.endpoint_num
    }

    fn transfer_type(&self) -> TransferType {
        self.transfer_type
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn max_packet_size(&self) -> u16 {
        self.max_packet_size
    }

    fn in_toggle(&self) -> bool {
        self.in_toggle
    }

    fn set_in_toggle(&mut self, toggle: bool) {
        self.in_toggle = toggle
    }

    fn out_toggle(&self) -> bool {
        self.out_toggle
    }

    fn set_out_toggle(&mut self, toggle: bool) {
        self.out_toggle = toggle
    }
}

macro_rules! add_device {
    ($func_name:ident, $device:ident, $err:expr) => {
        pub fn $func_name(
            &self,
            slot_id: usize,
            device_descriptor: DeviceDescriptor,
            addr: u8,
        ) -> Result<(), DriverError> {
            let mut device = self.$device.lock();
            if Driver::want_device(&device.driver, &device_descriptor) {
                device.slot_id = Some(slot_id);
                return Driver::add_device(&mut device.driver, device_descriptor, addr);
            }

            Err(DriverError::Permanent(addr, $err))
        }
    };
}

#[macro_export]
macro_rules! tick {
    ($class_drivers:expr, $controller:expr, $millis:expr) => {
        use usb_host::Driver;
        macro_rules! tick_device {
            ($device:ident) => {
                if let Some(slot_id) = $class_drivers.$device().0 {
                    if let Some(host) = $controller.usb_device_host_at(slot_id) {
                        $class_drivers.$device().1.tick($millis, host).unwrap();
                    }
                }
            };
        }
        tick_device!(mouse);
        tick_device!(keyboard);
    };
}

#[derive(Debug)]
pub struct DriverInfo<T: AsyncDriver> {
    pub slot_id: Option<usize>,
    pub driver: T,
}

#[derive(Debug)]
pub struct ClassDriverManager<MF, KF>
where
    MF: Fn(u8, &[u8]),
    KF: Fn(u8, &[u8]),
{
    mouse: Mutex<DriverInfo<MouseDriver<MF>>>,
    keyboard: Mutex<DriverInfo<BootKeyboardDriver<KF>>>,
}

impl<MF, KF> ClassDriverManager<MF, KF>
where
    MF: Fn(u8, &[u8]),
    KF: Fn(u8, &[u8]),
{
    pub fn new(mouse_callback: MF, keyboard_callback: KF) -> Self {
        let mouse = DriverInfo {
            slot_id: None,
            driver: MouseDriver::new_mouse(mouse_callback),
        };
        let mouse = Mutex::new(mouse);
        let keyboard = DriverInfo {
            slot_id: None,
            driver: BootKeyboardDriver::new_boot_keyboard(keyboard_callback),
        };
        let keyboard = Mutex::new(keyboard);
        Self { mouse, keyboard }
    }

    // pub fn tick<'a>(
    //     &mut self,
    //     millis: usize,
    //     mut get_host: impl FnMut(usize) -> Option<&'a mut dyn usb_host::USBHost>, // slot_id to host
    // ) -> Result<(), DriverError> {
    //     macro_rules! tick_device {
    //         ($device:ident) => {
    //             let device = self.$device.lock();
    //             if let Some(slot_id) = device.address {
    //                 if let Some(host) = get_host(slot_id) {
    //                     Driver::tick(&mut self.$device.1, millis, host)?;
    //                 }
    //             }
    //         };
    //     }
    //     tick_device!(mouse);
    //     tick_device!(keyboard);
    //     Ok(())
    // }

    // pub fn driver_at(&mut self, slot_id: usize) -> Mutex<&mut dyn Driver> {
    //     let mouse = self.mouse.lock();
    //     if let Some(slot) = mouse.slot_id {
    //         if slot == slot_id {
    //             return Some(&mut self.mouse.1);
    //         }
    //     }
    //     if let Some(slot) = self.keyboard.0 {
    //         if slot == slot_id {
    //             return Some(&mut self.keyboard.1);
    //         }
    //     }
    //     None
    // }

    pub fn tick_at(
        &mut self,
        slot_id: usize,
        millis: usize,
        host: &mut dyn usb_host::USBHost,
    ) -> Result<(), DriverError> {
        // if let Some(driver) = self.driver_at(slot_id) {
        //     driver.tick(millis, host)?;
        // }

        // Ok(())
        unimplemented!()
    }

    pub fn mouse(&self) -> &Mutex<DriverInfo<MouseDriver<MF>>> {
        &self.mouse
    }

    pub fn keyboard(&self) -> &Mutex<DriverInfo<BootKeyboardDriver<KF>>> {
        &self.keyboard
    }

    add_device!(add_mouse_device, mouse, "Mouse device not wanted");

    add_device!(add_keyboard_device, keyboard, "Keyboard device not wanted");
}
