use gtk::glib;

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

    #[repr(C)]
    pub struct RdwDisplayVncClass {
        pub parent_class: rdw::imp::RdwDisplayClass,
    }

    unsafe impl ClassStruct for RdwDisplayVncClass {
        type Type = DisplayVnc;
    }

    #[repr(C)]
    pub struct RdwDisplayVnc {
        parent: rdw::imp::RdwDisplay,
    }

    impl std::fmt::Debug for RdwDisplayVnc {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwDisplayVnc")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwDisplayVnc {
        type Type = DisplayVnc;
    }

    #[derive(Debug)]
    pub struct DisplayVnc {
        connection: gvnc::Connection,
    }

    impl Default for DisplayVnc {
        fn default() -> Self {
            Self {
                connection: gvnc::Connection::new(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DisplayVnc {
        const NAME: &'static str = "RdwDisplayVnc";
        type Type = super::DisplayVnc;
        type ParentType = rdw::Display;
        type Class = RdwDisplayVncClass;
        type Instance = RdwDisplayVnc;
    }

    impl ObjectImpl for DisplayVnc {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);
        }
    }

    impl WidgetImpl for DisplayVnc {}

    impl GLAreaImpl for DisplayVnc {}

    impl DisplayVnc {}
}

glib::wrapper! {
    pub struct DisplayVnc(ObjectSubclass<imp::DisplayVnc>) @extends rdw::Display, gtk::Widget;
}

impl DisplayVnc {
    pub fn new() -> Self {
        glib::Object::new::<DisplayVnc>(&[]).unwrap()
    }
}

impl Default for DisplayVnc {
    fn default() -> Self {
        Self::new()
    }
}
