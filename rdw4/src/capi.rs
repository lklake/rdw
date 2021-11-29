use gtk::{
    gdk,
    glib::{self, translate::*},
};

use crate::{display::*, egl::RdwDmabufScanout};

#[no_mangle]
pub extern "C" fn rdw_error_quark() -> glib::ffi::GQuark {
    <crate::Error as glib::error::ErrorDomain>::domain().into_glib()
}

#[no_mangle]
pub extern "C" fn rdw_display_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::Display as glib::types::StaticType>::static_type().into_glib()
}

/// rdw_display_get_display_size:
/// @dpy: A #RdwDisplay
/// @width: (out): display width
/// @height: (out): display height
#[no_mangle]
pub extern "C" fn rdw_display_get_display_size(
    dpy: *mut RdwDisplay,
    width: *mut usize,
    height: *mut usize,
) -> bool {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    match this.display_size() {
        Some(res) => unsafe {
            *width = res.0;
            *height = res.1;
            true
        },
        _ => false,
    }
}

/// rdw_display_set_display_size:
/// @dpy: A #RdwDisplay
/// @width: display width
/// @height: display height
///
/// Set the display size. If width & height are 0, the display size is set to
/// unknown.
#[no_mangle]
pub extern "C" fn rdw_display_set_display_size(dpy: *mut RdwDisplay, width: usize, height: usize) {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    let size = if width != 0 && height != 0 {
        Some((width, height))
    } else {
        None
    };
    this.set_display_size(size);
}

/// rdw_display_define_cursor:
/// @dpy: A #RdwDisplay
/// @cursor: (nullable): a #GdkCursor
///
/// Set the cursor shape.
#[no_mangle]
pub extern "C" fn rdw_display_define_cursor(
    dpy: *mut RdwDisplay,
    cursor: *const gdk::ffi::GdkCursor,
) {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    let cursor = unsafe { from_glib_none(cursor) };
    this.define_cursor(cursor);
}

/// rdw_display_set_cursor_position:
/// @dpy: A #RdwDisplay
/// @enabled: whether the cursor is shown
///
/// Set the cursor position (in mouse relative mode).
#[no_mangle]
pub extern "C" fn rdw_display_set_cursor_position(
    dpy: *mut RdwDisplay,
    enabled: bool,
    x: usize,
    y: usize,
) {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    let pos = enabled.then(|| (x, y));
    this.set_cursor_position(pos);
}

/// rdw_display_update_area:
/// @dpy: A #RdwDisplay
/// @data: (array) (element-type guint8): data
#[no_mangle]
pub extern "C" fn rdw_display_update_area(
    dpy: *mut RdwDisplay,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    stride: i32,
    data: *const u8,
) {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    let data = unsafe { std::slice::from_raw_parts(data, (h * stride) as _) };
    this.update_area(x, y, w, h, stride, data);
}

/// rdw_display_render:
/// @dpy: A #RdwDisplay
#[no_mangle]
pub extern "C" fn rdw_display_render(dpy: *mut RdwDisplay) {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    this.render();
}

/// rdw_display_set_dmabuf_scanout:
/// @dpy: A #RdwDisplay
#[no_mangle]
pub extern "C" fn rdw_display_set_dmabuf_scanout(
    dpy: *mut RdwDisplay,
    dmabuf: *const RdwDmabufScanout,
) {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    let dmabuf = unsafe { &*dmabuf };
    let dmabuf = RdwDmabufScanout {
        width: dmabuf.width,
        height: dmabuf.height,
        stride: dmabuf.stride,
        fourcc: dmabuf.fourcc,
        modifier: dmabuf.modifier,
        fd: dmabuf.fd,
        y0_top: dmabuf.y0_top,
    };
    this.set_dmabuf_scanout(dmabuf);
}

#[no_mangle]
pub extern "C" fn rdw_content_provider_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::ContentProvider as glib::types::StaticType>::static_type().into_glib()
}

#[no_mangle]
pub extern "C" fn rdw_usb_redir_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::UsbRedir as glib::types::StaticType>::static_type().into_glib()
}

#[no_mangle]
pub extern "C" fn rdw_usb_device_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::UsbDevice as glib::types::StaticType>::static_type().into_glib()
}
