use usb_host::SetupPacket;

#[derive(Clone, Debug, Copy)]
pub struct SetupPacketWrapper(pub SetupPacket);

#[derive(Clone, Debug, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SetupPacketRaw {
    pub bm_request_type: u8,
    pub b_request: u8,
    pub w_value: u16,
    pub w_index: u16,
    pub w_length: u16,
}

impl From<SetupPacket> for SetupPacketRaw {
    fn from(setup_packet: SetupPacket) -> Self {
        let SetupPacket {
            bm_request_type,
            b_request,
            w_value,
            w_index,
            w_length,
        } = setup_packet;
        use core::mem::transmute;
        unsafe {
            Self {
                bm_request_type: transmute(bm_request_type),
                b_request: transmute(b_request),
                w_value: transmute(w_value),
                w_index,
                w_length,
            }
        }
    }
}

impl PartialEq for SetupPacketWrapper {
    fn eq(&self, other: &Self) -> bool {
        Into::<SetupPacketRaw>::into(self.0).eq(&other.0.into())
    }
}

impl Eq for SetupPacketWrapper {}

impl PartialOrd for SetupPacketWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Into::<SetupPacketRaw>::into(self.0).partial_cmp(&other.0.into())
    }
}

impl Ord for SetupPacketWrapper {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        Into::<SetupPacketRaw>::into(self.0).cmp(&other.0.into())
    }
}
