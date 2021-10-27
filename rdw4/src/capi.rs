use gtk::glib::{self, translate::*};

#[no_mangle]
pub extern "C" fn rdw_error_quark() -> glib::ffi::GQuark {
    <crate::Error as glib::error::ErrorDomain>::domain().into_glib()
}

#[no_mangle]
pub extern "C" fn rdw_display_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::Display as glib::types::StaticType>::static_type().into_glib()
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

#[no_mangle]
pub extern "C" fn rdw_key_event_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::KeyEvent as glib::types::StaticType>::static_type().into_glib()
}

#[no_mangle]
pub extern "C" fn rdw_scroll_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::Scroll as glib::types::StaticType>::static_type().into_glib()
}

#[no_mangle]
pub extern "C" fn rdw_grab_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::Grab as glib::types::StaticType>::static_type().into_glib()
}
