use std::convert::TryFrom;

use glib::{clone, subclass::prelude::*};
use gtk::{gio, glib, prelude::*};
use spice::ChannelExt;
use spice_client_glib as spice;
use keycodemap::KEYMAP_XORGEVDEV2XTKBD;
use rdw::DisplayExt;

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
        pub(crate) input: glib::WeakRef<spice::InputsChannel>,
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

            obj.connect_key_press(clone!(@weak obj => move |_, keyval, keycode| {
                let self_ = Self::from_instance(&obj);
                log::debug!("key-press: {:?}", (keyval, keycode));
                // TODO: get the correct keymap according to gdk display type
                if let Some(xt) = KEYMAP_XORGEVDEV2XTKBD.get(keycode as usize) {
                    if let Some(input) = self_.input.upgrade() {
                        input.key_press(*xt as _);
                    }
                }
            }));

            obj.connect_key_release(clone!(@weak obj => move |_, keyval, keycode| {
                let self_ = Self::from_instance(&obj);
                log::debug!("key-release: {:?}", (keyval, keycode));
                // TODO: get the correct keymap according to gdk display type
                if let Some(xt) = KEYMAP_XORGEVDEV2XTKBD.get(keycode as usize) {
                    if let Some(input) = self_.input.upgrade() {
                        input.key_release(*xt as _);
                    }
                }
            }));

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
                            log::debug!("mouse-update: {}", main.get_property_mouse_mode());
                        }));
                        self_.main.set(Some(&main));
                    },
                    Inputs => {
                        let input = channel.clone().downcast::<spice::InputsChannel>().unwrap();
                        input.connect_inputs_modifiers(clone!(@weak obj => move |input| {
                            let modifiers = input.get_property_key_modifiers();
                            log::debug!("inputs-modifiers: {}", modifiers);
                            input.connect_channel_event(clone!(@weak obj => move |input, event| {
                                if event == spice::ChannelEvent::Opened {
                                    if input.get_property_socket().unwrap().get_family() == gio::SocketFamily::Unix {
                                        log::debug!("on unix socket");
                                    }
                                }
                            }));
                        }));
                        self_.input.set(Some(&input));
                        spice::ChannelExt::connect(&input);
                    }
                    Display => {
                        let dpy = channel.clone().downcast::<spice::DisplayChannel>().unwrap();
                        dpy.connect_display_primary_create(|dpy| {
                            let mut primary = spice::DisplayPrimary::new();
                            if !dpy.get_primary(0, &mut primary) {
                                log::warn!("primary-create: failed to get primary");
                                return;
                            }
                            log::debug!("primary-create: {:?}", primary);
                        });
                        dpy.connect_display_primary_destroy(|_| {
                            log::debug!("primary-destroy");
                        });
                        dpy.connect_display_mark(|_, mark| {
                            log::debug!("primary-mark: {}", mark);
                        });
                        dpy.connect_display_invalidate(|_, x, y, w, h| {
                            log::debug!("primary-invalidate: {:?}", (x, y, w, h));
                        });
                        dpy.connect_property_gl_scanout_notify(|dpy| {
                            let scanout = dpy.get_gl_scanout();
                            log::debug!("notify::gl-scanout: {:?}", scanout);
                        });
                        dpy.connect_property_monitors_notify(|_dpy| {
                            //let monitors = dpy.get_monitors();
                            log::debug!("notify::monitors: todo");
                        });
                        dpy.connect_gl_draw(|_dpy, x, y, w, h| {
                            log::debug!("gl-draw: {:?}", (x, y, w, h));
                        });
                        spice::ChannelExt::connect(&dpy);
                    },
                    Cursor => {
                        let cursor = channel.clone().downcast::<spice::CursorChannel>().unwrap();
                        cursor.connect_cursor_move(|_cursor, x, y| {
                            log::debug!("cursor-move: {:?}", (x, y));
                        });
                        cursor.connect_cursor_reset(|_cursor| {
                            log::debug!("cursor-reset");
                        });
                        cursor.connect_cursor_hide(|_cursor| {
                            log::debug!("cursor-hide");
                        });
                        cursor.connect_property_cursor_notify(|cursor| {
                            let cursor = cursor.get_property_cursor();
                            log::debug!("cursor-notify: {:?}", cursor);
                        });
                    }
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
