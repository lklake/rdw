use spice_client_glib as spice;
use glib::{clone, subclass::prelude::*, translate::*};
use gtk::{glib, prelude::*};

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

    #[repr(C)]
    pub struct RdwDisplaySpiceClass {
        pub parent_class: rdw::imp::RdwDisplayClass,
    }

    unsafe impl ClassStruct for RdwDisplaySpiceClass {
        type Type = DisplaySpice;
    }

    #[repr(C)]
    pub struct RdwDisplaySpice {
        parent: rdw::imp::RdwDisplay,
    }

    impl std::fmt::Debug for RdwDisplaySpice {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwDisplaySpice")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwDisplaySpice {
        type Type = DisplaySpice;
    }

    #[derive(Debug)]
    pub struct DisplaySpice {
        pub(crate) session: spice::Session,
    }

    impl Default for DisplaySpice {
        fn default() -> Self {
            Self {
                session: spice::Session::new(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DisplaySpice {
        const NAME: &'static str = "RdwDisplaySpice";
        type Type = super::DisplaySpice;
        type ParentType = rdw::Display;
        type Class = RdwDisplaySpiceClass;
        type Instance = RdwDisplaySpice;
    }

    impl ObjectImpl for DisplaySpice {}

    impl WidgetImpl for DisplaySpice {}

    impl rdw::DisplayImpl for DisplaySpice {}
}

glib::wrapper! {
    pub struct DisplaySpice(ObjectSubclass<imp::DisplaySpice>) @extends rdw::Display, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DisplaySpice {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[]).unwrap()
    }

    pub fn session(&self) -> &spice::Session {
        let self_ = imp::DisplaySpice::from_instance(self);

        &self_.session
    }
}
