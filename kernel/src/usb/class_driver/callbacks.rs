use kernel_lib::{
    render::{AtomicVec2D, Vector2D},
    shapes::mouse::MOUSE_CURSOR_SHAPE,
};

use crate::graphics::get_pixcel_writer;

static MOUSE_CURSOR: AtomicVec2D = AtomicVec2D::new(200, 100);

pub fn mouse(_address: u8, buf: &[u8]) {
    let x_diff = buf[1] as i8;
    let y_diff = buf[2] as i8;
    log::debug!("{:?}", [x_diff, y_diff]);
    MOUSE_CURSOR.add(x_diff as isize, y_diff as isize);
    if let Some(pixcel_writer) = get_pixcel_writer() {
        let (mut x, mut y) = MOUSE_CURSOR.into_vec();
        use core::cmp::{max, min};
        x = min(
            max(x, 0),
            pixcel_writer.horizontal_resolution() as isize - 1,
        );
        y = min(max(y, 0), pixcel_writer.vertical_resolution() as isize - 1);
        let vec = Vector2D::new(x as usize, y as usize);
        log::debug!(
            "rendering: {:?} in [{}, {}]",
            vec,
            pixcel_writer.horizontal_resolution(),
            pixcel_writer.vertical_resolution()
        );
        pixcel_writer.fill_shape(vec, &MOUSE_CURSOR_SHAPE);
    };
}

pub fn keyboard(address: u8, buf: &[u8]) {
    log::debug!("keyboard input: {:?}, {:?}", address, buf);
}
