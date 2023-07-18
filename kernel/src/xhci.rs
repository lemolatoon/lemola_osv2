use core::{cell::OnceCell, ffi::c_void};

use spin::Mutex;

use crate::{
    alloc::alloc::GlobalAllocator,
    interrupts::InterruptVector,
    memory::MemoryMapper,
    pci, serial_println,
    usb::class_driver::callbacks::{self, CallbackType},
};

use self::controller::XhciController;

pub mod command_ring;
pub mod controller;
pub mod device_manager;
pub mod event_ring;
pub mod port;
pub mod transfer_ring;
pub mod trb;

pub static XHC: Mutex<
    OnceCell<XhciController<MemoryMapper, &'static GlobalAllocator, CallbackType, CallbackType>>,
> = Mutex::new(OnceCell::new());

pub fn init_xhci_controller() {
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
    let bsp_local_apic_id: u8 = unsafe { (0xfee00020 as *mut u32).read_volatile() as u8 };
    pci::configure_msi_fixed_destination(
        xhci_device,
        bsp_local_apic_id,
        pci::MSITriggerMode::Level,
        pci::MSIDeliveryMode::Fixed,
        InterruptVector::Xhci,
        0,
    );

    let class_drivers = crate::usb::class_driver::ClassDriverManager::new(
        callbacks::mouse(),
        callbacks::keyboard(),
    );

    log::info!("xhc_mmio_base: {:?}", xhc_mmio_base as *const c_void);
    let memory_mapper = crate::memory::MemoryMapper::new();
    let mut controller =
        unsafe { XhciController::new(xhc_mmio_base as usize, memory_mapper, class_drivers) };
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
        log::debug!("Port {}: is_connected = {}", port_idx, is_connected);
        if is_connected {
            controller.configure_port_at(port_idx as usize);
        }
    }
    log::debug!("Configured ports");

    XHC.lock().get_or_init(|| controller);
}
