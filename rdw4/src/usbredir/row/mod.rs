mod imp;

use super::Device;
use gtk::{glib, subclass::prelude::ObjectSubclassExt};

glib::wrapper! {
    pub struct Row(ObjectSubclass<imp::Row>) @extends gtk::Widget;
}

impl Row {
    pub fn new(device: &Device) -> Self {
        glib::Object::new(&[("device", device)]).expect("Failed to create Row")
    }

    pub fn switch(&self) -> &gtk::Switch {
        let self_ = imp::Row::from_instance(self);
        &*self_.switch
    }

    pub fn label(&self) -> &gtk::Label {
        let self_ = imp::Row::from_instance(self);
        &*self_.label
    }
}
