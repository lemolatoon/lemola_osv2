extern crate alloc;
use core::sync::atomic::AtomicBool;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use kernel_lib::futures::yield_pending;
use kernel_lib::layer::{Position, Window};
use kernel_lib::mutex::Mutex;
use kernel_lib::render::{RendererMut, Vector2D};
use kernel_lib::Color;

use crate::graphics::LAYER_MANGER;

pub static CLICKED_POSITION_QUEUE: Mutex<VecDeque<(usize, usize)>> = Mutex::new(VecDeque::new());

const SIZE: usize = 20;
const PIXCEL_SIZE: usize = 30;
const BOARD_POS: Vector2D = Vector2D::new(0, 0);

pub static RUNNING: AtomicBool = AtomicBool::new(true);

pub fn frame_buffer_position_to_board_position(
    frame_buffer_position: Vector2D,
) -> Option<(usize, usize)> {
    log::debug!("transforming...: {:?}", &frame_buffer_position);
    let x = frame_buffer_position.x as isize - BOARD_POS.x as isize;
    let y = frame_buffer_position.y as isize - BOARD_POS.y as isize;
    log::debug!("(relative) (x, y) = {:?}", (x, y));
    if x < 0 || y < 0 {
        return None;
    }
    let x = x as usize / PIXCEL_SIZE;
    let y = y as usize / PIXCEL_SIZE;
    log::debug!("(x, y) = {:?}", (x, y));
    if x >= SIZE || y >= SIZE {
        return None;
    }
    Some((x, y))
}

pub async fn do_lifegame() {
    let window = Window::new(
        SIZE * PIXCEL_SIZE,
        SIZE * PIXCEL_SIZE,
        Some(Color::black()),
        Position::new(0, 0),
    );
    let id = kernel_lib::lock!(LAYER_MANGER)
        .get_mut()
        .unwrap()
        .new_layer(window);
    // let pixcel_writer = get_pixcel_writer().unwrap();
    let board: [[u8; SIZE]; SIZE] = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    ];
    let mut board: Vec<Vec<bool>> = board
        .into_iter()
        .map(|inner| inner.into_iter().map(|n| n == 1).collect())
        .collect();
    loop {
        for _ in 0..100000 {
            {
                let mut queue = kernel_lib::lock!(CLICKED_POSITION_QUEUE);
                let is_empty = queue.is_empty();
                while let Some((x, y)) = queue.pop_front() {
                    board[y][x] = true;
                }
                if !is_empty {
                    kernel_lib::lock!(LAYER_MANGER)
                        .get_mut()
                        .unwrap()
                        .layer_mut(id)
                        .unwrap()
                        .render_board(&board, BOARD_POS, PIXCEL_SIZE, Color::green());
                }
            }
            yield_pending().await;
        }
        kernel_lib::lock!(LAYER_MANGER).get().unwrap().flush();
        yield_pending().await;
        // log::info!("RUNNING: {}", RUNNING.load(core::sync::atomic::Ordering::SeqCst));
        if RUNNING.load(core::sync::atomic::Ordering::SeqCst) {
            process::<SIZE>(&mut board);
        }
        kernel_lib::lock!(LAYER_MANGER)
            .get_mut()
            .unwrap()
            .layer_mut(id)
            .unwrap()
            .render_board(&board, BOARD_POS, PIXCEL_SIZE, Color::green());
        yield_pending().await;
    }
}

fn process<const SIZE: usize>(board: &mut [Vec<bool>]) {
    let mut next_board = [[false; SIZE]; SIZE];
    for i in 0..SIZE {
        for j in 0..SIZE {
            let mut count = 0;
            for x in -1..=1 {
                for y in -1..=1 {
                    if x == 0 && y == 0 {
                        continue;
                    }
                    let x = i as isize + x;
                    let y = j as isize + y;
                    if x < 0 || x >= SIZE as isize || y < 0 || y >= SIZE as isize {
                        continue;
                    }
                    if board[x as usize][y as usize] {
                        count += 1;
                    }
                }
            }
            if board[i][j] {
                if count == 2 || count == 3 {
                    next_board[i][j] = true;
                }
            } else if count == 3 {
                next_board[i][j] = true;
            }
        }
    }

    // copy next_board to board
    for i in 0..SIZE {
        for j in 0..SIZE {
            board[i][j] = next_board[i][j];
        }
    }
}
