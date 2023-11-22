pub mod task;
extern crate alloc;
use core::ffi::c_void;

use conquer_once::spin::OnceCell;
use kernel_lib::futures::yield_pending;

use crate::{
    alloc::alloc::GlobalAllocator,
    interrupts::InterruptVector,
    memory::MemoryMapper,
    pci, serial_println,
    usb::class_driver::{callbacks::CallbackType, ClassDriverManager},
};

use self::controller::XhciController;

pub mod command_ring;
pub mod controller;
pub mod device_manager;
pub mod event_ring;
pub mod port;
pub mod transfer_ring;
pub mod trb;
pub mod user_event_ring;

pub type Controller<MF, KF> = XhciController<MemoryMapper, &'static GlobalAllocator, MF, KF>;
pub type Xhc = Controller<CallbackType, CallbackType>;

static XHC: OnceCell<Xhc> = OnceCell::uninit();
pub fn get_xhc() -> &'static Xhc {
    XHC.get().expect("XHC not initialized")
}

const LOCAL_APIC_ADDRESS: usize = 0xfee0_0000;
pub fn read_local_apic_id(offset: usize) -> u8 {
    unsafe { ((LOCAL_APIC_ADDRESS + offset) as *mut u32).read_volatile() as u8 }
}

pub fn write_local_apic_id(offset: usize, data: u32) {
    unsafe { ((LOCAL_APIC_ADDRESS + offset) as *mut u32).write_volatile(data) };
}

pub async fn process_user_event() {
    let xhc = get_xhc();
    loop {
        xhc.process_user_event().await;
        yield_pending().await;
    }
}

pub async fn process_already_popped() {
    let xhc = get_xhc();
    loop {
        xhc.process_once_received().await;
        yield_pending().await;
    }
}

pub async fn process_event() {
    let xhc = get_xhc();
    loop {
        xhc.process_event().await;
        yield_pending().await;
    }
}

pub async fn poll_forever<MF, KF>(controller: &Controller<MF, KF>)
where
    MF: Fn(u8, &[u8]) + 'static,
    KF: Fn(u8, &[u8]) + 'static,
{
    loop {
        {
            if controller.pending_already_popped_queue() {
                controller.process_once_received().await;
                yield_pending().await;
                yield_pending().await;
            }
            if controller.pending_event() {
                controller.process_event().await;
                yield_pending().await;
                yield_pending().await;
            }

            controller.process_user_event().await;
            for _ in 0..100 {
                yield_pending().await;
            }
        }
    }
}

pub async fn tick_mouse_forever<MF, KF>(controller: &Controller<MF, KF>)
where
    MF: Fn(u8, &[u8]) + 'static,
    KF: Fn(u8, &[u8]) + 'static,
{
    let mut count = 0;
    loop {
        // for avoiding deadlock
        // x86_64::instructions::interrupts::disable();
        {
            controller.async_tick_mouse(count).await.unwrap();
        }
        count += 1;
        yield_pending().await;
        // x86_64::instructions::interrupts::enable();
    }
}

pub async fn tick_keyboard_forever<MF, KF>(controller: &Controller<MF, KF>)
where
    MF: Fn(u8, &[u8]) + 'static,
    KF: Fn(u8, &[u8]) + 'static,
{
    let count = 0;
    loop {
        controller.async_tick_keyboard(count).await.unwrap();
        yield_pending().await;
    }
}

pub fn init_xhci_controller(
    class_driver_manager: &'static ClassDriverManager<CallbackType, CallbackType>,
) -> &'static Xhc {
    let devices = crate::pci::register::scan_all_bus();
    for device in &devices {
        serial_println!(
            "vend: {}, class: {}, head: {}",
            device.vendor_id(),
            device.class_code(),
            device.header_type()
        );
    }
    let xhci_device = devices
        .iter()
        .find(|pci_device| {
            pci_device.class_code().is_xhci_controller() && pci_device.vendor_id().is_intel()
        })
        .map_or_else(
            || {
                devices
                    .iter()
                    .find(|pci_device| pci_device.class_code().is_xhci_controller())
            },
            Some,
        )
        .expect("xhci device not found");
    log::info!(
        "xhci device found, {:x}, {:x}, {:x}",
        xhci_device.bus(),
        xhci_device.device(),
        xhci_device.function()
    );
    serial_println!(
        "vend: {}, class: {}, head: {}",
        xhci_device.vendor_id(),
        xhci_device.class_code(),
        xhci_device.header_type()
    );
    let xhc_bar = xhci_device.read_bar(0).unwrap();
    let xhc_mmio_base = xhc_bar & 0xffff_ffff_ffff_fff0; // 下位4bitはBARのフラグ

    // bootstrap processor's id
    let bsp_local_apic_id: u8 = (unsafe { (0xfee00020 as *mut u32).read_volatile() } >> 24) as u8;
    pci::configure_msi_fixed_destination(
        xhci_device,
        bsp_local_apic_id,
        pci::MSITriggerMode::Level,
        pci::MSIDeliveryMode::Fixed,
        InterruptVector::Xhci,
        0,
    );

    log::info!("xhc_mmio_base: {:?}", xhc_mmio_base as *const c_void);
    let memory_mapper = crate::memory::MemoryMapper::new();
    let controller =
        unsafe { XhciController::new(xhc_mmio_base as usize, memory_mapper, class_driver_manager) };
    log::info!("xhc initialized");
    controller.run();

    for port_idx in 0..controller.number_of_ports() {
        let registers = controller.registers();
        let port_register_sets = &registers.port_register_set;
        let is_connected = port_register_sets
            .read_volatile_at(port_idx as usize)
            .portsc
            .current_connect_status();
        drop(registers);
        log::debug!("portsc[{}]: is_connected = {}", port_idx, is_connected);
        if is_connected {
            controller.configure_port_at(port_idx as usize);
        }
    }
    log::debug!("Configured ports");

    XHC.init_once(|| controller);
    get_xhc()
}

pub fn next_route(routing: u32, port: u8) -> u32 {
    // https://github.com/foliagecanine/tritium-os/blob/master/kernel/arch/i386/usb/xhci.c#L845
    let mut shift = 0;
    for _ in 0..4 {
        if routing & (0xf << shift) == 0 {
            log::debug!(
                "next_route: routing = {:x}, port = {}, shift = {}, ret = {:x}",
                routing,
                port,
                shift,
                routing | ((port as u32) << shift)
            );
            return routing | ((port as u32) << shift);
        }
        shift += 4;
    }

    0
}
