use xhci::registers::PortRegisterSet;

#[derive(Debug, Clone)]
pub struct PortConfigureState {
    pub port_config_phase: [PortConfigPhase; 256],
    pub addressing_port_index: Option<usize>,
}

impl PortConfigureState {
    pub fn new() -> Self {
        Self {
            port_config_phase: [PortConfigPhase::NotConnected; 256],
            addressing_port_index: None,
        }
    }

    pub fn is_connected(&self, port_id: usize) -> bool {
        self.port_config_phase[port_id] != PortConfigPhase::NotConnected
    }

    pub fn port_phase_at(&self, port_id: usize) -> PortConfigPhase {
        self.port_config_phase[port_id]
    }

    pub fn start_configuration_at(&mut self, port_idx: usize) {
        self.addressing_port_index = Some(port_idx);
        self.port_config_phase[port_idx] = PortConfigPhase::ResettingPort;
    }
}

impl Default for PortConfigureState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PortConfigPhase {
    NotConnected,
    WaitingAddressed,
    ResettingPort,
    EnablingSlot,
    AddressingDevice,
    InitializingDevice,
    ConfiguringEndpoints,
    Configured,
}

pub struct PortWrapper<'a> {
    port_idx: usize,
    port_register: &'a mut PortRegisterSet,
}

impl<'a> PortWrapper<'a> {
    pub fn new(port_idx: usize, port_register: &'a mut PortRegisterSet) -> Self {
        Self {
            port_idx,
            port_register,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.port_register.portsc.current_connect_status()
    }
}
