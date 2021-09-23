use crate::Display;
use gtk::glib::{self, translate::*};

#[no_mangle]
pub extern "C" fn rdw_display_get_type() -> glib::ffi::GType {
    <Display as glib::types::StaticType>::static_type().to_glib()
}
