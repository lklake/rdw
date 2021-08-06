use glib::object::Cast;
use glib::object::ObjectExt;
use glib::object::ObjectType as ObjectType_;
use glib::signal::connect_raw;
use glib::signal::SignalHandlerId;
use glib::translate::*;
use glib::StaticType;
use glib::ToValue;
use gtk::subclass::prelude::*;
use gtk::{gio, glib, prelude::*};
use std::boxed::Box as Box_;
use std::mem::transmute;

mod device;
pub use device::Device;

mod imp;
mod row;

glib::wrapper! {
    pub struct UsbRedir(ObjectSubclass<imp::UsbRedir>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl UsbRedir {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[]).unwrap()
    }

    pub fn model(&self) -> &gio::ListStore {
        let self_ = imp::UsbRedir::from_instance(self);
        &self_.model
    }

    pub fn find_item<F: Fn(&Device) -> bool>(&self, test: F) -> Option<u32> {
        let self_ = imp::UsbRedir::from_instance(self);
        self_.find_item(test)
    }

    pub fn connect_device_state_set<F: Fn(&Self, &Device, bool) + 'static>(
        &self,
        f: F,
    ) -> SignalHandlerId {
        unsafe extern "C" fn state_set_trampoline<F: Fn(&UsbRedir, &Device, bool) + 'static>(
            this: *mut <imp::UsbRedir as ObjectSubclass>::Instance,
            device: *mut <device::imp::Device as ObjectSubclass>::Instance,
            state: glib::ffi::gboolean,
            f: glib::ffi::gpointer,
        ) {
            let f: &F = &*(f as *const F);
            f(
                &from_glib_borrow(this),
                &from_glib_borrow(device),
                from_glib(state),
            );
        }
        unsafe {
            let f: Box_<F> = Box_::new(f);
            connect_raw(
                self.as_ptr() as *mut _,
                b"device-state-set\0".as_ptr() as *const _,
                Some(transmute::<_, unsafe extern "C" fn()>(
                    state_set_trampoline::<F> as *const (),
                )),
                Box_::into_raw(f),
            )
        }
    }
}

impl Default for UsbRedir {
    fn default() -> Self {
        Self::new()
    }
}
