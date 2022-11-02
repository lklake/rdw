use std::convert::TryFrom;

use glib::{clone, signal::SignalHandlerId, subclass::prelude::*};
use gtk::{gdk, gio, glib, prelude::*};
use keycodemap::KEYMAP_XORGEVDEV2XTKBD;
use rdw::{gtk, DisplayExt};
use spice::prelude::*;
use spice_client_glib as spice;
#[cfg(unix)]
use std::os::unix::io::IntoRawFd;

mod imp {
    use super::*;
    use crate::util;
    use gtk::subclass::prelude::*;
    use once_cell::sync::Lazy;
    use std::cell::{Cell, RefCell};

    #[repr(C)]
    pub struct RdwSpiceDisplayClass {
        pub parent_class: rdw::RdwDisplayClass,
    }

    unsafe impl ClassStruct for RdwSpiceDisplayClass {
        type Type = Display;
    }

    #[repr(C)]
    pub struct RdwSpiceDisplay {
        parent: rdw::RdwDisplay,
    }

    impl std::fmt::Debug for RdwSpiceDisplay {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwSpiceDisplay")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwSpiceDisplay {
        type Type = Display;
    }

    #[derive(Default)]
    pub(crate) struct Clipboard {
        pub(crate) watch_id: Cell<Option<SignalHandlerId>>,
        pub(crate) tx: RefCell<
            Option<(
                spice::ClipboardFormat,
                futures::channel::mpsc::Sender<glib::Bytes>,
            )>,
        >,
    }

    #[derive(Default)]
    pub struct Display {
        pub(crate) session: spice::Session,
        pub(crate) monitor_config: Cell<Option<spice::DisplayMonitorConfig>>,
        pub(crate) main: glib::WeakRef<spice::MainChannel>,
        pub(crate) input: glib::WeakRef<spice::InputsChannel>,
        pub(crate) display: glib::WeakRef<spice::DisplayChannel>,
        pub(crate) last_button_state: Cell<Option<i32>>,
        pub(crate) nth_monitor: usize,
        pub(crate) clipboard: [Clipboard; 2],
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwSpiceDisplay";
        type Type = super::Display;
        type ParentType = rdw::Display;
        type Class = RdwSpiceDisplayClass;
        type Instance = RdwSpiceDisplay;
    }

    impl ObjectImpl for Display {
        fn properties() -> &'static [glib::ParamSpec] {
            use glib::ParamFlags as Flags;

            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![glib::ParamSpecObject::new(
                    "session",
                    "Session",
                    "Spice client session",
                    spice::Session::static_type(),
                    Flags::READABLE,
                )]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, _value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "session" => self.session.to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().set_mouse_absolute(true);

            self.obj().connect_key_event(
                clone!(@weak self as this => move |_, keyval, keycode, event| {
                    log::debug!("key-event: {:?}", (event, keyval, keycode));
                    // TODO: get the correct keymap according to gdk display type
                    if let Some(&xt) = KEYMAP_XORGEVDEV2XTKBD.get(keycode as usize) {
                        if let Some(input) = this.input.upgrade() {
                            if event.contains(rdw::KeyEvent::PRESS|rdw::KeyEvent::RELEASE) {
                                input.key_press_and_release(xt as _)
                            } else if event.contains(rdw::KeyEvent::PRESS) {
                                input.key_press(xt as _);
                            } else if event.contains(rdw::KeyEvent::RELEASE) {
                                input.key_release(xt as _);
                            }
                        }
                    }
                }),
            );

            self.obj().connect_motion(clone!(@weak self as this => move |_, x, y| {
                log::debug!("motion: {:?}", (x, y));
                if let Some(input) = this.input.upgrade() {
                    input.position(x as _, y as _, this.nth_monitor as _, this.last_button_state());
                }
            }));

            self.obj()
                .connect_motion_relative(clone!(@weak self as this => move |_, dx, dy| {
                    log::debug!("motion-relative: {:?}", (dx, dy));
                    if let Some(input) = this.input.upgrade() {
                        input.motion(dx as _, dy as _, this.last_button_state());
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

            self.obj().connect_resize_request(clone!(@weak self as this => move |_, width, height, wmm, hmm| {
                log::debug!("resize-request: {:?}", (width, height));
                if let Some(main) = this.main.upgrade() {
                    main.update_display_enabled(this.nth_monitor as _, true, false);
                    main.update_display_mm(this.nth_monitor as _, wmm as _, hmm as _, false);
                    main.update_display(this.nth_monitor as _, 0, 0, width as _, height as _, true);
                }
            }));

            let session = &self.session;

            session.connect_channel_new(clone!(@weak self as this => move |_session, channel| {
                use spice::ChannelType::*;

                let type_ = match spice::ChannelType::try_from(channel.channel_type()) {
                    Ok(t) => t,
                    _ => return,
                };

                match type_ {
                    Main => {
                        let main = channel.clone().downcast::<spice::MainChannel>().unwrap();
                        this.main.set(Some(&main));

                        main.connect_channel_event(clone!(@weak this => move |_, event| {
                            use spice::ChannelEvent::*;

                            if event == Closed {
                                this.session.disconnect();
                            }
                        }));

                        main.connect_main_mouse_update(clone!(@weak this => move |main| {
                            let mode = spice::MouseMode::from_bits_truncate(main.mouse_mode());
                            log::debug!("mouse-update: {:?}", mode);
                            this.obj().set_mouse_absolute(mode.contains(spice::MouseMode::CLIENT));
                        }));

                        main.connect_main_clipboard_selection(clone!(@weak this => move |_main, selection, type_, data| {
                            log::debug!("clipboard-data: {:?}", (selection, type_, data.len()));
                            if let Some((req_type, mut tx)) = this.clipboard[selection as usize].tx.take() {
                                if type_ != req_type as u32 {
                                    log::warn!("Didn't get expected type from guest clipboard!");
                                    return;
                                }
                                if let Err(e) = tx.try_send(glib::Bytes::from(data)) {
                                    log::warn!("Failed to send clipboard data to future: {}", e);
                                }
                            }
                        }));

                        main.connect_main_clipboard_selection_grab(clone!(@weak this => move |_main, selection, types| {
                            let types: Vec<_> = types.iter()
                                                     .filter_map(|&t| spice::ClipboardFormat::try_from(t as i32).ok())
                                                     .filter_map(util::mime_from_format)
                                                     .collect();
                            log::debug!("clipboard-grab: {:?}", (selection, &types));
                            if let Some(clipboard) = this.clipboard_from_selection(selection) {
                                let content = rdw::ContentProvider::new(&types, clone!(@weak this => @default-return None, move |mime, stream, prio| {
                                    log::debug!("content-provider-write: {:?}", (mime, stream));
                                    let format = match util::format_from_mime(mime) {
                                        Some(f) => f,
                                        None => return None,
                                    };

                                    Some(Box::pin(clone!(@weak this, @strong stream => @default-return panic!(), async move {
                                        use futures::stream::StreamExt;

                                        if this.clipboard[selection as usize].tx.borrow().is_some() {
                                            return Err(glib::Error::new(gio::IOErrorEnum::Failed, "clipboard request pending"));
                                        }

                                        if let Some(main) = this.main.upgrade() {
                                            let (tx, mut rx) = futures::channel::mpsc::channel(1);
                                            this.clipboard[selection as usize].tx.replace(Some((format, tx)));
                                            main.clipboard_selection_request(selection, format as u32);
                                            if let Some(bytes) = rx.next().await {
                                                return stream.write_bytes_future(&bytes, prio).await.map(|_| ());
                                            }
                                        }

                                        Err(glib::Error::new(gio::IOErrorEnum::Failed, "failed to request clipboard data"))
                                    })))
                                }));
                                if let Err(e) = clipboard.set_content(Some(&content)) {
                                    log::warn!("Failed to set clipboard grab: {}", e);
                                }
                            }
                        }));

                        main.connect_main_clipboard_selection_release(clone!(@weak this => move |_main, selection| {
                            log::debug!("clipboard-release: {:?}", selection);
                            if let Some(clipboard) = this.clipboard_from_selection(selection) {
                                if let Err(e) = clipboard.set_content(gdk::ContentProvider::NONE) {
                                    log::warn!("Failed to release clipboard: {}", e);
                                }
                            }
                        }));

                        main.connect_main_clipboard_selection_request(clone!(@weak this => @default-return false, move |main, selection, type_| {
                            let mime = spice::ClipboardFormat::try_from(type_ as i32).map_or(None, util::mime_from_format);
                            log::debug!("clipboard-request: {:?}", (selection, mime));

                            if let (Some(mime), Some(clipboard)) = (mime, this.clipboard_from_selection(selection)) {
                                glib::MainContext::default().spawn_local(glib::clone!(@weak this, @weak clipboard, @strong main => async move {
                                    let res = clipboard.read_future(&[mime], glib::Priority::default()).await;
                                    log::debug!("clipboard-read: {:?}", res);

                                    if let Ok((stream, mime)) = res {
                                        if let Some(format) = util::format_from_mime(&mime) {
                                            let out = gio::MemoryOutputStream::new_resizable();
                                            let res = out.splice_future(
                                                &stream,
                                                gio::OutputStreamSpliceFlags::CLOSE_SOURCE | gio::OutputStreamSpliceFlags::CLOSE_TARGET,
                                                glib::Priority::default()).await;
                                            match res {
                                                Ok(size) => {
                                                    let data = out.steal_as_bytes();
                                                    main.clipboard_selection_notify(selection, format as u32, data.as_ref());
                                                    log::debug!("clipboard-sent: {}", size);
                                                    return;
                                                }
                                                Err(e) => {
                                                    log::warn!("Failed to read clipboard: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    main.clipboard_selection_notify(selection, 0, &[]);
                                }));
                            }
                            true
                        }));
                    },
                    Inputs => {
                        let input = channel.clone().downcast::<spice::InputsChannel>().unwrap();
                        this.input.set(Some(&input));

                        input.connect_inputs_modifiers(clone!(@weak this => move |input| {
                            let modifiers = input.key_modifiers();
                            log::debug!("inputs-modifiers: {}", modifiers);
                            input.connect_channel_event(clone!(@weak this => move |input, event| {
                                if event == spice::ChannelEvent::Opened && input.socket().unwrap().family() == gio::SocketFamily::Unix {
                                    log::debug!("on unix socket");
                                }
                            }));
                        }));
                        ChannelExt::connect(&input);
                    }
                    Display => {
                        let dpy = channel.clone().downcast::<spice::DisplayChannel>().unwrap();
                        this.display.set(Some(&dpy));

                        dpy.connect_display_primary_create(clone!(@weak this => move |_| {
                            log::debug!("primary-create");
                        }));

                        dpy.connect_display_primary_destroy(|_| {
                            log::debug!("primary-destroy");
                        });

                        dpy.connect_display_mark(clone!(@weak this => move |_, mark| {
                            log::debug!("primary-mark: {}", mark);
                            this.invalidate_monitor();
                        }));

                        dpy.connect_display_invalidate(clone!(@weak this => move |_, x, y, w, h| {
                            log::debug!("primary-invalidate: {:?}", (x, y, w, h));
                            this.invalidate(x as _, y as _, w as _, h as _);
                        }));

                        dpy.connect_gl_scanout_notify(clone!(@weak this => move |dpy| {
                            let scanout = dpy.gl_scanout();
                            log::debug!("notify::gl-scanout: {:?}", scanout);

                            #[cfg(unix)]
                            if let Some(scanout) = scanout {
                                this.obj().set_dmabuf_scanout(rdw::RdwDmabufScanout {
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

                        dpy.connect_gl_draw(clone!(@weak this => move |dpy, x, y, w, h| {
                            log::debug!("gl-draw: {:?}", (x, y, w, h));
                            this.obj().render();
                            dpy.gl_draw_done();
                        }));

                        dpy.connect_monitors_notify(clone!(@weak this => move |dpy| {
                            let monitors = dpy.monitors();
                            log::debug!("notify::monitors: {:?}", monitors);

                            let monitor_config = monitors.and_then(|m| m.get(this.nth_monitor).copied());
                            if let Some((0, 0, w, h)) = monitor_config.map(|c| c.geometry()) {
                                this.obj().set_display_size(Some((w, h)));
                            }
                            this.monitor_config.set(monitor_config);
                        }));

                        ChannelExt::connect(&dpy);
                    },
                    Cursor => {
                        let cursor = channel.clone().downcast::<spice::CursorChannel>().unwrap();

                        cursor.connect_cursor_move(clone!(@weak this => move |_cursor, x, y| {
                            log::debug!("cursor-move: {:?}", (x, y));
                            this.obj().set_cursor_position(Some((x as _, y as _)));
                        }));

                        cursor.connect_cursor_reset(clone!(@weak this => move |_cursor| {
                            log::debug!("cursor-reset");
                            this.obj().define_cursor(None);
                        }));

                        cursor.connect_cursor_hide(clone!(@weak this => move |_cursor| {
                            log::debug!("cursor-hide");
                            let cursor = gdk::Cursor::from_name("none", None);
                            this.obj().define_cursor(cursor);
                        }));

                        cursor.connect_cursor_notify(clone!(@weak this => move |cursor| {
                            let cursor = cursor.cursor();
                            log::debug!("cursor-notify: {:?}", cursor);
                            if let Some(cursor) = cursor {
                                match cursor.cursor_type() {
                                    Ok(spice::CursorType::Alpha) => {
                                        let cursor = rdw::Display::make_cursor(
                                            cursor.data().unwrap(),
                                            cursor.width(),
                                            cursor.height(),
                                            0,
                                            0,
                                            this.obj().scale_factor()
                                        );
                                        this.obj().define_cursor(Some(cursor));
                                    }
                                    e => log::warn!("Unhandled cursor type: {:?}", e),
                                }
                            }
                        }));

                        ChannelExt::connect(&cursor);
                    }
                    _ => {}
                }
            }));
        }

        fn dispose(&self) {
            if let Some(id) = self.clipboard[0].watch_id.take() {
                let clipboard = self.clipboard_from_selection(0).unwrap();
                clipboard.disconnect(id);
            }
            if let Some(id) = self.clipboard[1].watch_id.take() {
                let clipboard = self.clipboard_from_selection(1).unwrap();
                clipboard.disconnect(id);
            }
        }
    }

    impl WidgetImpl for Display {
        fn realize(&self) {
            self.parent_realize();

            self.add_clipboard_watch(0);
            self.add_clipboard_watch(1);
        }
    }

    impl rdw::DisplayImpl for Display {}

    impl Display {
        fn add_clipboard_watch(&self, selection: u32) {
            let clipboard = self.clipboard_from_selection(selection).unwrap();
            let watch_id = clipboard.connect_changed(clone!(@weak self as this => move |clipboard| {
                let is_local = clipboard.is_local();
                if let (false, Some(main), formats) = (is_local, this.main.upgrade(), clipboard.formats()) {
                    let mut types = formats.mime_types()
                                           .iter()
                                           .filter_map(|m| util::format_from_mime(m))
                                           .map(|f| f as u32)
                                           .collect::<Vec<_>>();
                    types.sort_unstable();
                    types.dedup();
                    if !types.is_empty() {
                        log::debug!(">clipboard-grab({}): {:?}", selection, types);
                        main.clipboard_selection_grab(selection, &types);
                    }
                }
            }));

            self.clipboard[selection as usize]
                .watch_id
                .set(Some(watch_id));
        }

        fn clipboard_from_selection(&self, selection: u32) -> Option<gdk::Clipboard> {
            let obj = self.obj();

            match selection {
                0 => Some(gdk::traits::DisplayExt::clipboard(
                    &obj.upcast_ref::<gtk::Widget>().display(),
                )),
                1 => Some(gdk::traits::DisplayExt::primary_clipboard(
                    &obj.upcast_ref::<gtk::Widget>().display(),
                )),
                _ => {
                    log::warn!("Unsupport clipboard selection: {}", selection);
                    None
                }
            }
        }

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

        fn invalidate_monitor(&self) {
            if let Some(c) = self.monitor_config.get() {
                let (x, y, w, h) = c.geometry();
                self.invalidate(x, y, w, h);
            }
        }

        fn invalidate(&self, x: usize, y: usize, w: usize, h: usize) {
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

                    self.obj().update_area(
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
    pub struct Display(ObjectSubclass<imp::Display>) @extends rdw::Display, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Display {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[])
    }

    pub fn session(&self) -> &spice::Session {
        let imp = imp::Display::from_obj(self);

        &imp.session
    }
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}
