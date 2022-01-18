use crate::spice;
use glib::{clone, MainContext};
use gtk::{glib, prelude::*};
use rdw::gtk;

pub struct UsbRedir {}

impl UsbRedir {
    pub fn build(session: &spice::Session) -> Result<rdw::UsbRedir, glib::Error> {
        // FIXME... we probably want to avoid & rewrite the spice-gtk usb manager...
        // for now we have to match devices somehow...
        let manager = spice::UsbDeviceManager::get(session)?;

        let redir = rdw::UsbRedir::new();
        redir.model().connect_items_changed(
            clone!(@weak manager, @weak redir => move |model, _pos, _rm, _add| {
                for dev in manager.devices().iter() {
                    if manager.is_device_connected(dev) {
                        if let Some(pos) = redir.find_item(|item| same_device(dev, item)) {
                            let item = model.item(pos).unwrap();
                            item.set_property("active", true);
                        }
                    }
                }
            }),
        );

        redir.connect_device_state_set(clone!(@weak session, @weak manager => move |redir, item, state| {
            for dev in manager.devices().iter() {
                if same_device(dev, item) {
                    let dev = dev.clone();
                    MainContext::default().spawn_local(clone!(@weak manager, @weak item, @weak redir => async move {
                        match set_device_state(dev, manager, state).await {
                            Ok(active) => item.set_property("active", active),
                            Err(e) => {
                                if state {
                                    item.set_property("active", false);
                                }
                                redir.emit_by_name::<()>("show-error",&[&e.to_string()]);
                            },
                        }
                    }));
                    break;
                }
            }
        }));

        manager.connect_device_error(clone!(@weak redir => move |_, _, e| {
            redir.emit_by_name::<()>("show-error",&[&e.to_string()]);
        }));

        let free_channels = manager.free_channels();
        log::debug!("free_channels: {}", free_channels);
        manager
            .bind_property("free-channels", &redir, "free-channels")
            .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
            .build();
        manager.connect_free_channels_notify(|manager| {
            log::debug!("Free USB channels: {}", manager.free_channels());
        });

        Ok(redir)
    }
}

fn same_device(spice: &spice::UsbDevice, rdw: &rdw::UsbDevice) -> bool {
    let spice_device = match spice.libusb_device() {
        Some(d) => format!("{:?}", d),
        _ => return false,
    };
    let rdw_device = match rdw.device() {
        Some(d) => format!("{:?}", d),
        _ => return false,
    };

    rdw_device == spice_device
}

async fn set_device_state(
    dev: spice::UsbDevice,
    manager: spice::UsbDeviceManager,
    state: bool,
) -> std::result::Result<bool, glib::Error> {
    if state == manager.is_device_connected(&dev) {
        return Ok(state);
    }
    if state {
        manager.connect_device_future(&dev).await.map(|_| true)
    } else {
        manager.disconnect_device_future(&dev).await.map(|_| false)
    }
}
