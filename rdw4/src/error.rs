use gtk::glib;

#[derive(Clone, Copy, Debug, PartialEq, Eq, glib::GErrorDomain)]
#[gerror_domain(name = "RdwError")]
#[repr(C)]
pub enum Error {
    GL,
    Failed,
}
