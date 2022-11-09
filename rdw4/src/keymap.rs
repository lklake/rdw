use gtk::prelude::*;

pub fn keymap_xtkbd() -> Option<&'static [u16]> {
    let Some(window) = gtk::Window::toplevels().item(0).and_then(|w| w.downcast::<gtk::Widget>().ok()) else {
        log::warn!("No top-level window? no keymap...");
        return None;
    };

    #[cfg(windows)]
    if window
        .display()
        .downcast::<gdk_win32::Win32Display>()
        .is_ok()
    {
        return Some(keycodemap::KEYMAP_WIN322XTKBD);
    };

    #[cfg(unix)]
    if window
        .display()
        .downcast::<gdk_wl::WaylandDisplay>()
        .is_ok()
    {
        return Some(keycodemap::KEYMAP_XORGEVDEV2XTKBD);
    }

    #[cfg(unix)]
    if let Ok(_dpy) = window.display().downcast::<gdk_x11::X11Display>() {
        todo!()
    };

    log::warn!("Unsupported GDK windowing platform. Please report an issue!");
    None
}

pub fn keymap_qnum() -> Option<&'static [u16]> {
    let Some(window) = gtk::Window::toplevels().item(0).and_then(|w| w.downcast::<gtk::Widget>().ok()) else {
        log::warn!("No top-level window? no keymap...");
        return None;
    };

    #[cfg(windows)]
    if window
        .display()
        .downcast::<gdk_win32::Win32Display>()
        .is_ok()
    {
        return Some(keycodemap::KEYMAP_WIN322QNUM);
    };

    #[cfg(unix)]
    if window
        .display()
        .downcast::<gdk_wl::WaylandDisplay>()
        .is_ok()
    {
        return Some(keycodemap::KEYMAP_XORGEVDEV2QNUM);
    }

    #[cfg(unix)]
    if let Ok(_dpy) = window.display().downcast::<gdk_x11::X11Display>() {
        todo!()
    };

    log::warn!("Unsupported GDK windowing platform. Please report an issue!");
    None
}
