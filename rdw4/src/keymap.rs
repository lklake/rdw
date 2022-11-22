use gtk::gdk;
use gtk::prelude::*;

fn get_display() -> Option<gdk::Display> {
    let Some(window) = gtk::Window::toplevels().item(0).and_then(|w| w.downcast::<gtk::Widget>().ok()) else {
        log::warn!("No top-level window? no keymap...");
        return None;
    };

    Some(window.display())
}

pub fn keymap_xtkbd() -> Option<&'static [u16]> {
    let Some(dpy) = get_display() else {
        return None;
    };

    let map = match dpy.backend() {
        #[cfg(windows)]
        gdk::Backend::Win32 => keycodemap::KEYMAP_WIN322XTKBD,
        gdk::Backend::Wayland => keycodemap::KEYMAP_XORGEVDEV2XTKBD,
        gdk::Backend::X11 => {
            // TODO check X11 server..
            keycodemap::KEYMAP_XORGEVDEV2XTKBD
        }
        be => {
            log::warn!("Unsupported display backend: {be:?}");
            return None;
        }
    };

    Some(map)
}

pub fn keymap_qnum() -> Option<&'static [u16]> {
    let Some(dpy) = get_display() else {
        return None;
    };

    let map = match dpy.backend() {
        #[cfg(windows)]
        gdk::Backend::Win32 => keycodemap::KEYMAP_WIN322QNUM,
        gdk::Backend::Wayland => keycodemap::KEYMAP_XORGEVDEV2QNUM,
        gdk::Backend::X11 => {
            // TODO check X11 server..
            keycodemap::KEYMAP_XORGEVDEV2QNUM
        }
        be => {
            log::warn!("Unsupported display backend: {be:?}");
            return None;
        }
    };

    Some(map)
}
