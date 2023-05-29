use crate::Color;
pub trait Shape {
    fn get_width(&self) -> usize;
    fn get_height(&self) -> usize;
    fn get_pixel(&self, x: usize, y: usize) -> Color;
}

impl<const W: usize, const H: usize, T: Into<Color> + Copy> Shape for [[T; W]; H] {
    fn get_width(&self) -> usize {
        W
    }

    fn get_height(&self) -> usize {
        H
    }

    fn get_pixel(&self, x: usize, y: usize) -> Color {
        self[y][x].into()
    }
}

pub mod mouse {
    use crate::Color;

    const MOUSE_CURSOR_WIDTH: usize = 15;
    const MOUSE_CURSOR_HEIGHT: usize = 24;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MouseCursorPixel {
        BackGround,
        Frame,
        Cursor,
    }

    impl From<MouseCursorPixel> for Color {
        fn from(pixel: MouseCursorPixel) -> Self {
            match pixel {
                MouseCursorPixel::BackGround => Color::new(173, 216, 230), // light blue
                MouseCursorPixel::Frame => Color::new(0, 0, 0),            // black
                MouseCursorPixel::Cursor => Color::new(255, 255, 255),     // white
            }
        }
    }

    const fn to(c: char) -> MouseCursorPixel {
        match c {
            ' ' => MouseCursorPixel::BackGround,
            '@' => MouseCursorPixel::Frame,
            '.' => MouseCursorPixel::Cursor,
            _ => panic!("Unexpected"),
        }
    }

    #[doc(hidden)]
    pub const fn decode<const LEN: usize>(strings: &[u8]) -> [MouseCursorPixel; LEN] {
        let mut buf = [MouseCursorPixel::BackGround; LEN];
        let mut idx = 0;
        loop {
            buf[idx] = to(strings[idx] as char);
            idx += 1;
            if idx == LEN {
                break;
            }
        }
        buf
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! arraify_single {
        ($s:literal) => {{
            const LEN: usize = $s.len();
            const STRINGS: &'static [u8] = $s.as_bytes();
            const RES: [MouseCursorPixel; LEN] = $crate::shapes::mouse::decode(STRINGS);
            RES
        }};
    }

    #[doc(hidden)]
    macro_rules! arraify {
        ($($s:literal),*) => {{
           [$($crate::arraify_single!($s)),*]
        }};
    }

    pub const MOUSE_CURSOR_SHAPE: [[MouseCursorPixel; MOUSE_CURSOR_WIDTH]; MOUSE_CURSOR_HEIGHT] = arraify!(
        "@              ",
        "@@             ",
        "@.@            ",
        "@..@           ",
        "@...@          ",
        "@....@         ",
        "@.....@        ",
        "@......@       ",
        "@.......@      ",
        "@........@     ",
        "@.........@    ",
        "@..........@   ",
        "@...........@  ",
        "@............@ ",
        "@......@@@@@@@@",
        "@......@       ",
        "@....@@.@      ",
        "@...@ @.@      ",
        "@..@   @.@     ",
        "@.@    @.@     ",
        "@@      @.@    ",
        "@       @.@    ",
        "         @.@   ",
        "         @@@   "
    );
}
