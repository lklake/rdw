pub mod imp;

use glib::{
    object::ObjectType as ObjectType_,
    signal::{connect_raw, SignalHandlerId},
    translate::*,
    Object,
};
use std::{boxed::Box as Box_, mem::transmute};

use gtk::{glib, subclass::prelude::*};
use usbredirhost::rusb;

glib::wrapper! {
    pub struct Device(ObjectSubclass<imp::Device>);
}

// TODO: make a base class, and derive it for libusb/emulated etc
impl Device {
    pub fn new() -> Self {
        Object::new(&[]).expect("Failed to create `Device`")
    }

    pub fn device(&self) -> Option<rusb::Device<rusb::Context>> {
        let self_ = imp::Device::from_instance(self);
        self_.device()
    }

    pub fn set_device(&self, device: rusb::Device<rusb::Context>) {
        let self_ = imp::Device::from_instance(self);
        self_.set_device(device)
    }

    pub fn is_device(&self, device: &rusb::Device<rusb::Context>) -> bool {
        let self_ = imp::Device::from_instance(self);
        let d = self_.device.borrow();

        if let Some(d) = &*d {
            d == device
        } else {
            false
        }
    }

    pub fn connect_state_set<F: Fn(&Self, bool) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn state_set_trampoline<F: Fn(&Device, bool) + 'static>(
            this: *mut <imp::Device as ObjectSubclass>::Instance,
            state: glib::ffi::gboolean,
            f: glib::ffi::gpointer,
        ) {
            let f: &F = &*(f as *const F);
            f(&from_glib_borrow(this), from_glib(state));
        }
        unsafe {
            let f: Box_<F> = Box_::new(f);
            connect_raw(
                self.as_ptr() as *mut _,
                b"state-set\0".as_ptr() as *const _,
                Some(transmute::<_, unsafe extern "C" fn()>(
                    state_set_trampoline::<F> as *const (),
                )),
                Box_::into_raw(f),
            )
        }
    }
}

impl Default for Device {
    fn default() -> Self {
        Self::new()
    }
}
