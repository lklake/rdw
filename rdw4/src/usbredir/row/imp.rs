use std::cell::RefCell;

use glib::ParamSpecObject;
use glib::{clone, ParamSpec};
use gtk::{glib, prelude::*, subclass::prelude::*, CompositeTemplate};
use once_cell::sync::Lazy;

#[derive(Debug, Default, CompositeTemplate)]
#[template(file = "row.ui")]
pub struct Row {
    #[template_child]
    pub label: TemplateChild<gtk::Label>,
    #[template_child]
    pub switch: TemplateChild<gtk::Switch>,

    pub device: RefCell<Option<super::Device>>,
}

#[glib::object_subclass]
impl ObjectSubclass for Row {
    const NAME: &'static str = "RdwUsbRow";
    type Type = super::Row;
    type ParentType = gtk::Widget;

    fn class_init(klass: &mut Self::Class) {
        Self::bind_template(klass);
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for Row {
    fn properties() -> &'static [ParamSpec] {
        static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
            vec![ParamSpecObject::new(
                "device",
                "Device",
                "The associated device",
                super::Device::static_type(),
                glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
            )]
        });
        PROPERTIES.as_ref()
    }

    fn set_property(
        &self,
        _obj: &Self::Type,
        _id: usize,
        value: &glib::Value,
        pspec: &glib::ParamSpec,
    ) {
        match pspec.name() {
            "device" => {
                let device = value
                    .get()
                    .expect("type conformity checked by 'Object::set_property'");
                self.device.replace(device);
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "device" => self.device.borrow().to_value(),
            _ => unimplemented!(),
        }
    }

    fn constructed(&self, obj: &Self::Type) {
        self.parent_constructed(obj);

        if let Some(device) = &*self.device.borrow() {
            device
                .bind_property("name", &*self.label, "label")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();
            device
                .bind_property("active", &*self.switch, "active")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();
            // because we are waiting for state changes
            device
                .bind_property("active", &*self.switch, "state")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();
        }

        self.switch.connect_state_set(
            clone!(@weak obj as this => @default-panic, move |s, state| {
                let imp = Self::from_instance(&this);
                if let Some(device) = &*imp.device.borrow() {
                    device.emit_by_name::<()>("state-set", &[&state]);
                } else {
                    s.set_state(false);
                }
                gtk::Inhibit(true)
            }),
        );
    }

    // Needed for direct subclasses of GtkWidget;
    // Here you need to unparent all direct children
    // of your template.
    fn dispose(&self, obj: &Self::Type) {
        while let Some(child) = obj.first_child() {
            child.unparent();
        }
    }
}

impl WidgetImpl for Row {}
