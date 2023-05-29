use crate::{shapes::Shape, Color, PixcelWritable};

pub struct Vector2D {
    x: usize,
    y: usize,
}

impl Vector2D {
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
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

    fn fill_shape(&self, pos: Vector2D, shape: &dyn Shape) {
        for y in 0..shape.get_height() {
            for x in 0..shape.get_width() {
                self.write(pos.x + x, pos.y + y, shape.get_pixel(x, y).into());
            }
        }
    }
}

impl<T> Renderer for T where T: PixcelWritable {}
