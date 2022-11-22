use glib::{clone, subclass::prelude::*, translate::*};
use gtk::{glib, prelude::*};
use gvnc::prelude::*;
use rdw::gtk;

use rdw::DisplayExt;

mod imp {
    use super::*;
    use crate::framebuffer::*;
    use gtk::subclass::prelude::*;
    use once_cell::sync::Lazy;
    use std::{
        cell::{Cell, RefCell},
        convert::TryInto,
    };

    #[repr(C)]
    pub struct RdwVncDisplayClass {
        pub parent_class: rdw::RdwDisplayClass,
    }

    unsafe impl ClassStruct for RdwVncDisplayClass {
        type Type = Display;
    }

    #[repr(C)]
    pub struct RdwVncDisplay {
        parent: rdw::RdwDisplay,
    }

    impl std::fmt::Debug for RdwVncDisplay {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwVncDisplay")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwVncDisplay {
        type Type = Display;
    }

    #[derive(Debug)]
    pub struct Display {
        pub(crate) connection: gvnc::Connection,
        pub(crate) fb: RefCell<Option<Framebuffer>>,
        pub(crate) keycode_map: bool,
        pub(crate) allow_lossy: bool,
        pub(crate) last_motion: Cell<Option<(f64, f64)>>,
        pub(crate) last_button_mask: Cell<Option<u8>>,
        pub(crate) keymap: Cell<Option<&'static [u16]>>,
    }

    impl Default for Display {
        fn default() -> Self {
            Self {
                fb: RefCell::new(None),
                connection: gvnc::Connection::new(),
                keycode_map: true,
                allow_lossy: true,
                last_motion: Cell::new(None),
                last_button_mask: Cell::new(None),
                keymap: Cell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwVncDisplay";
        type Type = super::Display;
        type ParentType = rdw::Display;
        type Class = RdwVncDisplayClass;
        type Instance = RdwVncDisplay;
    }

    impl ObjectImpl for Display {
        fn properties() -> &'static [glib::ParamSpec] {
            use glib::ParamFlags as Flags;

            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![glib::ParamSpecObject::new(
                    "connection",
                    "Connection",
                    "gvnc connection",
                    gvnc::Connection::static_type(),
                    Flags::READABLE,
                )]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, _value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "connection" => panic!(),
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "connection" => self.connection.to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().set_mouse_absolute(true);

            self.obj().connect_key_event(
                clone!(@weak self as this => move |_, keyval, keycode, event| {
                    log::debug!("key-press: {:?}", (keyval, keycode));
                    if event.contains(rdw::KeyEvent::PRESS) {
                        this.key_event(true, keyval, keycode);
                    }
                    if event.contains(rdw::KeyEvent::RELEASE) {
                        this.key_event(false, keyval, keycode);
                    }
                }),
            );

            self.obj()
                .connect_motion(clone!(@weak self as this => move |_, x, y| {
                    log::debug!("motion: {:?}", (x, y));
                    this.last_motion.set(Some((x, y)));
                    if !this.obj().mouse_absolute() {
                        return;
                    }
                    let button_mask = this.last_button_mask();
                    if let Err(e) = this.connection.pointer_event(button_mask, x as _, y as _) {
                        log::warn!("Failed to send pointer event: {}", e);
                    }
                }));

            self.obj()
                .connect_motion_relative(clone!(@weak self as this => move |_, dx, dy| {
                    log::debug!("motion-relative: {:?}", (dx, dy));
                    if this.obj().mouse_absolute() {
                        return;
                    }
                    let button_mask = this.last_button_mask();
                    let (dx, dy) = (dx as i32 + 0x7fff, dy as i32 + 0x7fff);
                    if let Err(e) = this.connection.pointer_event(button_mask, dx as _, dy as _) {
                        log::warn!("Failed to send pointer event: {}", e);
                    }
                }));

            self.obj()
                .connect_mouse_press(clone!(@weak self as this => move |_, button| {
                    log::debug!("mouse-press: {:?}", button);
                    this.mouse_click(true, button);
                }));

            self.obj()
                .connect_mouse_release(clone!(@weak self as this => move |_, button| {
                    log::debug!("mouse-release: {:?}", button);
                    this.mouse_click(false, button);
                }));

            self.obj()
                .connect_scroll_discrete(clone!(@weak self as this => move |_, scroll| {
                    log::debug!("scroll-discrete: {:?}", scroll);
                    this.scroll(scroll);
                }));

            self.obj().connect_resize_request(
                clone!(@weak self as this => move |_, width, height, wmm, hmm| {
                    let sf = this.obj().scale_factor() as u32;
                    let (width, height) = (width / sf, height / sf);
                    let status = this.connection.set_size(width, height);
                    log::debug!("resize-request: {:?} -> {:?}", (width, height, wmm, hmm), status);
                }),
            );

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
                for &auth in &prefer_auth {
                    for a in va.iter() {
                        if a.get::<gvnc::ConnectionAuth>().unwrap() == auth {
                            if let Err(e) = conn.set_auth_type(auth.into_glib().try_into().unwrap())
                            {
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
                .connect_vnc_initialized(clone!(@weak self as this => move |conn| {
                    if let Err(e) = this.on_initialized() {
                        log::warn!("Failed to initialize: {}", e);
                        conn.shutdown();
                    }
                }));

            self.connection.connect_vnc_cursor_changed(clone!(@weak self as this => move |_, cursor| {
                log::debug!("cursor-changed: {:?}", &cursor);
                this.obj().define_cursor(
                    cursor.map(|c|{
                        let (w, h, hot_x, hot_y, data) = (c.width(), c.height(), c.hotx(), c.hoty(), c.data());
                        rdw::Display::make_cursor(data, w.into(), h.into(), hot_x.into(), hot_y.into(), 1)
                    })
                );
            }));

            self.connection.connect_vnc_pointer_mode_changed(
                clone!(@weak self as this => move |_, abs| {
                    log::debug!("pointer-mode-changed: {}", abs);
                    this.obj().set_mouse_absolute(abs);
                }),
            );

            self.connection.connect_vnc_server_cut_text(|_, text| {
                log::debug!("server-cut-text: {}", text);
            });

            self.connection.connect_vnc_framebuffer_update(
                clone!(@weak self as this => move |_, x, y, w, h| {
                    log::debug!("framebuffer-update: {:?}", (x, y, w, h));
                    if let Some(fb) = &*this.fb.borrow() {
                        let sub = fb.get_sub(
                            x as _,
                            y as _,
                            w as _,
                            h as _,
                        );
                        this.obj().update_area(x, y, w, h, BaseFramebufferExt::width(fb) * 4, sub);
                    }
                    if let Err(e) = this.framebuffer_update_request(true) {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }),
            );

            self.connection.connect_vnc_desktop_resize(
                clone!(@weak self as this => move |_, w, h| {
                    log::debug!("desktop-resize: {:?}", (w, h));
                    this.do_framebuffer_init();
                    this.obj().set_display_size(Some((w as _, h as _)));
                    if let Err(e) = this.framebuffer_update_request(false) {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }),
            );

            self.connection.connect_vnc_desktop_rename(|_, name| {
                log::debug!("desktop-rename: {}", name);
            });

            self.connection.connect_vnc_pixel_format_changed(
                clone!(@weak self as this => move |_, format| {
                    log::debug!("pixel-format-changed: {:?}", format);
                    this.do_framebuffer_init();
                    if let Err(e) = this.framebuffer_update_request(false) {
                        log::warn!("Failed to update framebuffer: {}", e);
                    }
                }),
            );

            self.connection.connect_vnc_auth_credential(|_, va| {
                log::debug!("auth-credential: {:?}", va);
            });
        }
    }

    impl WidgetImpl for Display {
        fn realize(&self) {
            self.parent_realize();

            self.keymap.set(rdw::keymap_qnum());
        }
    }

    impl rdw::DisplayImpl for Display {}

    impl Display {
        fn last_button_mask(&self) -> u8 {
            self.last_button_mask.get().unwrap_or(0)
        }

        fn key_event(&self, press: bool, keyval: u32, keycode: u32) {
            // TODO: get the correct keymap according to gdk display type
            if let Some(qnum) = self.keymap.get().and_then(|m| m.get(keycode as usize)) {
                if let Err(e) = self.connection.key_event(press, keyval, *qnum) {
                    log::warn!("Failed to send key event: {}", e);
                }
            }
        }

        fn button_event(&self, press: bool, button: u8) {
            let obj = self.obj();
            let (x, y) = if obj.mouse_absolute() {
                self.last_motion
                    .get()
                    .map_or((0, 0), |(x, y)| (x as _, y as _))
            } else {
                (0x7fff, 0x7fff)
            };
            let button = 1 << (button - 1);

            let mut button_mask = self.last_button_mask();
            if press {
                button_mask |= button;
            } else {
                button_mask &= !button;
            }
            self.last_button_mask.set(Some(button_mask));

            if let Err(e) = self.connection.pointer_event(button_mask, x, y) {
                log::warn!("Failed to send key event: {}", e);
            }
        }

        fn mouse_click(&self, press: bool, button: u32) {
            if button > 3 {
                log::warn!("Unhandled button event nth: {}", button);
                return;
            }
            self.button_event(press, button as _)
        }

        fn scroll(&self, scroll: rdw::Scroll) {
            let n = match scroll {
                rdw::Scroll::Up => 4,
                rdw::Scroll::Down => 5,
                rdw::Scroll::Left => 6,
                rdw::Scroll::Right => 7,
                _ => return,
            };
            self.button_event(true, n);
            self.button_event(false, n);
        }

        fn do_framebuffer_init(&self) {
            let remote_format = self.connection.pixel_format().unwrap();
            let (width, height) = (self.connection.width(), self.connection.height());
            let fb = Framebuffer::new(width as _, height as _, &remote_format);
            self.connection.set_framebuffer(&fb).unwrap();
            self.fb.replace(Some(fb));
        }

        fn framebuffer_update_request(&self, incremental: bool) -> Result<(), glib::BoolError> {
            self.connection.framebuffer_update_request(
                incremental,
                0,
                0,
                self.connection.width() as _,
                self.connection.height() as _,
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

            let mut format = self.connection.pixel_format().unwrap();
            log::debug!("format: {:?}", format);
            format.set_byte_order(gvnc::ByteOrder::Little);
            self.connection.set_pixel_format(&format)?;

            self.do_framebuffer_init();

            let pixbuf_supports = |fmt| {
                gtk::gdk_pixbuf::Pixbuf::formats()
                    .iter()
                    .any(|f| f.name().map_or(false, |name| name.as_str() == fmt))
            };

            if pixbuf_supports("jpeg") {
                if !self.allow_lossy {
                    enc.retain(|&x| x != TightJpeg5);
                }
            } else {
                enc.retain(|&x| x != TightJpeg5);
                enc.retain(|&x| x != Tight);
            }

            if self.keycode_map {
                enc.retain(|&x| x != ExtKeyEvent);
            }

            let enc: Vec<i32> = enc.into_iter().map(|x| x.into_glib()).collect();
            self.connection.set_encodings(&enc)?;

            self.framebuffer_update_request(false)?;
            Ok(())
        }
    }
}

glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends rdw::Display, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Display {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[])
    }

    pub fn connection(&self) -> &gvnc::Connection {
        let imp = imp::Display::from_obj(self);

        &imp.connection
    }
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}
