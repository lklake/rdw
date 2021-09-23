use gtk::glib;

#[derive(Clone, Copy, Debug, PartialEq, Eq, glib::GErrorDomain)]
#[gerror_domain(name = "RdwError")]
#[repr(C)]
pub enum Error {
    GL,
    Failed,
}

mod ffi {
    use glib::translate::IntoGlib;
    use gtk::glib;

    #[no_mangle]
    pub unsafe extern "C" fn rdw_error_quark() -> glib::ffi::GQuark {
        <super::Error as glib::error::ErrorDomain>::domain().into_glib()
    }
}
