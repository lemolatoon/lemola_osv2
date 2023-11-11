extern crate alloc;

use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use alloc::{collections::BTreeMap, vec};

use crate::{AsciiWriter, Color, PixcelWritableMut};

#[derive(Debug, Clone, Copy)]
pub struct Position {
    x: usize,
    y: usize,
}

impl Position {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

pub struct Window {
    transparent_color: Color,
    pixels: Vec<Vec<Color>>,
    position: Position,
}

impl Window {
    pub fn new(width: usize, height: usize, transparent_color: Color, position: Position) -> Self {
        let mut pixels = Vec::with_capacity(height);
        for _ in 0..height {
            pixels.push(vec![transparent_color; width]);
        }
        Self {
            transparent_color,
            pixels,
            position,
        }
    }

    pub fn move_to(&mut self, new_position: Position) {
        self.position = new_position;
    }

    pub fn move_relative(&mut self, x_diff: usize, y_diff: usize) {
        self.position.x += x_diff;
        self.position.y += y_diff;
    }

    pub fn height(&self) -> usize {
        self.pixels.len()
    }

    pub fn width(&self) -> usize {
        self.pixels[0].len()
    }

    pub fn flush(&self, writer: &(dyn AsciiWriter + Send + Sync)) {
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
                let color = self.pixels[x][y];
                if color == self.transparent_color {
                    continue;
                }

                writer.write(x, y, color);
            }
        }
    }

    pub fn write(&mut self, x: usize, y: usize, c: Color) {
        self.pixels[x][y] = c;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayerId(usize);

pub struct Layer {
    id: LayerId,
    window: Window,
}

impl Layer {
    pub fn new(window: Window) -> Self {
        static LATEST_UNUSED_ID: AtomicUsize = AtomicUsize::new(0);
        let id = LayerId(LATEST_UNUSED_ID.fetch_add(1, Ordering::Relaxed));
        Self { id, window }
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
        // let underlying_layer = Layer::new(Window::new(
        //     writer.horizontal_resolution(),
        //     writer.vertical_resolution(),
        //     Color::black(),
        //     Position::new(0, 0),
        // ));
        Self {
            writer,
            layer_stack: VecDeque::new(),
            layers: BTreeMap::new(),
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

    pub fn move_relative(&mut self, id: LayerId, x_diff: usize, y_diff: usize) {
        let Some(layer) = self.layers.get_mut(&id) else {
            return;
        };

        layer.window.move_relative(x_diff, y_diff);
    }

    pub fn flush(&self) {
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
