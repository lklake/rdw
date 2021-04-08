use gtk::glib::{self, gflags, GEnum};

mod display;
mod egl;
mod error;
mod util;

pub use display::*;

#[derive(Debug, Eq, PartialEq, Clone, Copy, GEnum)]
#[genum(type_name = "RdwScroll")]
#[repr(C)]
pub enum Scroll {
    Up,
    Down,
    Left,
    Right,
}

#[gflags("RdwGrab")]
#[repr(C)]
pub enum Grab {
    MOUSE = 0b0000_0001,
    KEYBOARD = 0b0000_0010,
}

impl std::default::Default for Grab {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(feature = "capi")]
mod capi;
