use std::cell::{Cell, RefCell};

use glib::{subclass::Signal, ParamFlags, ParamSpec, ParamSpecBoolean, ParamSpecString};
use gtk::{glib, prelude::*, subclass::prelude::*};
use once_cell::sync::Lazy;
use usbredirhost::rusb;

#[derive(Default)]
pub struct Device {
    pub device: RefCell<Option<rusb::Device<rusb::Context>>>,
    pub name: RefCell<String>,
    pub active: Cell<bool>,
}

#[glib::object_subclass]
impl ObjectSubclass for Device {
    const NAME: &'static str = "RdwUsbDevice";
    type Type = super::Device;
    type ParentType = glib::Object;
}

impl ObjectImpl for Device {
    fn properties() -> &'static [ParamSpec] {
        static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
            vec![
                ParamSpecString::new(
                    "name",
                    "Name",
                    "The device name",
                    None,
                    ParamFlags::READWRITE,
                ),
                ParamSpecBoolean::new(
                    "active",
                    "Active",
                    "Device is redirected",
                    false,
                    ParamFlags::READWRITE,
                ),
            ]
        });
        PROPERTIES.as_ref()
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "name" => self.name.borrow().to_value(),
            "active" => self.active.get().to_value(),
            _ => unimplemented!(),
        }
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &ParamSpec) {
        match pspec.name() {
            "name" => {
                self.name.replace(value.get().unwrap());
            }
            "active" => {
                self.active.set(value.get().unwrap());
            }
            _ => unimplemented!(),
        }
    }

    fn signals() -> &'static [Signal] {
        static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
            vec![Signal::builder("state-set")
                .param_types([bool::static_type()])
                .build()]
        });
        SIGNALS.as_ref()
    }
}

impl Device {
    pub fn set_name(&self, name: &str) {
        self.obj().set_property("name", name)
    }

    pub fn set_device(&self, device: rusb::Device<rusb::Context>) {
        if let Ok((manufacturer, product)) = device_manufacturer_product(&device) {
            self.set_name(&format!("{} {}", manufacturer, product));
        } else {
            self.set_name(&format!(
                "Bus {:03} Device {:03}",
                device.bus_number(),
                device.address()
            ));
        }
        self.device.replace(Some(device));
    }

    pub fn device(&self) -> Option<rusb::Device<rusb::Context>> {
        self.device.borrow().clone()
    }
}

#[cfg(unix)]
fn read_char_attribute(major: u32, minor: u32, attribute: &str) -> std::io::Result<String> {
    use std::io::prelude::*;

    let mut file = std::fs::File::open(format!("/sys/dev/char/{}:{}/{}", major, minor, attribute))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    loop {
        if !contents.ends_with('\n') {
            break;
        }
        contents.pop();
    }
    Ok(contents)
}

// TODO: non-linux version, with usb-ids?
#[cfg(unix)]
fn device_manufacturer_product(
    device: &rusb::Device<rusb::Context>,
) -> std::io::Result<(String, String)> {
    use std::fs::*;
    use std::os::unix::fs::MetadataExt;

    let (bus, addr) = (device.bus_number(), device.address());
    let metadata = metadata(format!("/dev/bus/usb/{:03}/{:03}", bus, addr))?;
    let rdev = metadata.rdev();
    let (major, minor) = unsafe { (libc::major(rdev), libc::minor(rdev)) };
    let manufacturer = read_char_attribute(major, minor, "manufacturer");
    let product = read_char_attribute(major, minor, "product");
    if manufacturer.is_ok() || product.is_ok() {
        Ok((
            manufacturer.unwrap_or_else(|_| "N/A".into()),
            product.unwrap_or_else(|_| "N/A".into()),
        ))
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Unknown device",
        ))
    }
}

#[cfg(not(unix))]
fn device_manufacturer_product(
    device: &rusb::Device<rusb::Context>,
) -> std::io::Result<(String, String)> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Unknown device",
    ))
}
