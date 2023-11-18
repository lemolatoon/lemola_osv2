use kernel_lib::{
    layer::{LayerId, Position, Window},
    pixel::new_rendering_handler,
    render::{RendererMut, Vector2D},
    shapes::{
        mouse::{MouseCursorPixel, MOUSE_CURSOR_SHAPE},
        Shape,
    },
};

use crate::{
    graphics::get_graphics_info,
    lifegame::{self, frame_buffer_position_to_board_position, CLICKED_POSITION_QUEUE},
    print_and_flush,
};

pub type CallbackType = fn(u8, &[u8]);

pub const fn keyboard() -> CallbackType {
    _keyboard
}

pub const fn mouse() -> CallbackType {
    _mouse
}

/// This function must be called before any other functions that use MOUSE_LAYER_ID.
/// # Safety
/// This method must be called before mouse driver is initialized.
/// Also, this method must be called only once.
pub unsafe fn init_mouse_cursor_layer() -> LayerId {
    let window = Window::new(
        MOUSE_CURSOR_SHAPE.get_width(),
        MOUSE_CURSOR_SHAPE.get_height(),
        new_rendering_handler(*get_graphics_info()),
        Some(MouseCursorPixel::BackGround.into()),
        Position::new(0, 0),
    );
    let id = {
        let mut mgr = crate::lock_layer_manager_raw!();
        let mgr = mgr.get_mut().unwrap();
        let id = mgr.new_layer(window);
        mgr.move_relative(id, 0, 0);
        let layer = mgr.layer_mut(id).unwrap();
        layer.fill_shape(Vector2D::new(0, 0), &MOUSE_CURSOR_SHAPE);

        id
    };

    // Safety: This function assumed to be called before any other functions that use MOUSE_LAYER_ID.
    unsafe {
        MOUSE_LAYER_ID = id;
    };
    id
}

static mut MOUSE_LAYER_ID: LayerId = LayerId::uninitialized();
fn mouse_layer_id() -> LayerId {
    // Safety: MOUSE_LAYER_ID is initialized by init_mouse_cursor_layer.
    unsafe { MOUSE_LAYER_ID }
}

#[doc(hidden)]
pub fn _mouse(_address: u8, buf: &[u8]) {
    let x_diff = buf[1] as i8;
    let y_diff = buf[2] as i8;
    let left_click = buf[0] & 0b1 != 0;
    let pos = {
        crate::lock_layer_manager!()
            .layer(mouse_layer_id())
            .unwrap()
            .window()
            .position()
    };
    log::debug!("pos: {:?}", pos);
    if left_click {
        let pos = {
            crate::lock_layer_manager!()
                .layer(mouse_layer_id())
                .unwrap()
                .window()
                .position()
        };
        let pos = Vector2D::new(pos.x, pos.y);
        log::debug!("pos: {:?}", pos);
        if let Some(pos) = frame_buffer_position_to_board_position(pos) {
            let mut queue = kernel_lib::lock!(CLICKED_POSITION_QUEUE);
            queue.push_back(pos);
        }
    }

    {
        crate::lock_layer_manager_mut!().move_relative(
            mouse_layer_id(),
            x_diff.into(),
            y_diff.into(),
        );
    }
}

#[doc(hidden)]
pub fn _keyboard(_address: u8, buf: &[u8]) {
    let shifted = (buf[0] & (L_SHIFT_BITMASK | R_SHIFT_BITMASK)) != 0;
    buf[1..]
        .iter()
        .filter_map(|&keycode| {
            if keycode == 0 {
                None
            } else if shifted {
                Some(KEYCODE_MAP_SHIFTED[keycode as usize])
            } else {
                Some(KEYCODE_MAP[keycode as usize])
            }
        })
        .for_each(|c| {
            if c == ' ' {
                // flip the RUNNING state
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

#[allow(dead_code)]
const L_CONTROL_BITMASK: u8 = 0b00000001;
const L_SHIFT_BITMASK: u8 = 0b00000010;
#[allow(dead_code)]
const L_ALT_BITMASK: u8 = 0b00000100;
#[allow(dead_code)]
const L_GUI_BITMASK: u8 = 0b00001000;
#[allow(dead_code)]
const R_CONTROL_BITMASK: u8 = 0b00010000;
const R_SHIFT_BITMASK: u8 = 0b00100000;
#[allow(dead_code)]
const R_ALT_BITMASK: u8 = 0b01000000;
#[allow(dead_code)]
const R_GUI_BITMASK: u8 = 0b10000000;
