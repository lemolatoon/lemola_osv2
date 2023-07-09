use usb_host::{
    DescriptorType, Endpoint, RequestCode, RequestDirection, RequestKind, RequestRecipient,
    RequestType, SetupPacket,
};

#[derive(Clone, Debug, Copy)]
pub struct SetupPacketWrapper(pub SetupPacket);

impl From<SetupPacket> for SetupPacketWrapper {
    fn from(setup_packet: SetupPacket) -> Self {
        Self(setup_packet)
    }
}

impl SetupPacketWrapper {
    pub fn descriptor(descriptor_type: DescriptorType, descriptor_index: u8, len: u16) -> Self {
        let bm_request_type = (
            RequestDirection::DeviceToHost,
            RequestKind::Standard,
            RequestRecipient::Device,
        )
            .into();
        let b_request = RequestCode::GetDescriptor;
        let w_value = (descriptor_index, descriptor_type as u8).into();
        let w_index = 0;
        let w_length = len;
        let setup_packet = SetupPacket {
            bm_request_type,
            b_request,
            w_value,
            w_index,
            w_length,
        };
        setup_packet.into()
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SetupPacketRaw {
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
