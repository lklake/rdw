use glib::subclass::prelude::*;
use gtk::{gdk, gio, glib};
use std::{future::Future, pin::Pin};

type WriteFunc = dyn Fn(
        &str,
        &gio::OutputStream,
        glib::Priority,
    ) -> Option<Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>>>
    + 'static;

pub mod imp {

    use super::*;
    use gtk::subclass::prelude::*;
    use once_cell::sync::OnceCell;

    #[repr(C)]
    pub struct RdwContentProviderClass {
        pub parent_class: gdk::ffi::GdkContentProviderClass,
    }

    unsafe impl ClassStruct for RdwContentProviderClass {
        type Type = ContentProvider;
    }

    #[repr(C)]
    pub struct RdwContentProvider {
        parent: gdk::ffi::GdkContentProvider,
    }

    impl std::fmt::Debug for RdwContentProvider {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwContentProvider")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwContentProvider {
        type Type = ContentProvider;
    }

    #[derive(Default)]
    pub struct ContentProvider {
        pub(crate) formats: OnceCell<gdk::ContentFormats>,
        pub(crate) write_future: OnceCell<Box<WriteFunc>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ContentProvider {
        const NAME: &'static str = "RdwContentProvider";
        type Type = super::ContentProvider;
        type ParentType = gdk::ContentProvider;
        type Class = RdwContentProviderClass;
        type Instance = RdwContentProvider;
    }

    impl ObjectImpl for ContentProvider {}

    impl ContentProviderImpl for ContentProvider {
        fn formats(&self, _provider: &Self::Type) -> gdk::ContentFormats {
            self.formats.get().unwrap().clone()
        }

        fn write_mime_type_future(
            &self,
            provider: &Self::Type,
            mime_type: &str,
            stream: &gio::OutputStream,
            io_priority: glib::Priority,
        ) -> Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>> {
            let imp = Self::from_instance(provider);
            let future = imp.write_future.get().unwrap()(mime_type, stream, io_priority);
            future.unwrap_or_else(|| {
                Box::pin(async move {
                    Err(glib::Error::new(
                        gio::IOErrorEnum::Failed,
                        "write_mime failed!",
                    ))
                })
            })
        }
    }
}

glib::wrapper! {
    pub struct ContentProvider(ObjectSubclass<imp::ContentProvider>) @extends gdk::ContentProvider;
}

impl ContentProvider {
    pub fn new<
        F: Fn(
                &str,
                &gio::OutputStream,
                glib::Priority,
            )
                -> Option<Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>>>
            + 'static,
    >(
        mime_types: &[&str],
        write_future: F,
    ) -> Self {
        let inst = glib::Object::new::<Self>(&[]).unwrap();
        let imp = imp::ContentProvider::from_instance(&inst);

        let mut formats = gdk::ContentFormatsBuilder::new();
        for m in mime_types {
            formats = formats.add_mime_type(m);
        }
        imp.formats.set(formats.build()).unwrap();
        assert!(imp.write_future.set(Box::new(write_future)).is_ok());
        inst
    }
}
