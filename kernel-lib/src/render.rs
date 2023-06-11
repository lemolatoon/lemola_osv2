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
