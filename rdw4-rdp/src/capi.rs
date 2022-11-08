use freerdp::sys;

use rdw::gtk::{
    self,
    glib::{self, translate::*},
};

use crate::display::*;

#[no_mangle]
pub extern "C" fn rdw_rdp_display_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <Display as glib::types::StaticType>::static_type().into_glib()
}

/// rdw_rdp_display_connect:
/// @dpy: A #RdwDisplay
#[no_mangle]
pub extern "C" fn rdw_rdp_display_connect(dpy: *mut RdwRdpDisplay) -> bool {
    let mut this: Display = unsafe { from_glib_none(dpy) };
    this.rdp_connect().is_ok()
}

/// rdw_rdp_display_get_settings:
/// @dpy: A #RdwDisplay
///
/// Returns: (transfer none): the associated FreeRDP settings
#[no_mangle]
pub extern "C" fn rdw_rdp_display_get_settings(dpy: *mut RdwRdpDisplay) -> *mut sys::rdpSettings {
    let this: &Display = unsafe { &from_glib_borrow(dpy) };
    let mut settings = std::ptr::null_mut();
    this.with_settings(|s| {
        settings = s.as_ptr();
        Ok(())
    })
    .unwrap();
    settings
}
