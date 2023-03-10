mod imp;

use super::Device;
use gtk::{glib, subclass::prelude::ObjectSubclassExt};

glib::wrapper! {
    pub struct Row(ObjectSubclass<imp::Row>) @extends gtk::Widget;
}

impl Row {
    pub(crate) fn new(device: &Device) -> Self {
        glib::Object::new::<Self>(&[("device", device)])
    }

    pub(crate) fn switch(&self) -> &gtk::Switch {
        let imp = imp::Row::from_obj(self);
        &imp.switch
    }
}
