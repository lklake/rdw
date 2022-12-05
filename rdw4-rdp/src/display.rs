use std::{sync::mpsc, thread, time::Duration};

use freerdp::{
    winpr::{wait_for_multiple_objects, WaitResult},
    RdpErr, RdpErrConnect, RdpError, Result,
};
use futures::stream::StreamExt;
use glib::{clone, subclass::prelude::*, translate::*, SignalHandlerId};
use gtk::{glib, prelude::*};

use rdw::{gtk, DisplayExt};

use crate::{
    handlers::{RdpContextHandler, RdpEvent},
    notifier::Notifier,
    util::{format_from_mime, string_from_utf16, utf16_from_utf8},
};

#[repr(C)]
pub struct RdwRdpDisplay {
    parent: rdw::RdwDisplay,
}

#[repr(C)]
pub struct RdwRdpDisplayClass {
    pub parent_class: rdw::RdwDisplayClass,
}

mod imp {
    use crate::util::mime_from_format;

    use super::*;
    use freerdp::{
        channels::{
            cliprdr::Format,
            disp::{MonitorFlags, MonitorLayout, Orientation},
        },
        client::{CliprdrFormat, Context},
        input::{KbdFlags, PtrFlags, PtrXFlags, WHEEL_ROTATION_MASK},
    };
    use futures::channel::{mpsc::UnboundedReceiver, oneshot};
    use glib::subclass::Signal;
    use gtk::subclass::prelude::*;
    use once_cell::sync::Lazy;
    use rdw::gtk::{gdk, gio, glib::MainContext};
    use std::{
        cell::{Cell, RefCell},
        sync::{
            mpsc::{Receiver, Sender},
            Arc, Mutex,
        },
    };

    #[derive(Debug)]
    enum Event {
        Disconnect(oneshot::Sender<Result<()>>),
        Keyboard(KbdFlags, u16),
        Mouse(PtrFlags, u16, u16),
        XMouse(PtrXFlags, u16, u16),
        MonitorLayout(Vec<MonitorLayout>),
        ClipboardRequest(Format),
        ClipboardFormatList(Vec<CliprdrFormat>),
        ClipboardData(Option<Vec<u8>>),
    }

    #[derive(Default)]
    pub(crate) struct Clipboard {
        pub(crate) watch_id: Cell<Option<SignalHandlerId>>,
        pub(crate) tx: RefCell<Option<(Format, futures::channel::mpsc::Sender<glib::Bytes>)>>,
    }

    impl std::fmt::Debug for Clipboard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Clipboard").field("tx", &self.tx).finish()
        }
    }

    unsafe impl ClassStruct for RdwRdpDisplayClass {
        type Type = Display;
    }

    impl std::fmt::Debug for RdwRdpDisplay {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwRdpDisplay")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwRdpDisplay {
        type Type = Display;
    }

    #[derive(Debug)]
    pub struct Display {
        pub(crate) context: Arc<Mutex<Box<Context<RdpContextHandler>>>>,
        state: RefCell<Option<RdpEvent>>,
        tx: RefCell<Option<Sender<Event>>>,
        notifier: Notifier,
        rx: RefCell<Option<UnboundedReceiver<RdpEvent>>>,
        last_mouse: Cell<(f64, f64)>,
        clipboard: Clipboard,
        keymap: Cell<Option<&'static [u16]>>,
        connected: Cell<bool>,
        eodl_tx: RefCell<Option<oneshot::Sender<()>>>,
    }

    impl Default for Display {
        fn default() -> Self {
            let (tx, rx) = futures::channel::mpsc::unbounded();
            let mut context = Context::new(RdpContextHandler::new(tx));
            context.settings.set_support_display_control(true);
            context
                .settings
                .set_os_major_type(freerdp::sys::OSMAJORTYPE_UNIX);
            context
                .settings
                .set_os_minor_type(freerdp::sys::OSMINORTYPE_NATIVE_WAYLAND);

            Self {
                context: Arc::new(Mutex::new(context)),
                state: RefCell::new(None),
                tx: Default::default(),
                notifier: Notifier::new().unwrap(),
                last_mouse: Cell::new((0.0, 0.0)),
                rx: RefCell::new(Some(rx)),
                clipboard: Default::default(),
                keymap: Default::default(),
                connected: Default::default(),
                eodl_tx: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwRdpDisplay";
        type Type = super::Display;
        type ParentType = rdw::Display;
        type Class = RdwRdpDisplayClass;
        type Instance = RdwRdpDisplay;
    }

    impl ObjectImpl for Display {
        fn properties() -> &'static [glib::ParamSpec] {
            use glib::ParamFlags as Flags;

            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![glib::ParamSpecBoolean::new(
                    "rdp-connected",
                    "RDP connected",
                    "Whether the RDP connection is up and running",
                    false,
                    Flags::READABLE,
                )]
            });
            PROPERTIES.as_ref()
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "rdp-connected" => self.connected.get().to_value(),
                _ => unimplemented!(),
            }
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![Signal::builder("rdp-authenticate")
                    .return_type_from(<bool>::static_type())
                    .build()]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().set_mouse_absolute(true);

            self.obj().connect_key_event(clone!(@weak self as this => move |_, keyval, keycode, event| {
                log::debug!("key-event: {:?}", (keyval, keycode, event));
                if keyval == gdk::Key::Pause.into_glib() {
                    unimplemented!()
                }
                if let Some(&xt) = this.keymap.get().and_then(|m| m.get(keycode as usize)) {
                    MainContext::default().spawn_local(glib::clone!(@weak this => async move {
                        let flags = if xt & 0x100 > 0 {
                            KbdFlags::EXTENDED
                        } else {
                            KbdFlags::empty()
                        };
                        if event.contains(rdw::KeyEvent::PRESS) {
                            let _ = this.send_event(Event::Keyboard(flags | KbdFlags::DOWN, xt)).await;
                        }
                        if event.contains(rdw::KeyEvent::RELEASE) {
                            let _ = this.send_event(Event::Keyboard(flags | KbdFlags::RELEASE, xt)).await;
                        }
                    }));
                }
            }));

            self.obj()
                .connect_motion(clone!(@weak self as this => move |_, x, y| {
                    log::debug!("motion: {:?}", (x, y));
                    MainContext::default().spawn_local(glib::clone!(@weak this => async move {
                        this.last_mouse.set((x, y));
                        let _ = this.send_event(Event::Mouse(PtrFlags::MOVE, x as _, y as _)).await;
                    }));
                }));

            self.obj()
                .connect_motion_relative(clone!(@weak self as this => move |_, dx, dy| {
                    log::debug!("motion-relative: {:?}", (dx, dy));
                }));

            self.obj()
                .connect_mouse_press(clone!(@weak self as this => move |_, button| {
                    log::debug!("mouse-press: {:?}", button);
                    MainContext::default().spawn_local(glib::clone!(@weak this => async move {
                        let _ = this.mouse_click(true, button).await;
                    }));
                }));

            self.obj()
                .connect_mouse_release(clone!(@weak self as this => move |_, button| {
                    log::debug!("mouse-release: {:?}", button);
                    MainContext::default().spawn_local(glib::clone!(@weak this => async move {
                        let _ = this.mouse_click(false, button).await;
                    }));
                }));

            self.obj().connect_resize_request(
                clone!(@weak self as this => move |_, width, height, wmm, hmm| {
                    let scale_factor = this.obj().scale_factor() * 100;
                    log::debug!("resize-request: {:?}", (width, height, wmm, hmm, scale_factor));
                    MainContext::default().spawn_local(glib::clone!(@weak this => async move {
                        let _ = this.send_event(Event::MonitorLayout(vec![MonitorLayout::new(
                            MonitorFlags::PRIMARY,
                            0, 0,
                            width, height,
                            wmm, hmm,
                            Orientation::Landscape,
                            scale_factor as _,
                            100,
                        )])).await;
                    }));
                }),
            );
        }
    }

    impl WidgetImpl for Display {
        fn realize(&self) {
            self.parent_realize();

            self.keymap.set(rdw::keymap_xtkbd());

            let ec = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            self.obj().add_controller(&ec);
            ec.connect_scroll(
                clone!(@weak self as this => @default-panic, move |_, dx, dy| {
                    MainContext::default().spawn_local(glib::clone!(@weak this => async move {
                        let _ = this.mouse_scroll(PtrFlags::HWHEEL, dx).await;
                        let _ = this.mouse_scroll(PtrFlags::WHEEL, dy).await;
                    }));
                    glib::signal::Inhibit(false)
                }),
            );

            let cb = gdk::traits::DisplayExt::clipboard(&self.obj().display());
            let watch_id = cb.connect_changed(clone!(@weak self as this => move |clipboard| {
                let is_local = clipboard.is_local();
                if let (false, formats) = (is_local, clipboard.formats()) {
                    let list = formats.mime_types()
                                      .iter()
                                      .map(|m| {
                                          let id = format_from_mime(m);
                                          let name = if id.is_some() {
                                              None
                                          } else {
                                              Some(m.to_string())
                                          };
                                          CliprdrFormat {
                                              id,
                                              name,
                                          }
                                      })
                                      .collect::<Vec<_>>();
                    if !list.is_empty() {
                        log::debug!(">clipboard-grab: {:?}", list);
                        MainContext::default().spawn_local(glib::clone!(@weak this => async move {
                            let _ = this.send_event(Event::ClipboardFormatList(list)).await;
                        }));
                    }
                }
            }));

            self.clipboard.watch_id.set(Some(watch_id));
        }
    }

    impl rdw::DisplayImpl for Display {}

    impl Display {
        fn dispatch_rdp_event(&self, e: RdpEvent) {
            match e {
                RdpEvent::Authenticate { .. } => {
                    self.state.replace(Some(e));
                    glib::idle_add_local(
                        glib::clone!(@weak self as this => @default-return Continue(false), move || {
                            let res = this.obj().emit_by_name::<bool>("rdp-authenticate", &[]);
                            match this.state.take().unwrap() {
                                RdpEvent::Authenticate { settings, tx } => {
                                    let _ = tx.send(if res {
                                        Ok(settings)
                                    } else {
                                        Err(RdpError::Failed("Authenticate cancelled".into()))
                                    });
                                }
                                _ => {
                                    panic!()
                                }
                            }
                            Continue(false)
                        }),
                    );
                }
                RdpEvent::Connected => self.set_connected(true),
                RdpEvent::Disconnected => self.set_connected(false),
                RdpEvent::DesktopResize { w, h } => {
                    self.obj().set_display_size(Some((w as _, h as _)));
                }
                RdpEvent::Update { x, y, w, h } => {
                    let ctxt = self.context.lock().unwrap();
                    let gdi = ctxt.gdi().unwrap();
                    if let Some(buffer) = gdi.primary_buffer() {
                        let stride = gdi.stride();
                        let start = (x * 4 + y * stride) as _;
                        let end = ((x + w) * 4 + (y + h - 1) * stride) as _;

                        self.obj().update_area(
                            x as _,
                            y as _,
                            w as _,
                            h as _,
                            stride as _,
                            &buffer[start..end],
                        );
                    }
                }
                RdpEvent::CursorSet(cursor) => {
                    let inner = cursor.inner;
                    let cursor = rdw::Display::make_cursor(
                        &inner.data,
                        inner.width,
                        inner.height,
                        inner.x,
                        inner.y,
                        1,
                    );
                    self.obj().define_cursor(Some(cursor));
                }
                RdpEvent::CursorSetNull => {
                    let cursor = gdk::Cursor::from_name("none", None);
                    self.obj().define_cursor(cursor);
                }
                RdpEvent::CursorSetDefault => {
                    self.obj().define_cursor(None);
                }
                RdpEvent::ClipboardData { data } => {
                    if let Some((format, mut tx)) = self.clipboard.tx.take() {
                        let data = match format {
                            Format::UnicodeText => match string_from_utf16(data) {
                                Ok(res) => res.into_bytes(),
                                Err(e) => {
                                    log::warn!("Invalid utf16 text: {}", e);
                                    return;
                                }
                            },
                            _ => data,
                        };
                        if let Err(e) = tx.try_send(glib::Bytes::from_owned(data)) {
                            log::warn!("Failed to send clipboard data to future: {}", e);
                        }
                    }
                }
                RdpEvent::ClipboardSetContent { formats } => {
                    let cb = gdk::traits::DisplayExt::clipboard(&self.obj().display());
                    let content = rdw::ContentProvider::new(
                        &formats,
                        clone!(@weak self as this => @default-return None, move |mime, stream, prio| {
                            log::debug!("content-provider-write: {:?}", (mime, stream));
                            let format = match format_from_mime(mime) {
                                Some(format) => format,
                                _ => return None,
                            };
                            Some(Box::pin(clone!(@weak this, @strong stream => @default-return panic!(), async move {
                                use futures::stream::StreamExt;

                                if this.clipboard.tx.borrow().is_some() {
                                    return Err(glib::Error::new(gio::IOErrorEnum::Failed, "clipboard request pending"));
                                }
                                let (tx, mut rx) = futures::channel::mpsc::channel(1);
                                this.clipboard.tx.replace(Some((format, tx)));
                                if this.send_event(Event::ClipboardRequest(format)).await.is_ok() {
                                    if let Some(bytes) = rx.next().await {
                                        return stream.write_bytes_future(&bytes, prio).await.map(|_| ());
                                    }
                                }

                                Err(glib::Error::new(gio::IOErrorEnum::Failed, "failed to request clipboard data"))
                            })))
                        }),
                    );
                    if let Err(e) = cb.set_content(Some(&content)) {
                        log::warn!("Failed to set clipboard content: {}", e);
                    }
                }
                RdpEvent::ClipboardDataRequest { format } => {
                    glib::MainContext::default().spawn_local(glib::clone!(@weak self as this => async move {
                        let mut data = None;

                        if let Some(mime) = mime_from_format(format) {
                            let cb = gdk::traits::DisplayExt::clipboard(&this.obj().display());
                            let res = cb.read_future(&[mime], glib::Priority::default()).await;
                            log::debug!("clipboard-read: {:?}", res);
                            if let Ok((stream, _)) = res {
                                let out = gio::MemoryOutputStream::new_resizable();
                                let res = out.splice_future(
                                    &stream,
                                    gio::OutputStreamSpliceFlags::CLOSE_SOURCE | gio::OutputStreamSpliceFlags::CLOSE_TARGET,
                                    glib::Priority::default()).await;
                                if res.is_ok() {
                                    let bytes = out.steal_as_bytes();
                                    if format.is_text() {
                                        data = utf16_from_utf8(bytes.as_ref()).ok();
                                    } else {
                                        data = Some(bytes.to_vec());
                                    }
                                }
                            }
                        }
                        let _ = this.send_event(Event::ClipboardData(data)).await;
                    }));
                }
            }
        }

        fn set_connected(&self, connected: bool) {
            if self.connected.replace(connected) != connected {
                self.obj().notify("rdp-connected");
            }
        }

        pub(crate) async fn connect(&self) -> Result<()> {
            fn do_connect(context: &mut Arc<Mutex<Box<Context<RdpContextHandler>>>>) -> Result<()> {
                let mut ctxt = context.lock().unwrap();
                loop {
                    let res = ctxt.instance.connect();
                    if let Some(err) = ctxt.last_error() {
                        log::warn!("connect error: {:?}", err);
                        match err {
                            RdpErr::RdpErrConnect(RdpErrConnect::AuthenticationFailed)
                            | RdpErr::RdpErrConnect(RdpErrConnect::LogonFailure) => {
                                // this should trigger RdpEvent::Authenticate on next connect()
                                ctxt.settings.set_username(None).unwrap();
                                ctxt.settings.set_password(None).unwrap();
                                continue;
                            }
                            _ => {}
                        }
                    }
                    break res;
                }
            }

            fn do_loop(
                context: &mut Arc<Mutex<Box<Context<RdpContextHandler>>>>,
                rx: Receiver<Event>,
                notifier: Notifier,
            ) -> Result<()> {
                let res = freerdp_main_loop(context, rx, notifier);

                // on unsollicted disconnect, is this necessary? comment out, as it sends Disconnect events twice..
                // let mut ctxt = context.lock().unwrap();
                // let _ = ctxt.instance.disconnect();
                log::debug!("freerdp thread end: {:?}", res);
                res
            }

            let mut rdp_event_rx = self
                .rx
                .take()
                .ok_or_else(|| RdpError::Failed("already started".into()))?;

            let (conn_tx, conn_rx) = oneshot::channel();

            let (tx, rx) = mpsc::channel();
            self.tx.replace(Some(tx));
            let notifier = self.notifier.clone();
            let mut context = self.context.clone();
            let thread = thread::spawn(move || {
                let res = do_connect(&mut context);
                let connected = res.is_ok();
                conn_tx.send(res).unwrap();
                if connected {
                    let _res = do_loop(&mut context, rx, notifier);
                }
            });

            // the "dispatch loop"
            MainContext::default().spawn_local(clone!(@weak self as this => async move {
                while let Some(e) = rdp_event_rx.next().await {
                    let disconnected = matches!(e, RdpEvent::Disconnected);
                    this.dispatch_rdp_event(e);
                    if disconnected {
                        break;
                    }
                }
                let _ = thread.join().unwrap();
                this.tx.replace(None);
                this.rx.replace(Some(rdp_event_rx));
                if let Some(eodl) = this.eodl_tx.take() {
                    let _ = eodl.send(());
                }
            }));

            conn_rx.await.unwrap()
        }

        pub(crate) async fn disconnect(&self) -> Result<()> {
            // since the dispatch loop is running in the same thread, this is not racy
            // it must be running
            if self.tx.borrow().is_none() {
                return Ok(());
            }
            if self.eodl_tx.borrow().is_some() {
                return Err(RdpError::Failed("Disconnect in progress".into()));
            }

            let (eodl_tx, eodl_rx) = oneshot::channel();
            self.eodl_tx.replace(Some(eodl_tx));

            let (tx, rx) = oneshot::channel();
            MainContext::default().spawn_local(glib::clone!(@weak self as this => async move {
                let _ = this.send_event(Event::Disconnect(tx)).await;
            }));

            let res = rx.await.unwrap_or(Ok(()));
            let _ = eodl_rx.await;
            self.eodl_tx.replace(None);
            res
        }

        async fn send_event(&self, event: Event) -> Result<()> {
            match &*self.tx.borrow() {
                Some(tx) => {
                    tx.send(event)
                        .map_err(|_| RdpError::Failed("send() failed!".into()))?;
                    self.notifier.notify().await
                }
                None => Err(RdpError::Failed("No event channel!".into())),
            }
        }

        async fn mouse_click(&self, press: bool, button: u32) -> Result<()> {
            let (x, y) = self.last_mouse.get();
            let (x, y) = (x as _, y as _);
            let mut event = match button {
                gdk::BUTTON_PRIMARY => Event::Mouse(PtrFlags::BUTTON1, x, y),
                gdk::BUTTON_MIDDLE => Event::Mouse(PtrFlags::BUTTON3, x, y),
                gdk::BUTTON_SECONDARY => Event::Mouse(PtrFlags::BUTTON2, x, y),
                8 | 97 => Event::XMouse(PtrXFlags::BUTTON1, x, y),
                9 | 112 => Event::XMouse(PtrXFlags::BUTTON2, x, y),
                _ => {
                    return Err(RdpError::Failed(format!("Unhandled button {}", button)));
                }
            };
            if press {
                match event {
                    Event::Mouse(ref mut flags, _, _) => {
                        *flags |= PtrFlags::DOWN;
                    }
                    Event::XMouse(ref mut flags, _, _) => {
                        *flags |= PtrXFlags::DOWN;
                    }
                    _ => unreachable!(),
                }
            }
            self.send_event(event).await
        }

        async fn mouse_scroll(&self, flags: PtrFlags, delta: f64) -> Result<()> {
            // FIXME: loop for large values?
            let windows_delta = f64::clamp(delta * -120.0, -256.0, 255.0) as i16;
            self.send_event(Event::Mouse(
                unsafe {
                    PtrFlags::from_bits_unchecked(
                        flags.bits() | (windows_delta as u16 & WHEEL_ROTATION_MASK),
                    )
                },
                0,
                0,
            ))
            .await
        }

        pub fn with_settings(
            &self,
            f: impl FnOnce(&mut freerdp::Settings) -> Result<()>,
        ) -> Result<()> {
            match &mut *self.state.borrow_mut() {
                Some(RdpEvent::Authenticate { settings, .. }) => f(settings),
                _ => f(&mut self.context.lock().unwrap().settings),
            }
        }
    }

    fn freerdp_main_loop(
        context: &mut Arc<Mutex<Box<Context<RdpContextHandler>>>>,
        rx: Receiver<Event>,
        notifier: Notifier,
    ) -> Result<()> {
        let notifier_handle = notifier.handle()?;
        loop {
            let handles = {
                let mut ctxt = context.lock().unwrap();
                if ctxt.instance.shall_disconnect() {
                    break;
                }

                ctxt.event_handles().unwrap()
            };

            let mut handles: Vec<_> = handles.iter().collect();
            handles.push(&notifier_handle);
            wait_for_multiple_objects(&handles, false, None).unwrap();

            if let WaitResult::Object(_) = notifier_handle.wait(Some(&Duration::ZERO))? {
                let e = rx
                    .recv()
                    .map_err(|e| RdpError::Failed(format!("recv(): {}", e)))?;
                dispatch_client_event(context, e)?;
                notifier.read_sync()?;
            }

            let mut ctxt = context.lock().unwrap();
            // FIXME: we use unbounded channels to send RDP events, because we
            // hold the context lock and it is contended with the Display/Gtk
            // thread.. instead, we should have bounded channels but wait for
            // free space.
            if !ctxt.check_event_handles() {
                if let Some(e) = ctxt.last_error() {
                    eprintln!("{:?}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    fn dispatch_client_event(
        context: &mut Arc<Mutex<Box<Context<RdpContextHandler>>>>,
        e: Event,
    ) -> Result<()> {
        let mut ctxt = context.lock().unwrap();
        match e {
            Event::Disconnect(tx) => {
                let res = ctxt.instance.disconnect();
                let _ = tx.send(res);
            }
            Event::Keyboard(flags, code) => {
                if let Some(mut input) = ctxt.input() {
                    input.send_keyboard_event(flags, code)?;
                }
            }
            Event::Mouse(flags, x, y) => {
                if let Some(mut input) = ctxt.input() {
                    input.send_mouse_event(flags, x, y)?;
                }
            }
            Event::XMouse(flags, x, y) => {
                if let Some(mut input) = ctxt.input() {
                    input.send_extended_mouse_event(flags, x, y)?;
                }
            }
            Event::MonitorLayout(layout) => {
                if let Some(disp) = ctxt.disp.as_mut() {
                    disp.send_monitor_layout(&layout)?;
                }
            }
            Event::ClipboardRequest(format) => {
                if let Some(clip) = ctxt.cliprdr.as_mut() {
                    clip.send_client_format_data_request(format)?;
                }
            }
            Event::ClipboardFormatList(list) => {
                if let Some(clip) = ctxt.cliprdr.as_mut() {
                    clip.send_client_format_list(&list)?;
                }
            }
            Event::ClipboardData(data) => {
                if let Some(clip) = ctxt.cliprdr.as_mut() {
                    clip.send_client_format_data_response(data.as_deref())?;
                }
            }
        }
        Ok(())
    }
}

glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends rdw::Display, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Display {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[])
    }

    pub fn with_settings(
        &self,
        f: impl FnOnce(&mut freerdp::Settings) -> Result<()>,
    ) -> Result<()> {
        self.imp().with_settings(f)
    }

    pub async fn rdp_connect(&self) -> Result<()> {
        self.imp().connect().await
    }

    pub async fn rdp_disconnect(&self) -> Result<()> {
        self.imp().disconnect().await
    }

    pub fn last_error(&self) -> Option<RdpErr> {
        let ctxt = self.imp().context.lock().unwrap();
        ctxt.last_error()
    }

    pub fn connect_rdp_authenticate<F: Fn(&Self) -> bool + 'static>(
        &self,
        f: F,
    ) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P) -> bool + 'static>(
            this: *mut RdwRdpDisplay,
            f: glib::ffi::gpointer,
        ) -> bool
        where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(Display::from_glib_borrow(this).unsafe_cast_ref::<P>())
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"rdp-authenticate\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}
