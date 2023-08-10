extern crate alloc;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use kernel_lib::futures::yield_pending;
use kernel_lib::mutex::Mutex;
use kernel_lib::render::Vector2D;
use kernel_lib::{Color, PixcelInfo};
use x86_64::structures::paging::frame;

use crate::graphics::get_pixcel_writer;

pub static CLICKED_POSITION_QUEUE: Mutex<VecDeque<(usize, usize)>> = Mutex::new(VecDeque::new());

const SIZE: usize = 20;
const PIXCEL_SIZE: usize = 10;
const BOARD_POS: Vector2D = Vector2D::new(300, 400);
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
    let pixcel_writer = get_pixcel_writer().unwrap();
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
                while let Some((x, y)) = queue.pop_front() {
                    log::debug!("popped position: {:?}", (x, y));
                    board[y][x] = true;
                    pretty_print_board(&board);
                }
            }
            yield_pending().await;
        }
        process::<SIZE>(&mut board);
        pixcel_writer.render_board(&board, BOARD_POS, PIXCEL_SIZE, Color::green());
    }
}

fn pretty_print_board(board: &Vec<Vec<bool>>) {
    for row in board {
        let mut array = [0; 20];
        for i in 0..(array.len()) {
            if row[i] {
                array[i] = 1;
            }
        }
        log::debug!("{:?}", array);
    }
}

fn process<const SIZE: usize>(board: &mut Vec<Vec<bool>>) {
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
            } else {
                if count == 3 {
                    next_board[i][j] = true;
                }
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
