use std::convert::TryFrom;

use glib::{clone, subclass::prelude::*};
use gtk::{glib, prelude::*};
use spice::ChannelExt;
use spice_client_glib as spice;

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

    #[derive(Debug, Default)]
    pub struct DisplaySpice {
        pub(crate) session: spice::Session,
        pub(crate) main: glib::WeakRef<spice::MainChannel>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DisplaySpice {
        const NAME: &'static str = "RdwDisplaySpice";
        type Type = super::DisplaySpice;
        type ParentType = rdw::Display;
        type Class = RdwDisplaySpiceClass;
        type Instance = RdwDisplaySpice;
    }

    impl ObjectImpl for DisplaySpice {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);
            let session = &self.session;

            session.connect_channel_new(clone!(@weak obj => move |_session, channel| {
                use spice::ChannelType::*;
                let self_ = Self::from_instance(&obj);

                let type_ = match spice::ChannelType::try_from(channel.get_property_channel_type()) {
                    Ok(t) => t,
                    _ => return,
                };

                match type_ {
                    Main => {
                        let main = channel.clone().downcast::<spice::MainChannel>().unwrap();
                        main.connect_main_mouse_update(clone!(@weak obj => move |main| {
                            let self_ = Self::from_instance(&obj);
                            dbg!((self_, main.get_property_mouse_mode()));
                        }));
                        self_.main.set(Some(&main));
                    },
                    Inputs => {
                        let input = channel.clone().downcast::<spice::InputsChannel>().unwrap();
                        input.connect_inputs_modifiers(clone!(@weak obj => move |input| {
                            let modifiers = input.get_property_key_modifiers();
                            log::debug!("inputs-modifiers: {}", modifiers)
                        }));
                        spice::ChannelExt::connect(&input);
                    }
                    Display => {
                        let dpy = channel.clone().downcast::<spice::DisplayChannel>().unwrap();
                        dpy.connect_property_gl_scanout_notify(|dpy| {
                            log::debug!("notify::gl-scanout: {:?}", dpy.get_gl_scanout());
                            dbg!(dpy.get_gl_scanout().unwrap().fd());
                        });
                        dpy.connect_gl_draw(|_dpy, x, y, w, h| {
                            log::debug!("gl-draw: {:?}", (x, y, w, h));
                        });
                        spice::ChannelExt::connect(&dpy);
                    },
                    _ => return,
                }
            }));
        }
    }

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
