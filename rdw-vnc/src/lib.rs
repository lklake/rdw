use glib::{subclass::prelude::*, translate::*};
use gtk::glib;

mod imp {
    use std::convert::TryInto;

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
        pub(crate) connection: gvnc::Connection,
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

            self.connection.connect_vnc_auth_choose_type(|conn, va| {
                log::debug!("auth-choose-type: {:?}", va);

                let prefer_auth = [
                    // Both these two provide TLS based auth, and can layer
                    // all the other auth types on top. So these two must
                    // be the first listed
                    gvnc::ConnectionAuth::Vencrypt,
                    gvnc::ConnectionAuth::Tls,
                    // Then stackable auth types in order of preference
                    gvnc::ConnectionAuth::Sasl,
                    gvnc::ConnectionAuth::Mslogonii,
                    gvnc::ConnectionAuth::Mslogon,
                    gvnc::ConnectionAuth::Ard,
                    gvnc::ConnectionAuth::Vnc,
                    // Or nothing at all
                    gvnc::ConnectionAuth::None,
                ];
                for auth in &prefer_auth {
                    for a in va.iter() {
                        if a.get::<gvnc::ConnectionAuth>().unwrap() == Some(*auth) {
                            conn.set_auth_type(auth.to_glib().try_into().unwrap());
                            return;
                        }
                    }
                }

                log::debug!("No preferred auth type found");
                conn.shutdown();
            });
            self.connection.connect_vnc_initialized(|_| {
                log::debug!("initialized");
            });
            self.connection.connect_vnc_cursor_changed(|_, cursor| {
                log::debug!("cursor-changed: {:?}", &cursor);
            });
            self.connection.connect_vnc_pointer_mode_changed(|_, abs| {
                log::debug!("pointer-mode-changed: {}", abs);
            });
            self.connection.connect_vnc_server_cut_text(|_, text| {
                log::debug!("server-cut-text: {}", text);
            });
            self.connection
                .connect_vnc_framebuffer_update(|_, x, y, w, h| {
                    log::debug!("framebuffer-update: {:?}", (x, y, w, h));
                });
            self.connection.connect_vnc_desktop_resize(|_, w, h| {
                log::debug!("desktop-resize: {:?}", (w, h));
            });
            self.connection.connect_vnc_desktop_rename(|_, name| {
                log::debug!("desktop-rename: {}", name);
            });
            self.connection
                .connect_vnc_pixel_format_changed(|_, format| {
                    log::debug!("pixel-format-changed: {:?}", format);
                });
            self.connection.connect_vnc_auth_credential(|_, va| {
                log::debug!("auth-credential: {:?}", va);
            });
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

    pub fn connection(&self) -> gvnc::Connection {
        let self_ = imp::DisplayVnc::from_instance(self);

        self_.connection.clone()
    }
}

impl Default for DisplayVnc {
    fn default() -> Self {
        Self::new()
    }
}
