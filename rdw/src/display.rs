use gtk::{gdk, glib};
use gtk::prelude::*;

use crate::{egl, error::Error};

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

        fn class_init(_klass: &mut Self::Class) {
            // Assume EGL for now, done at class init time but could be done lazily?
            let egl = egl::egl();

            gl::load_with(|s| {
                egl.get_proc_address(s)
                    .map(|f| f as _)
                    .unwrap_or(std::ptr::null())
            });
        }
    }

    impl ObjectImpl for Display {}

    impl WidgetImpl for Display {
        fn realize(&self, widget: &Self::Type) {
            widget.set_has_depth_buffer(false);
            widget.set_has_stencil_buffer(false);
            widget.set_auto_render(false);
            widget.set_required_version(3, 2);
            self.parent_realize(widget);
            widget.make_current();

            if let Err(e) = unsafe { self.realize_gl() } {
                let e = glib::Error::new(Error::GL, &e);
                widget.set_error(Some(&e));
            }
        }
    }

    impl GLAreaImpl for Display {
        fn render(&self, _gl_area: &Self::Type, _context: &gdk::GLContext) -> bool {
            unsafe {
                gl::ClearColor(0.1, 0.1, 0.1, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::BLEND);
            }
            false
        }
    }

    impl Display {
        unsafe fn realize_gl(&self) -> Result<(), String> {
            Ok(())
        }
    }

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
