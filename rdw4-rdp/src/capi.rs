use rdw::gtk::{
    self,
    glib::{self, translate::*},
};

#[no_mangle]
pub extern "C" fn rdw_rdp_display_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <crate::Display as glib::types::StaticType>::static_type().into_glib()
}
