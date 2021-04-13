use std::convert::TryFrom;

use glib::{clone, subclass::prelude::*};
use gtk::{gdk, gio, glib, prelude::*};
use keycodemap::KEYMAP_XORGEVDEV2XTKBD;
use rdw::DisplayExt;
use spice::ChannelExt;
use spice_client_glib as spice;
use std::os::unix::io::IntoRawFd;

mod imp {
    use std::cell::Cell;

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
        pub(crate) monitor_config: Cell<Option<spice::DisplayMonitorConfig>>,
        pub(crate) main: glib::WeakRef<spice::MainChannel>,
        pub(crate) input: glib::WeakRef<spice::InputsChannel>,
        pub(crate) display: glib::WeakRef<spice::DisplayChannel>,
        pub(crate) last_button_state: Cell<Option<i32>>,
        pub(crate) nth_monitor: usize,
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

            obj.set_mouse_absolute(true);

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

            obj.connect_motion(clone!(@weak obj => move |_, x, y| {
                let self_ = Self::from_instance(&obj);
                log::debug!("motion: {:?}", (x, y));
                if let Some(input) = self_.input.upgrade() {
                    input.position(x as _, y as _, self_.nth_monitor as _, self_.last_button_state());
                }
            }));

            obj.connect_motion_relative(clone!(@weak obj => move |_, dx, dy| {
                let self_ = Self::from_instance(&obj);
                log::debug!("motion-relative: {:?}", (dx, dy));
                if let Some(input) = self_.input.upgrade() {
                    input.motion(dx as _, dy as _, self_.last_button_state());
                }
            }));

            obj.connect_mouse_press(clone!(@weak obj => move |_, button| {
                let self_ = Self::from_instance(&obj);
                log::debug!("mouse-press: {:?}", button);
                self_.mouse_click(true, button);
            }));

            obj.connect_mouse_release(clone!(@weak obj => move |_, button| {
                let self_ = Self::from_instance(&obj);
                log::debug!("mouse-release: {:?}", button);
                self_.mouse_click(false, button);
            }));

            obj.connect_scroll_discrete(clone!(@weak obj => move |_, scroll| {
                let self_ = Self::from_instance(&obj);
                log::debug!("scroll-discrete: {:?}", scroll);
                self_.scroll(scroll);
            }));

            obj.connect_resize_request(clone!(@weak obj => move |_, width, height| {
                let self_ = Self::from_instance(&obj);
                log::debug!("resize-request: {:?}", (width, height));
                if let Some(main) = self_.main.upgrade() {
                    main.update_display(self_.nth_monitor as _, 0, 0, width as _, height as _, true);
                }
            }));

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
                        self_.main.set(Some(&main));

                        main.connect_channel_event(clone!(@weak obj => move |_, event| {
                            use spice::ChannelEvent::*;

                            let self_ = Self::from_instance(&obj);
                            if event == Closed {
                                self_.session.disconnect();
                            }
                        }));
                        main.connect_main_mouse_update(clone!(@weak obj => move |main| {
                            let mode = spice::MouseMode::from_bits_truncate(main.get_property_mouse_mode());
                            log::debug!("mouse-update: {:?}", mode);
                            obj.set_mouse_absolute(mode.contains(spice::MouseMode::CLIENT));
                        }));
                    },
                    Inputs => {
                        let input = channel.clone().downcast::<spice::InputsChannel>().unwrap();
                        self_.input.set(Some(&input));

                        input.connect_inputs_modifiers(clone!(@weak obj => move |input| {
                            let modifiers = input.get_property_key_modifiers();
                            log::debug!("inputs-modifiers: {}", modifiers);
                            input.connect_channel_event(clone!(@weak obj => move |input, event| {
                                if event == spice::ChannelEvent::Opened && input.get_property_socket().unwrap().get_family() == gio::SocketFamily::Unix {
                                    log::debug!("on unix socket");
                                }
                            }));
                        }));
                        spice::ChannelExt::connect(&input);
                    }
                    Display => {
                        let dpy = channel.clone().downcast::<spice::DisplayChannel>().unwrap();
                        self_.display.set(Some(&dpy));

                        dpy.connect_display_primary_create(clone!(@weak obj => move |_| {
                            log::debug!("primary-create");
                        }));

                        dpy.connect_display_primary_destroy(|_| {
                            log::debug!("primary-destroy");
                        });

                        dpy.connect_display_mark(|_, mark| {
                            log::debug!("primary-mark: {}", mark);
                        });

                        dpy.connect_display_invalidate(clone!(@weak obj => move |_, x, y, w, h| {
                            let self_ = Self::from_instance(&obj);
                            log::debug!("primary-invalidate: {:?}", (x, y, w, h));
                            self_.invalidate(x as _, y as _, w as _, h as _);
                        }));

                        dpy.connect_property_gl_scanout_notify(clone!(@weak obj => move |dpy| {
                            let scanout = dpy.get_gl_scanout();
                            log::debug!("notify::gl-scanout: {:?}", scanout);

                            if let Some(scanout) = scanout {
                                obj.set_dmabuf_scanout(rdw::DmabufScanout {
                                    width: scanout.width(),
                                    height: scanout.height(),
                                    stride: scanout.stride(),
                                    fourcc: scanout.format(),
                                    y0_top: scanout.y0_top(),
                                    modifier: 0,
                                    fd: scanout.into_raw_fd(),
                                });
                            }
                        }));

                        dpy.connect_gl_draw(clone!(@weak obj => move |dpy, x, y, w, h| {
                            log::debug!("gl-draw: {:?}", (x, y, w, h));
                            obj.render();
                            dpy.gl_draw_done();
                        }));

                        dpy.connect_property_monitors_notify(clone!(@weak obj => move |dpy| {
                            let self_ = Self::from_instance(&obj);
                            let monitors = dpy.get_property_monitors();
                            log::debug!("notify::monitors: {:?}", monitors);

                            let monitor_config = monitors.and_then(|m| m.get(self_.nth_monitor).copied());
                            if let Some((0, 0, w, h)) = monitor_config.map(|c| c.geometry()) {
                                obj.set_display_size(Some((w, h)));
                                if self_.primary().is_some() {
                                    self_.invalidate(0, 0, w, h);
                                }
                            }
                            self_.monitor_config.set(monitor_config);
                        }));

                        spice::ChannelExt::connect(&dpy);
                    },
                    Cursor => {
                        let cursor = channel.clone().downcast::<spice::CursorChannel>().unwrap();

                        cursor.connect_cursor_move(clone!(@weak obj => move |_cursor, x, y| {
                            log::debug!("cursor-move: {:?}", (x, y));
                            obj.set_cursor_position(Some((x as _, y as _)));
                        }));

                        cursor.connect_cursor_reset(|_cursor| {
                            log::debug!("cursor-reset");
                        });

                        cursor.connect_cursor_hide(clone!(@weak obj => move |_cursor| {
                            log::debug!("cursor-hide");
                            let cursor = gdk::Cursor::from_name("none", None);
                            obj.define_cursor(cursor);
                        }));

                        cursor.connect_property_cursor_notify(clone!(@weak obj => move |cursor| {
                            let cursor = cursor.get_property_cursor();
                            log::debug!("cursor-notify: {:?}", cursor);
                            if let Some(cursor) = cursor {
                                match cursor.cursor_type() {
                                    Ok(spice::CursorType::Alpha) => {
                                        let cursor = rdw::Display::make_cursor(
                                            cursor.data().unwrap(),
                                            cursor.width(),
                                            cursor.height(),
                                            cursor.hot_x(),
                                            cursor.hot_y(),
                                            obj.get_scale_factor()
                                        );
                                        obj.define_cursor(Some(cursor));
                                    }
                                    e => log::warn!("Unhandled cursor type: {:?}", e),
                                }
                            }
                        }));

                        spice::ChannelExt::connect(&cursor);
                    }
                    _ => {}
                }
            }));
        }
    }

    impl WidgetImpl for DisplaySpice {}

    impl rdw::DisplayImpl for DisplaySpice {}

    impl DisplaySpice {
        fn button_event(&self, press: bool, button: spice::MouseButton) {
            assert_ne!(button, spice::MouseButton::Invalid);

            let mut button_state = self.last_button_state();
            let button = button as i32;
            let button_mask = 1 << (button - 1);
            if press {
                button_state |= button_mask;
            } else {
                button_state &= !button_mask;
            }
            self.last_button_state.set(Some(button_state));

            if let Some(input) = self.input.upgrade() {
                if press {
                    input.button_press(button, button_state);
                } else {
                    input.button_release(button, button_state);
                }
            }
        }

        fn mouse_click(&self, press: bool, button: u32) {
            let button = match button {
                gdk::BUTTON_PRIMARY => spice::MouseButton::Left,
                gdk::BUTTON_MIDDLE => spice::MouseButton::Middle,
                gdk::BUTTON_SECONDARY => spice::MouseButton::Right,
                button => {
                    log::warn!("Unhandled button event nth: {}", button);
                    return;
                }
            };

            self.button_event(press, button);
        }

        fn scroll(&self, scroll: rdw::Scroll) {
            let n = match scroll {
                rdw::Scroll::Up => spice::MouseButton::Up,
                rdw::Scroll::Down => spice::MouseButton::Down,
                other => {
                    log::debug!("spice doesn't have scroll: {:?}", other);
                    return;
                }
            };
            self.button_event(true, n);
            self.button_event(false, n);
        }

        fn last_button_state(&self) -> i32 {
            self.last_button_state.get().unwrap_or(0)
        }

        fn primary(&self) -> Option<spice::DisplayPrimary> {
            self.monitor_config.get().and_then(|c| {
                self.display
                    .upgrade()
                    .and_then(|d| d.primary(c.surface_id()))
            })
        }

        fn invalidate(&self, x: usize, y: usize, w: usize, h: usize) {
            let obj = self.get_instance();

            let (monitor_x, monitor_y, _w, _h) = match self.monitor_config.get() {
                Some(config) => config.geometry(),
                _ => return,
            };

            if (monitor_x, monitor_y) != (0, 0) {
                log::warn!(
                    "offset monitor geometry not yet supported: {:?}",
                    (monitor_x, monitor_y)
                );
                return;
            }

            let primary = match self.primary() {
                Some(primary) => primary,
                _ => {
                    log::warn!("no primary");
                    return;
                }
            };

            let fmt = primary.format().unwrap_or(spice::SurfaceFormat::Invalid);
            match fmt {
                spice::SurfaceFormat::_32XRGB => {
                    let stride = primary.stride();
                    let buf = primary.data();
                    let start = x * 4 + y * stride;
                    let end = (x + w) * 4 + (y + h - 1) * stride;

                    obj.update_area(
                        x as _,
                        y as _,
                        w as _,
                        h as _,
                        stride as _,
                        &buf[start..end],
                    );
                }
                _ => {
                    log::debug!("format not supported: {:?}", fmt);
                }
            }
        }
    }
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

impl Default for DisplaySpice {
    fn default() -> Self {
        Self::new()
    }
}
