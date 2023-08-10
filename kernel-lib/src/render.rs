extern crate alloc;
use core::ops::{Add, AddAssign};

use alloc::vec::Vec;

use crate::{shapes::Shape, Color, PixcelWritable};

#[derive(Debug)]
pub struct AtomicVec2D {
    x: core::sync::atomic::AtomicIsize,
    y: core::sync::atomic::AtomicIsize,
}

impl AtomicVec2D {
    pub const fn new(x: isize, y: isize) -> Self {
        Self {
            x: core::sync::atomic::AtomicIsize::new(x),
            y: core::sync::atomic::AtomicIsize::new(y),
        }
    }
}

impl AtomicVec2D {
    pub fn add(&self, diff_x: isize, diff_y: isize) {
        use core::sync::atomic::Ordering::Relaxed;
        self.x.fetch_add(diff_x, Relaxed);
        self.y.fetch_add(diff_y, Relaxed);
    }

    pub fn into_vec(&self) -> (isize, isize) {
        (
            self.x.load(core::sync::atomic::Ordering::Relaxed),
            self.y.load(core::sync::atomic::Ordering::Relaxed),
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Vector2D {
    pub x: usize,
    pub y: usize,
}

impl Vector2D {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

impl Add for Vector2D {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vector2D {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

pub trait Renderer: PixcelWritable {
    fn fill_rect_at(&self, pos: Vector2D, size: Vector2D, color: Color, layer_id: usize) {
        for y in pos.y..pos.y + size.y {
            for x in pos.x..pos.x + size.x {
                self.write_at(x, y, color, layer_id);
            }
        }
    }

    fn render_board_at(&self, board: &Vec<Vec<bool>>, pos: Vector2D, size: usize, color: Color, layer_id: usize) {
        let len = board.len();
        for y in 0..len {
            for x in 0..len {
                let block_pos = Vector2D::new(pos.x + x * size, pos.y + y * size);
                if board[y][x] {
                    self.fill_rect_at(
                        Vector2D::new(block_pos.x + 1, block_pos.y + 1),
                        Vector2D::new(size - 1, size - 1),
                        color,
                        layer_id
                    );
                } else {
                    self.fill_rect_at(
                        Vector2D::new(block_pos.x + 1, block_pos.y + 1),
                        Vector2D::new(size - 1, size - 1),
                        Color::black(),
                        layer_id
                    );
                }
                self.draw_rect_outline_at(block_pos, Vector2D::new(size, size), Color::white(), layer_id);
            }
        }
    }

    fn draw_rect_outline_at(&self, pos: Vector2D, size: Vector2D, color: Color, layer_id: usize) {
        for x in pos.x..pos.x + size.x {
            self.write_at(x, pos.y, color, layer_id);
            self.write_at(x, pos.y + size.y - 1, color, layer_id);
        }
        for y in pos.y..pos.y + size.y {
            self.write_at(pos.x, y, color, layer_id);
            self.write_at(pos.x + size.x - 1, y, color, layer_id);
        }
    }

    fn fill_shape_at(&self, pos: Vector2D, shape: &dyn Shape, layer_id: usize) {
        for y in 0..shape.get_height() {
            for x in 0..shape.get_width() {
                self.write_at(pos.x + x, pos.y + y, shape.get_pixel(x, y), layer_id);
            }
        }
    }
}

impl<T> Renderer for T where T: PixcelWritable {}
