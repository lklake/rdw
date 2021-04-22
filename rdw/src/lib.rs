use gtk::glib::{gflags, GEnum};

mod display;
mod egl;
mod error;
mod util;

pub use display::*;
pub use egl::DmabufScanout;

#[derive(Debug, Eq, PartialEq, Clone, Copy, GEnum)]
#[genum(type_name = "RdwScroll")]
#[repr(C)]
pub enum Scroll {
    Up,
    Down,
    Left,
    Right,
}

#[gflags("KeyEvent")]
#[repr(C)] // See https://github.com/bitflags/bitflags/pull/187
pub enum KeyEvent {
    PRESS = 0b0000_0001,
    RELEASE = 0b0000_0010,
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
