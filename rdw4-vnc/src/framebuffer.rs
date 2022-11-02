use glib::translate::ToGlibPtrMut;
use gtk::{glib, subclass::prelude::ObjectSubclassExt};
use rdw::gtk;

use gvnc::{prelude::*, subclass::base_framebuffer::*};

/// cbindgen::ignore
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
    // FIXME: make it pub(crate)
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
        .downcast()
        .unwrap();
        let imp = imp::Framebuffer::from_obj(&fb);
        imp.buffer.set(buffer).unwrap();
        fb
    }

    pub fn get_sub(&self, x: usize, y: usize, w: usize, h: usize) -> &[u8] {
        let imp = imp::Framebuffer::from_obj(self);
        let buf = imp.buffer.get().unwrap();
        let bw: usize = FramebufferExt::width(self) as _;
        let start = (x + y * bw) * 4;
        let end = (x + w + (y + h - 1) * bw) * 4;
        &buf[start..end]
    }
}
