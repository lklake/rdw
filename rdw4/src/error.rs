use gtk::glib;

#[derive(Clone, Copy, Debug, PartialEq, Eq, glib::ErrorDomain)]
#[error_domain(name = "RdwError")]
#[repr(C)]
pub enum Error {
    GL,
    Failed,
}
