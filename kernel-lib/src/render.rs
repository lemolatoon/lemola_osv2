use core::ops::{Add, AddAssign};

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
    fn fill_rect(&self, pos: Vector2D, size: Vector2D, color: Color) {
        for y in pos.y..pos.y + size.y {
            for x in pos.x..pos.x + size.x {
                self.write(x, y, color);
            }
        }
    }

    fn draw_rect_outline(&self, pos: Vector2D, size: Vector2D, color: Color) {
        for x in pos.x..pos.x + size.x {
            self.write(x, pos.y, color);
            self.write(x, pos.y + size.y - 1, color);
        }
        for y in pos.y..pos.y + size.y {
            self.write(pos.x, y, color);
            self.write(pos.x + size.x - 1, y, color);
        }
    }

    fn fill_shape(&self, pos: Vector2D, shape: &dyn Shape) {
        for y in 0..shape.get_height() {
            for x in 0..shape.get_width() {
                self.write(pos.x + x, pos.y + y, shape.get_pixel(x, y));
            }
        }
    }
}

impl<T> Renderer for T where T: PixcelWritable {}
