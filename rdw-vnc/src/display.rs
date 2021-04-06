use glib::{clone, subclass::prelude::*, translate::*};
use gtk::{glib, prelude::*};

use rdw::DisplayExt;

mod imp {
    use std::{cell::RefCell, convert::TryInto};

    use super::*;
    use crate::framebuffer::*;
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
        pub(crate) fb: RefCell<Option<Framebuffer>>,
        pub(crate) keycode_map: Option<()>,
        pub(crate) allow_lossy: bool,
    }

    impl Default for DisplayVnc {
        fn default() -> Self {
            Self {
                connection: gvnc::Connection::new(),
                fb: RefCell::new(None),
                keycode_map: None,
                allow_lossy: true,
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
                use gvnc::ConnectionAuth::*;
                log::debug!("auth-choose-type: {:?}", va);

                let prefer_auth = [
                    // Both these two provide TLS based auth, and can layer
                    // all the other auth types on top. So these two must
                    // be the first listed
                    Vencrypt, Tls, // Then stackable auth types in order of preference
                    Sasl, Mslogonii, Mslogon, Ard, Vnc, None, // Or nothing at all
                ];
                for auth in &prefer_auth {
                    for a in va.iter() {
                        if a.get::<gvnc::ConnectionAuth>().unwrap() == Some(*auth) {
                            if let Err(e) = conn.set_auth_type(auth.to_glib().try_into().unwrap()) {
                                log::warn!("Failed to set auth type: {}", e);
                                conn.shutdown();
                            }
                            return;
                        }
                    }
                }

                log::debug!("No preferred auth type found");
                conn.shutdown();
            });
            self.connection
                .connect_vnc_initialized(clone!(@weak obj => move |conn| {
                    let self_ = imp::DisplayVnc::from_instance(&obj);
                    if let Err(e) = self_.on_initialized() {
                        log::warn!("Failed to initialize: {}", e);
                        conn.shutdown();
                    }
                }));
            self.connection.connect_vnc_cursor_changed(clone!(@weak obj => move |_, cursor| {
                log::debug!("cursor-changed: {:?}", &cursor);
                obj.define_cursor(
                    cursor.map(|c|{
                        let (w, h, hot_x, hot_y, data) = (c.get_width(), c.get_height(), c.get_hotx(), c.get_hoty(), c.get_data());
                        rdw::Display::make_cursor(data, w.into(), h.into(), hot_x.into(), hot_y.into(), obj.get_scale_factor())
                    })
                );
            }));
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
            self.connection
                .connect_vnc_desktop_resize(clone!(@weak obj => move |_, w, h| {
                    let self_ = imp::DisplayVnc::from_instance(&obj);
                    log::debug!("desktop-resize: {:?}", (w, h));
                    self_.do_framebuffer_init();
                    if let Err(e) = self_.framebuffer_update_request() {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }));
            self.connection.connect_vnc_desktop_rename(|_, name| {
                log::debug!("desktop-rename: {}", name);
            });
            self.connection
                .connect_vnc_pixel_format_changed(clone!(@weak obj => move |_, format| {
                    let self_ = imp::DisplayVnc::from_instance(&obj);
                    log::debug!("pixel-format-changed: {:?}", format);
                    self_.do_framebuffer_init();
                    if let Err(e) = self_.framebuffer_update_request() {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }));
            self.connection.connect_vnc_auth_credential(|_, va| {
                log::debug!("auth-credential: {:?}", va);
            });
        }
    }

    impl WidgetImpl for DisplayVnc {}

    impl GLAreaImpl for DisplayVnc {}

    impl rdw::DisplayImpl for DisplayVnc {}

    impl DisplayVnc {
        fn do_framebuffer_init(&self) {
            let remote_format = self.connection.get_pixel_format().unwrap();
            let (width, height) = (self.connection.get_width(), self.connection.get_height());
            let fb = Framebuffer::new(
                width.try_into().unwrap(),
                height.try_into().unwrap(),
                &remote_format,
            );
            self.connection.set_framebuffer(&fb).unwrap();
            self.fb.replace(Some(fb));
        }

        fn framebuffer_update_request(&self) -> Result<(), glib::BoolError> {
            self.connection.framebuffer_update_request(
                false,
                0,
                0,
                self.connection.get_width().try_into().unwrap(),
                self.connection.get_height().try_into().unwrap(),
            )
        }

        fn on_initialized(&self) -> Result<(), glib::BoolError> {
            use gvnc::ConnectionEncoding::*;
            log::debug!("on_initialized");

            // The order determines which encodings the
            // server prefers when it has a choice to use
            let mut enc = vec![
                TightJpeg5,
                Tight,
                Xvp,
                ExtKeyEvent,
                LedState,
                ExtendedDesktopResize,
                DesktopResize,
                DesktopName,
                LastRect,
                Wmvi,
                Audio,
                AlphaCursor,
                RichCursor,
                Xcursor,
                PointerChange,
                Zrle,
                Hextile,
                Rre,
                CopyRect,
                Raw,
            ];

            let mut format = self.connection.get_pixel_format().unwrap();
            log::debug!("format: {:?}", format);
            format.set_byte_order(gvnc::ByteOrder::Little);
            self.connection.set_pixel_format(&format)?;

            self.do_framebuffer_init();

            fn pixbuf_supports(fmt: &str) -> bool {
                gtk::gdk_pixbuf::Pixbuf::get_formats()
                    .iter()
                    .any(|f| f.get_name().map_or(false, |name| name.as_str() == fmt))
            }

            if pixbuf_supports("jpeg") {
                if !self.allow_lossy {
                    enc.retain(|x| *x != TightJpeg5);
                }
            } else {
                enc.retain(|x| *x != TightJpeg5);
                enc.retain(|x| *x != Tight);
            }

            if self.keycode_map.is_none() {
                enc.retain(|x| *x != ExtKeyEvent);
            }

            let enc: Vec<i32> = enc.into_iter().map(|x| x.to_glib()).collect();
            self.connection.set_encodings(&enc)?;

            self.framebuffer_update_request()?;
            Ok(())
        }
    }
}

glib::wrapper! {
    pub struct DisplayVnc(ObjectSubclass<imp::DisplayVnc>) @extends rdw::Display, gtk::GLArea, gtk::Widget;
}

impl DisplayVnc {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[]).unwrap()
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
