use xhci::ring::trb::{self};

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct TrbRaw([u32; 4]);

impl TrbRaw {
    pub fn new_unchecked(raw: [u32; 4]) -> Self {
        TrbRaw(raw)
    }

    pub fn cycle_bit(&self) -> bool {
        unsafe { core::mem::transmute::<_, trb::Link>(self.clone().into_raw()) }.cycle_bit()
    }

    /// # Safety
    /// `ptr` must be a valid pointer as `[u32; 4]`.
    pub unsafe fn new_from_ptr(ptr: *const [u32; 4]) -> Self {
        Self::new_unchecked(*ptr)
    }

    pub fn into_raw(self) -> [u32; 4] {
        self.0
    }

    pub fn write_in_order(&mut self, another: Self) {
        for (dst, src) in self.0.iter_mut().zip(another.into_raw()) {
            unsafe { (dst as *mut u32).write_volatile(src) };
        }
    }
}

impl TryFrom<TrbRaw> for trb::event::Allowed {
    type Error = TrbRaw;

    fn try_from(value: TrbRaw) -> Result<Self, Self::Error> {
        value.into_raw().try_into().map_err(TrbRaw::new_unchecked)
    }
}

impl TryFrom<TrbRaw> for trb::transfer::Allowed {
    type Error = TrbRaw;

    fn try_from(value: TrbRaw) -> Result<Self, Self::Error> {
        value.into_raw().try_into().map_err(TrbRaw::new_unchecked)
    }
}

impl TryFrom<TrbRaw> for trb::command::Allowed {
    type Error = TrbRaw;

    fn try_from(value: TrbRaw) -> Result<Self, Self::Error> {
        value.into_raw().try_into().map_err(TrbRaw::new_unchecked)
    }
}
