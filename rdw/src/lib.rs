use gtk::glib::{self, GEnum};

mod display;
mod egl;
mod error;
mod util;

pub use display::*;

#[derive(Debug, Eq, PartialEq, Clone, Copy, GEnum)]
#[repr(u32)]
#[genum(type_name = "RdwScroll")]
pub enum Scroll {
    Up,
    Down,
    Left,
    Right,
}

#[cfg(feature = "capi")]
mod capi;
