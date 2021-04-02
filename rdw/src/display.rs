use gtk::{gdk, glib};

pub mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

    #[repr(C)]
    pub struct RdwDisplayClass {
        pub parent_class: gtk::ffi::GtkGLAreaClass,
    }

    unsafe impl ClassStruct for RdwDisplayClass {
        type Type = Display;
    }

    #[repr(C)]
    pub struct RdwDisplay {
        parent: gtk::ffi::GtkGLArea,
    }

    impl std::fmt::Debug for RdwDisplay {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwDisplay")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwDisplay {
        type Type = Display;
    }

    #[derive(Default)]
    pub struct Display {}

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwDisplay";
        type Type = super::Display;
        type ParentType = gtk::GLArea;
        type Class = RdwDisplayClass;
        type Instance = RdwDisplay;
    }

    impl ObjectImpl for Display {}

    impl WidgetImpl for Display {}

    impl GLAreaImpl for Display {
        fn render(&self, _gl_area: &Self::Type, _context: &gdk::GLContext) -> bool {
            false
        }
    }

    impl Display {}

    pub trait DisplayImpl: DisplayImplExt + GLAreaImpl {}

    pub trait DisplayImplExt: ObjectSubclass {}

    unsafe impl<T: GLAreaImpl> IsSubclassable<T> for super::Display {
        fn class_init(class: &mut glib::Class<Self>) {
            <gtk::Widget as IsSubclassable<T>>::class_init(class);
        }

        fn instance_init(instance: &mut glib::subclass::InitializingObject<T>) {
            <gtk::Widget as IsSubclassable<T>>::instance_init(instance);
        }
    }
}

glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends gtk::GLArea;
}
