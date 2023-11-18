extern crate alloc;

use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use alloc::{collections::BTreeMap, vec};

use crate::pixel::RenderedPixel;
use crate::{AsciiWriter, Color, PixcelWritableMut};

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

impl Position {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

pub struct Window {
    transparent_color: Option<Color>,
    rendering_handler: Box<dyn RenderedPixel + Send + Sync>,
    buffer: Vec<u8>,
    pixels: Vec<Vec<Color>>,
    position: Position,
}

impl Window {
    pub fn new(
        width: usize,
        height: usize,
        rendering_handler: Box<dyn RenderedPixel + Send + Sync>,
        transparent_color: Option<Color>,
        position: Position,
    ) -> Self {
        let mut pixels = Vec::with_capacity(width);
        for _ in 0..width {
            pixels.push(vec![transparent_color.unwrap_or(Color::black()); height]);
        }
        let buffer = vec![0; width * height * 4];
        Self {
            rendering_handler,
            buffer,
            transparent_color,
            pixels,
            position,
        }
    }

    pub fn move_to(&mut self, new_position: Position) {
        self.position = new_position;
    }

    pub fn move_relative(&mut self, x_diff: isize, y_diff: isize) {
        let x = self.position.x as isize + x_diff;
        let y = self.position.y as isize + y_diff;
        self.position.x = x.try_into().unwrap_or(0);
        self.position.y = y.try_into().unwrap_or(0);
    }

    pub fn width(&self) -> usize {
        self.pixels.len()
    }

    pub fn height(&self) -> usize {
        self.pixels[0].len()
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn flush(&self, writer: &(dyn AsciiWriter + Send + Sync)) {
        if let Some(transparent_color) = self.transparent_color {
            for y in self.position.y
                ..core::cmp::min(
                    self.position.y + self.height(),
                    writer.horizontal_resolution(),
                )
            {
                for x in self.position.x
                    ..core::cmp::min(
                        self.position.x + self.width(),
                        writer.pixcels_per_scan_line(),
                    )
                {
                    let color = self.pixels[x - self.position.x][y - self.position.y];
                    if color == transparent_color {
                        continue;
                    }
                    writer.write(x, y, color);
                }
            }
        } else {
            // let frame_buffer_base = writer.frame_buffer_base();
            // let height = core::cmp::min(
            //     self.height(),
            //     writer.horizontal_resolution() - self.position.y,
            // );
            // for y in 0..height {
            //     let offset =
            //         (self.position.y + y) * writer.pixcels_per_scan_line() + self.position.x * 4;
            //     let frame_buffer_row_base = unsafe { frame_buffer_base.add(offset) };
            //     let width = core::cmp::min(
            //         self.width(),
            //         writer.pixcels_per_scan_line() - self.position.x,
            //     );
            //     let frame_buffer_row_slice =
            //         unsafe { core::slice::from_raw_parts_mut(frame_buffer_row_base, width) };

            //     let buffer_row_slice = &self.buffer[(y * self.width())..(y * self.width() + width)];
            //     log::debug!("buffer_row_slice: {:?}", buffer_row_slice.as_ptr_range());
            //     log::debug!(
            //         "frame_buffer_row_slice: {:?}",
            //         frame_buffer_row_slice.as_ptr_range()
            //     );
            //     frame_buffer_row_slice.copy_from_slice(buffer_row_slice);
            // }
            let y_range = self.position.y
                ..core::cmp::min(
                    self.position.y + self.height(),
                    writer.horizontal_resolution(),
                );
            let x_range = self.position.x
                ..core::cmp::min(
                    self.position.x + self.width(),
                    writer.pixcels_per_scan_line(),
                );
            let get_frame_buffer_index =
                |x: usize, y: usize| (x + y * writer.pixcels_per_scan_line()) * 4;
            let get_buffer_index = |x: usize, y: usize| (x + y * self.width()) * 4;
            let frame_buffer_base = writer.frame_buffer_base();
            for y in y_range {
                let offset = get_frame_buffer_index(x_range.start, y);
                let frame_buffer_row_base = unsafe { frame_buffer_base.add(offset) };
                let width = (x_range.end - x_range.start) * 4;
                let frame_buffer_row_slice =
                    unsafe { core::slice::from_raw_parts_mut(frame_buffer_row_base, width) };

                let buffer_row_slice = &self.buffer[get_buffer_index(
                    x_range.start - self.position.x,
                    y - self.position.y,
                )
                    ..get_buffer_index(x_range.end - self.position.x, y - self.position.y)];

                frame_buffer_row_slice.copy_from_slice(buffer_row_slice);
            }
        }
    }

    pub fn write(&mut self, x: usize, y: usize, c: Color) {
        self.pixels[x][y] = c;
        let index = (x + y * self.width()) * 4;
        self.buffer[index..index + 4].copy_from_slice(&self.rendering_handler.pixel(c));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayerId(usize);

impl LayerId {
    pub const fn uninitialized() -> Self {
        Self(usize::MAX)
    }
}

pub struct Layer {
    id: LayerId,
    window: Window,
}

impl Layer {
    pub fn new(window: Window) -> Self {
        static LATEST_UNUSED_ID: AtomicUsize = AtomicUsize::new(0);
        let id = LayerId(LATEST_UNUSED_ID.fetch_add(1, Ordering::Relaxed));
        assert!(id < LayerId::uninitialized());
        Self { id, window }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn id(&self) -> LayerId {
        self.id
    }
}

impl PixcelWritableMut for Layer {
    fn write(&mut self, x: usize, y: usize, color: Color) {
        self.window.write(x, y, color);
    }
}

pub struct LayerManager<'a> {
    writer: &'a (dyn AsciiWriter + Send + Sync),
    layer_stack: VecDeque<LayerId>,
    layers: BTreeMap<LayerId, Layer>,
}

impl<'a> LayerManager<'a> {
    pub fn new(writer: &'a (dyn AsciiWriter + Send + Sync)) -> Self {
        let layers = BTreeMap::new();
        Self {
            writer,
            layer_stack: VecDeque::new(),
            layers,
        }
    }
    pub fn new_layer(&mut self, window: Window) -> LayerId {
        let layer = Layer::new(window);
        let id = layer.id();

        self.layer_stack.push_front(id);
        self.layers.insert(id, layer);

        id
    }

    pub fn move_layer(&mut self, id: LayerId, new_position: Position) {
        let Some(layer) = self.layers.get_mut(&id) else {
            return;
        };

        layer.window.move_to(new_position);
    }

    pub fn move_relative(&mut self, id: LayerId, x_diff: isize, y_diff: isize) {
        let Some(layer) = self.layers.get_mut(&id) else {
            return;
        };

        layer.window.move_relative(x_diff, y_diff);
    }

    pub fn flush(&self) {
        // clear
        // let base = self.writer.frame_buffer_base();
        // let len = self.writer.vertical_resolution() * self.writer.pixcels_per_scan_line() * 4;
        // unsafe { core::ptr::write_bytes(base, 0, len) }

        for layer_id in self.layer_stack.iter() {
            let layer = self.layers.get(layer_id).unwrap();
            layer.window.flush(self.writer);
        }
    }

    pub fn set_to_top(&mut self, id: LayerId) {
        self.hide(id);
        self.layer_stack.push_back(id);
    }

    pub fn hide(&mut self, id: LayerId) {
        let Some(index) = self.layer_stack.iter().position(|&x| x == id) else {
            return;
        };
        self.layer_stack.remove(index);
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(&id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.get_mut(&id)
    }
}
