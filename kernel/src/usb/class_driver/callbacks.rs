use kernel_lib::{
    render::{AtomicVec2D, Vector2D},
    shapes::mouse::MOUSE_CURSOR_SHAPE,
};

use crate::{
    graphics::get_pixcel_writer,
    lifegame::{self, frame_buffer_position_to_board_position, CLICKED_POSITION_QUEUE},
    print, print_and_flush,
    usb::class_driver::keyboard,
};

static MOUSE_CURSOR: AtomicVec2D = AtomicVec2D::new(700, 500);

pub type CallbackType = fn(u8, &[u8]);

pub const fn keyboard() -> CallbackType {
    _keyboard
}

pub const fn mouse() -> CallbackType {
    _mouse
}

#[doc(hidden)]
pub fn _mouse(_address: u8, buf: &[u8]) {
    let x_diff = buf[1] as i8;
    let y_diff = buf[2] as i8;
    log::debug!("{:?}", [x_diff, y_diff]);
    let left_click = buf[0] & 0b1 != 0;
    log::debug!("buf: {:?}, clicked: {}", buf, left_click);
    if left_click {
        let pos = MOUSE_CURSOR.into_vec();
        let pos = Vector2D::new(pos.0 as usize, pos.1 as usize);
        if let Some(pos) = frame_buffer_position_to_board_position(pos) {
            let mut queue = kernel_lib::lock!(CLICKED_POSITION_QUEUE);
            queue.push_back(pos);
        }
    }

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

#[doc(hidden)]
pub fn _keyboard(_address: u8, buf: &[u8]) {
    let shifted = (buf[0] & (L_SHIFT_BITMASK | R_SHIFT_BITMASK)) != 0;
    buf[1..]
        .iter()
        .filter_map(|&keycode| {
            log::debug!("keycode: {}", keycode);
            if keycode == 0 {
                None
            } else if shifted {
                Some(KEYCODE_MAP_SHIFTED[keycode as usize])
            } else {
                Some(KEYCODE_MAP[keycode as usize])
            }
        })
        .for_each(|c| {
            log::debug!("char: '{}'", c);
            if c == ' ' {
                // flip the RUNNING state
                log::debug!("flip");
                lifegame::RUNNING.fetch_not(core::sync::atomic::Ordering::SeqCst);
            }
            print_and_flush!("{}", c)
        });
}

const BS: char = '\u{08}';
const NULL: char = '\u{0}';
// for boot keyboard interface
const KEYCODE_MAP: [char; 144] = [
    NULL, NULL, NULL, NULL, 'a', 'b', 'c', 'd', // 0
    'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', // 8
    'm', 'n', 'o', 'p', 'q', 'r', 's', 't', // 16
    'u', 'v', 'w', 'x', 'y', 'z', '1', '2', // 24
    '3', '4', '5', '6', '7', '8', '9', '0', // 32
    '\n', BS, BS, '\t', ' ', '-', '=', '[', // 40
    ']', '\\', '#', ';', '\'', '`', ',', '.', // 48
    '/', NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 56
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 64
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 72
    NULL, NULL, NULL, NULL, '/', '*', '-', '+', // 80
    '\n', '1', '2', '3', '4', '5', '6', '7', // 88
    '8', '9', '0', '.', '\\', NULL, NULL, '=', // 96
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 104
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 112
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 120
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 128
    NULL, '\\', NULL, NULL, NULL, NULL, NULL, NULL, // 136
];

const KEYCODE_MAP_SHIFTED: [char; 144] = [
    NULL, NULL, NULL, NULL, 'A', 'B', 'C', 'D', // NULL
    'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', // 8
    'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', // 16
    'U', 'V', 'W', 'X', 'Y', 'Z', '!', '@', // 24
    '#', '$', '%', '^', '&', '*', '(', ')', // 32
    '\n', BS, BS, '\t', ' ', '_', '+', '{', // 4NULL
    '}', '|', '~', ':', '"', '~', '<', '>', // 48
    '?', NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 56
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 64
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 72
    NULL, NULL, NULL, NULL, '/', '*', '-', '+', // 8NULL
    '\n', '1', '2', '3', '4', '5', '6', '7', // 88
    '8', '9', '0', '.', '\\', NULL, NULL, '=', // 96
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 1NULL4
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 112
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 12NULL
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, // 128
    NULL, '|', NULL, NULL, NULL, NULL, NULL, NULL, // 136
];

const L_CONTROL_BITMASK: u8 = 0b00000001;
const L_SHIFT_BITMASK: u8 = 0b00000010;
const L_ALT_BITMASK: u8 = 0b00000100;
const L_GUI_BITMASK: u8 = 0b00001000;
const R_CONTROL_BITMASK: u8 = 0b00010000;
const R_SHIFT_BITMASK: u8 = 0b00100000;
const R_ALT_BITMASK: u8 = 0b01000000;
const R_GUI_BITMASK: u8 = 0b10000000;
