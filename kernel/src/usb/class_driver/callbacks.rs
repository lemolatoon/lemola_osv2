pub fn mouse(address: u8, buf: &[u8]) {
    log::info!("mouse input: {:?}, {:?}", address, buf);
}

pub fn keyboard(address: u8, buf: &[u8]) {
    log::info!("keyboard input: {:?}, {:?}", address, buf);
}
