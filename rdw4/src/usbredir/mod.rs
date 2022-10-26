use glib::{
    object::{Cast, ObjectExt, ObjectType as ObjectType_},
    signal::{connect_raw, SignalHandlerId},
    translate::*,
    StaticType, ToValue,
};
use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use std::{boxed::Box as Box_, mem::transmute};

mod device;
pub use device::Device;

mod imp;
/// cbindgen:ignore
mod row;

glib::wrapper! {
    pub struct UsbRedir(ObjectSubclass<imp::UsbRedir>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl UsbRedir {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[])
    }

    pub fn model(&self) -> &gio::ListStore {
        let imp = imp::UsbRedir::from_instance(self);
        &imp.model
    }

    pub fn find_item<F: Fn(&Device) -> bool>(&self, test: F) -> Option<u32> {
        let imp = imp::UsbRedir::from_instance(self);
        imp.find_item(test)
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
