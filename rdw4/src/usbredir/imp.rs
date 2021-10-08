use super::*;
use glib::{clone, subclass::Signal, ParamSpec};
use gtk::CompositeTemplate;
use once_cell::sync::Lazy;
use rusb::UsbContext;
use std::{
    cell::{Cell, RefCell},
    thread,
};
use usbredirhost::rusb;

#[derive(Debug)]
enum RdwUsbEvent<T: UsbContext> {
    DeviceArrived(rusb::Device<T>),
    DeviceLeft(rusb::Device<T>),
}

#[derive(Debug)]
struct RdwUsbHandler<T: UsbContext> {
    pub tx: glib::Sender<RdwUsbEvent<T>>,
}

impl<T: UsbContext> rusb::Hotplug<T> for RdwUsbHandler<T> {
    fn device_arrived(&mut self, device: rusb::Device<T>) {
        let _ = self.tx.send(RdwUsbEvent::DeviceArrived(device));
    }

    fn device_left(&mut self, device: rusb::Device<T>) {
        let _ = self.tx.send(RdwUsbEvent::DeviceLeft(device));
    }
}

#[derive(Debug)]
struct RdwUsbContext {
    #[allow(unused)]
    pub ctxt: rusb::Context,
    #[allow(unused)]
    pub reg: rusb::Registration<rusb::Context>,
}

impl RdwUsbContext {
    fn new() -> Option<(Self, glib::Receiver<RdwUsbEvent<rusb::Context>>)> {
        let ctxt = match rusb::Context::new() {
            Ok(ctxt) => ctxt,
            Err(e) => {
                log::warn!("Failed to create USB context: {}", e);
                return None;
            }
        };

        let (tx, rx) = glib::MainContext::channel(glib::source::Priority::default());
        let reg = match rusb::HotplugBuilder::new()
            .enumerate(true)
            .register(&ctxt, Box::new(RdwUsbHandler { tx }))
        {
            Ok(reg) => reg,
            Err(e) => {
                log::warn!("Failed to register USB callback: {}", e);
                return None;
            }
        };

        let ctx = ctxt.clone();
        thread::spawn(move || loop {
            // note: there is a busy loop with libusb <= 1.0.24!..
            if let Err(e) = ctx.handle_events(None) {
                log::warn!("USB context failed to loop: {}", e);
                break;
            }
        });
        Some((Self { ctxt, reg }, rx))
    }
}

#[repr(C)]
pub struct RdwUsbRedirClass {
    pub parent_class: gtk::ffi::GtkWidgetClass,
}

unsafe impl ClassStruct for RdwUsbRedirClass {
    type Type = UsbRedir;
}

#[repr(C)]
pub struct RdwUsbRedir {
    parent: gtk::ffi::GtkWidget,
}

impl std::fmt::Debug for RdwUsbRedir {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("RdwUsbRedir")
            .field("parent", &self.parent)
            .finish()
    }
}

unsafe impl InstanceStruct for RdwUsbRedir {
    type Type = UsbRedir;
}

#[derive(Debug, CompositeTemplate)]
#[template(file = "usbredir.ui")]
pub struct UsbRedir {
    #[template_child]
    pub listbox: TemplateChild<gtk::ListBox>,

    #[template_child]
    pub infobar: TemplateChild<gtk::InfoBar>,

    #[template_child]
    pub error_label: TemplateChild<gtk::Label>,

    #[template_child]
    pub free_label: TemplateChild<gtk::Label>,

    pub model: gio::ListStore,

    ctxt: RefCell<Option<RdwUsbContext>>,

    free_channels: Cell<i32>,
}

impl Default for UsbRedir {
    fn default() -> Self {
        Self {
            model: gio::ListStore::new(device::Device::static_type()),
            listbox: TemplateChild::default(),
            infobar: TemplateChild::default(),
            error_label: TemplateChild::default(),
            free_label: TemplateChild::default(),
            ctxt: RefCell::default(),
            free_channels: Cell::default(),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for UsbRedir {
    const NAME: &'static str = "RdwUsbRedir";
    type Type = super::UsbRedir;
    type ParentType = gtk::Widget;
    type Class = RdwUsbRedirClass;
    type Instance = RdwUsbRedir;

    fn class_init(klass: &mut Self::Class) {
        Self::bind_template(klass);
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for UsbRedir {
    fn constructed(&self, _obj: &Self::Type) {}

    fn properties() -> &'static [ParamSpec] {
        static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
            vec![ParamSpec::new_int(
                "free-channels",
                "Free channels",
                "Number of free channels",
                -1,
                i32::MAX,
                -1,
                glib::ParamFlags::READWRITE,
            )]
        });
        PROPERTIES.as_ref()
    }

    fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "free-channels" => self.free_channels.get().to_value(),
            _ => unimplemented!(),
        }
    }

    fn set_property(&self, _tag: &Self::Type, _id: usize, value: &glib::Value, pspec: &ParamSpec) {
        match pspec.name() {
            "free-channels" => {
                let n = value.get().unwrap();
                self.free_channels.set(n);
                self.free_label.set_label(&format!("({} free channels)", n));
                self.free_label.set_visible(n >= 0);
            }
            _ => unimplemented!(),
        }
    }

    fn signals() -> &'static [Signal] {
        static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
            vec![
                Signal::builder(
                    "device-state-set",
                    &[
                        device::Device::static_type().into(),
                        bool::static_type().into(),
                    ],
                    <()>::static_type().into(),
                )
                .build(),
                Signal::builder(
                    "show-error",
                    &[String::static_type().into()],
                    // TODO: <glib::signal::Inhibit>::static_type().into(),
                    bool::static_type().into(),
                )
                .class_handler(|_token, args| {
                    let inst = args[0].get::<super::UsbRedir>().unwrap();
                    let self_ = UsbRedir::from_instance(&inst);
                    let msg: String = args[1].get().unwrap();
                    self_.error_label.set_label(&msg);
                    self_.infobar.set_revealed(true);
                    Some(true.to_value())
                })
                .accumulator(|_hint, ret, value| {
                    let handled: bool = value.get().unwrap_or_default();
                    *ret = value.clone();
                    !handled
                })
                .build(),
            ]
        });
        SIGNALS.as_ref()
    }

    fn dispose(&self, obj: &Self::Type) {
        while let Some(child) = obj.first_child() {
            child.unparent();
        }
    }
}

impl WidgetImpl for UsbRedir {
    fn realize(&self, widget: &Self::Type) {
        self.parent_realize(widget);

        if let Some((ctxt, rx)) = RdwUsbContext::new() {
            let _id = rx.attach(
                None,
                clone!(@weak widget as this => @default-return glib::Continue(false), move |ev| {
                    let self_ = Self::from_instance(&this);
                    match ev {
                        RdwUsbEvent::DeviceArrived(d) => self_.add_device(d),
                        RdwUsbEvent::DeviceLeft(d) => self_.remove_device(d),
                    }
                    glib::Continue(true)
                }),
            );
            self.ctxt.replace(Some(ctxt));
        }

        self.listbox
            .connect_row_activated(clone!(@weak widget as this => move |_, row| {
                let row: row::Row = row.first_child().unwrap().downcast().unwrap();
                row.switch().activate();
            }));

        self.listbox.bind_model(
            Some(&self.model),
            clone!(@weak widget as this => @default-panic, move |item| {
                let row = row::Row::new(item.downcast_ref().unwrap());
                row.upcast()
            }),
        );

        self.infobar.connect_response(|infobar, _id| {
            infobar.set_revealed(false);
        });
    }
}

impl UsbRedir {
    pub fn find_item<F: Fn(&Device) -> bool>(&self, test: F) -> Option<u32> {
        let mut pos = 0;
        loop {
            if let Some(item) = self.model.item(pos) {
                let item: Device = item.downcast().unwrap();
                if test(&item) {
                    break Some(pos);
                }
            } else {
                break None;
            }
            pos += 1;
        }
    }

    fn add_device(&self, d: rusb::Device<rusb::Context>) {
        match is_hub(&d) {
            Ok(true) => return,
            Err(e) => {
                log::warn!("Failed to get device details: {}", e);
                return;
            }
            _ => (),
        }
        if self.find_item(|item| item.is_device(&d)).is_some() {
            return;
        }

        let this = self.instance();
        let item = Device::new();
        item.connect_state_set(clone!(@weak this => move |device, state| {
            this.emit_by_name("device-state-set", &[device, &state]).unwrap();
        }));
        item.set_device(d);
        self.model.append(&item);
    }

    fn remove_device(&self, d: rusb::Device<rusb::Context>) {
        if let Some(pos) = self.find_item(|item| item.is_device(&d)) {
            self.model.remove(pos);
        }
    }
}

fn is_hub(d: &rusb::Device<rusb::Context>) -> rusb::Result<bool> {
    let desc = d.device_descriptor()?;
    if desc.class_code() == rusb::constants::LIBUSB_CLASS_HUB {
        return Ok(true);
    }
    match d.address() {
        0xff => Ok(true),        // root hub (HCD)
        n if n <= 1 => Ok(true), // root hub or bad address
        _ => Ok(false),
    }
}
