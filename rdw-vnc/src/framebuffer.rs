use glib::translate::ToGlibPtrMut;
use gtk::prelude::*;
use gtk::{glib, subclass::prelude::ObjectSubclassExt};

use gvnc::{subclass::base_framebuffer::*, FramebufferExt};

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;
    use once_cell::sync::OnceCell;

    #[derive(Debug, Default)]
    pub struct Framebuffer {
        pub(crate) buffer: OnceCell<Vec<u8>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Framebuffer {
        const NAME: &'static str = "RdwVncFramebuffer";
        type ParentType = gvnc::BaseFramebuffer;
        type Type = super::Framebuffer;
    }

    impl ObjectImpl for Framebuffer {}

    impl BaseFramebufferImpl for Framebuffer {}
}

glib::wrapper! {
    pub struct Framebuffer(ObjectSubclass<imp::Framebuffer>) @extends gvnc::BaseFramebuffer, @implements gvnc::Framebuffer;
}

impl Framebuffer {
    pub fn new(width: u16, height: u16, remote_format: &gvnc::PixelFormat) -> Self {
        let width = width as i32;
        let height = height as i32;
        let local_format = gvnc::PixelFormat::new_with(
            (255, 255, 255),
            (16, 8, 0),
            32,
            32,
            gvnc::ByteOrder::Little,
            1,
        )
        .unwrap();

        let buffer = vec![0; (width * height * 4) as usize];
        let mut value = glib::Value::from_type(glib::Type::POINTER);
        unsafe {
            glib::gobject_ffi::g_value_set_pointer(
                value.to_glib_none_mut().0,
                buffer.as_ptr() as _,
            );
        };
        let fb = glib::Object::with_values(
            Self::static_type(),
            &[
                ("buffer", value),
                ("width", width.to_value()),
                ("height", height.to_value()),
                ("rowstride", (width * 4).to_value()),
                ("local-format", local_format.to_value()),
                ("remote-format", remote_format.to_value()),
            ],
        )
        .unwrap()
        .downcast()
        .unwrap();
        let self_ = imp::Framebuffer::from_instance(&fb);
        self_.buffer.set(buffer).unwrap();
        fb
    }

    pub fn get_sub(&self, x: usize, y: usize, w: usize, h: usize) -> &[u8] {
        let self_ = imp::Framebuffer::from_instance(self);
        let b = self_.buffer.get().unwrap();
        let bw: usize = self.get_width().into();
        let start = (x + y * bw) * 4;
        let end = (x + w + (y + h - 1) * bw) * 4;
        &b[start..end]
    }
}
