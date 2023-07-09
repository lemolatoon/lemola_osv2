use static_assertions::assert_eq_size;
use xhci::ring::trb::{self, transfer};

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct TrbRaw([u32; 4]);

impl TrbRaw {
    pub fn new_unchecked(raw: [u32; 4]) -> Self {
        TrbRaw(raw)
    }

    pub unsafe fn new_from_ptr(ptr: *const [u32; 4]) -> Self {
        Self::new_unchecked(*ptr)
    }

    pub fn into_raw(self) -> [u32; 4] {
        self.0
    }

    pub fn write_in_order(&mut self, another: Self) {
        for (dst, src) in self.0.iter_mut().zip(another.into_raw()) {
            *dst = src;
        }
    }

    pub unsafe fn as_setup_stage_mut(&mut self) -> &mut transfer::SetupStage {
        assert_eq_size!(transfer::SetupStage, TrbRaw);
        core::mem::transmute(self)
    }
}

impl TryFrom<TrbRaw> for trb::event::Allowed {
    type Error = TrbRaw;

    fn try_from(value: TrbRaw) -> Result<Self, Self::Error> {
        value
            .into_raw()
            .try_into()
            .map_err(|raw| TrbRaw::new_unchecked(raw))
    }
}

impl TryFrom<TrbRaw> for trb::transfer::Allowed {
    type Error = TrbRaw;

    fn try_from(value: TrbRaw) -> Result<Self, Self::Error> {
        value
            .into_raw()
            .try_into()
            .map_err(|raw| TrbRaw::new_unchecked(raw))
    }
}

impl TryFrom<TrbRaw> for trb::command::Allowed {
    type Error = TrbRaw;

    fn try_from(value: TrbRaw) -> Result<Self, Self::Error> {
        value
            .into_raw()
            .try_into()
            .map_err(|raw| TrbRaw::new_unchecked(raw))
    }
}
