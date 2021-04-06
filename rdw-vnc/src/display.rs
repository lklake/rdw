use glib::{clone, subclass::prelude::*, translate::*};
use gtk::{glib, prelude::*};
use gvnc::FramebufferExt;

use keycodemap::KEYMAP_XORGEVDEV2QNUM;
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
        pub(crate) keycode_map: bool,
        pub(crate) allow_lossy: bool,
    }

    impl Default for DisplayVnc {
        fn default() -> Self {
            Self {
                connection: gvnc::Connection::new(),
                fb: RefCell::new(None),
                keycode_map: true,
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

            obj.connect_key_press(clone!(@weak obj => move |_, keyval, keycode| {
                let self_ = Self::from_instance(&obj);
                log::debug!("key-press: {:?}", (keyval, keycode));
                self_.key_event(true, keyval, keycode);
            }));

            obj.connect_key_release(clone!(@weak obj => move |_, keyval, keycode| {
                let self_ = Self::from_instance(&obj);
                log::debug!("key-release: {:?}", (keyval, keycode));
                self_.key_event(false, keyval, keycode);
            }));

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

                log::warn!("No preferred auth type found");
                conn.shutdown();
            });

            self.connection
                .connect_vnc_initialized(clone!(@weak obj => move |conn| {
                    let self_ = Self::from_instance(&obj);
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

            self.connection.connect_vnc_framebuffer_update(
                clone!(@weak obj => move |_, x, y, w, h| {
                    let self_ = Self::from_instance(&obj);
                    log::debug!("framebuffer-update: {:?}", (x, y, w, h));
                    if let Some(fb) = &*self_.fb.borrow() {
                        let sub = fb.get_sub(
                            x.try_into().unwrap(),
                            y.try_into().unwrap(),
                            w.try_into().unwrap(),
                            h.try_into().unwrap()
                        );
                        obj.update_area(x, y, w, h, (fb.get_width() * 4).into(), sub);
                    }
                    if let Err(e) = self_.framebuffer_update_request(true) {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }),
            );

            self.connection
                .connect_vnc_desktop_resize(clone!(@weak obj => move |_, w, h| {
                    let self_ = Self::from_instance(&obj);
                    log::debug!("desktop-resize: {:?}", (w, h));
                    self_.do_framebuffer_init();
                    obj.set_display_size(Some((w.try_into().unwrap(), h.try_into().unwrap())));
                    if let Err(e) = self_.framebuffer_update_request(false) {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }));

            self.connection.connect_vnc_desktop_rename(|_, name| {
                log::debug!("desktop-rename: {}", name);
            });

            self.connection.connect_vnc_pixel_format_changed(
                clone!(@weak obj => move |_, format| {
                    let self_ = Self::from_instance(&obj);
                    log::debug!("pixel-format-changed: {:?}", format);
                    self_.do_framebuffer_init();
                    if let Err(e) = self_.framebuffer_update_request(false) {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }),
            );

            self.connection.connect_vnc_auth_credential(|_, va| {
                log::debug!("auth-credential: {:?}", va);
            });
        }
    }

    impl WidgetImpl for DisplayVnc {}

    impl GLAreaImpl for DisplayVnc {}

    impl rdw::DisplayImpl for DisplayVnc {}

    impl DisplayVnc {
        fn key_event(&self, press: bool, keyval: u32, keycode: u32) {
            // TODO: get the correct keymap according to gdk display type
            if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                if let Err(e) = self.connection.key_event(press, keyval, *qnum) {
                    log::warn!("Failed to send key event: {}", e);
                }
            }
        }

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

        fn framebuffer_update_request(&self, incremental: bool) -> Result<(), glib::BoolError> {
            self.connection.framebuffer_update_request(
                incremental,
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

            if self.keycode_map {
                enc.retain(|x| *x != ExtKeyEvent);
            }

            let enc: Vec<i32> = enc.into_iter().map(|x| x.to_glib()).collect();
            self.connection.set_encodings(&enc)?;

            self.framebuffer_update_request(false)?;
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
