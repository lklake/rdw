use std::{
    convert::TryFrom,
    os::unix::prelude::RawFd,
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};

use freerdp::{
    locale::keyboard_init_ex,
    update,
    winpr::{wait_for_multiple_objects, FdMode, Handle, WaitResult},
    RdpError, Result, PIXEL_FORMAT_BGRA32,
};
use futures::{executor::block_on, stream::StreamExt, SinkExt};
use glib::{clone, subclass::prelude::*, translate::*, SignalHandlerId};
use gtk::{gio, glib, prelude::*};
use rdw::gtk::{self, gio::NONE_CANCELLABLE};

// use keycodemap::KEYMAP_XORGEVDEV2QNUM;
use rdw::DisplayExt;

#[repr(C)]
pub struct RdwRdpDisplay {
    parent: rdw::RdwDisplay,
}

#[repr(C)]
pub struct RdwRdpDisplayClass {
    pub parent_class: rdw::RdwDisplayClass,
}

mod imp {
    use super::*;
    use freerdp::{
        channels::disp::{MonitorFlags, MonitorLayout, Orientation},
        input::{KbdFlags, PtrFlags, PtrXFlags, WHEEL_ROTATION_MASK},
    };
    use glib::subclass::Signal;
    use gtk::subclass::prelude::*;
    use keycodemap::KEYMAP_XORGEVDEV2XTKBD;
    use once_cell::sync::{Lazy, OnceCell};
    use rdw::gtk::{gdk, glib::MainContext};
    use std::{
        cell::{Cell, RefCell},
        sync::{mpsc::Sender, Arc, Mutex},
        thread::JoinHandle,
    };

    #[derive(Debug)]
    enum Event {
        Keyboard(KbdFlags, u16),
        Mouse(PtrFlags, u16, u16),
        XMouse(PtrXFlags, u16, u16),
        MonitorLayout(Vec<MonitorLayout>),
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
        pub(crate) context: Arc<Mutex<freerdp::client::Context<RdpContextHandler>>>,
        state: RefCell<Option<RdpEvent>>,
        thread: OnceCell<JoinHandle<Result<()>>>,
        tx: OnceCell<Sender<Event>>,
        notifier: Notifier,
        rx: RefCell<Option<futures::channel::mpsc::Receiver<RdpEvent>>>,
        last_mouse: Cell<(f64, f64)>,
    }

    impl Default for Display {
        fn default() -> Self {
            let (tx, rx) = futures::channel::mpsc::channel(1);
            let mut context = freerdp::client::Context::new(RdpContextHandler { tx });
            context.settings.set_support_display_control(true);
            Self {
                context: Arc::new(Mutex::new(context)),
                state: RefCell::new(None),
                thread: OnceCell::new(),
                tx: OnceCell::new(),
                notifier: Notifier::new().unwrap(),
                last_mouse: Cell::new((0.0, 0.0)),
                rx: RefCell::new(Some(rx)),
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
        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![Signal::builder("rdp-authenticate", &[], <bool>::static_type().into()).build()]
            });
            SIGNALS.as_ref()
        }

        fn properties() -> &'static [glib::ParamSpec] {
            //use glib::ParamFlags as Flags;

            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| vec![]);
            PROPERTIES.as_ref()
        }

        fn set_property(
            &self,
            _obj: &Self::Type,
            _id: usize,
            _value: &glib::Value,
            pspec: &glib::ParamSpec,
        ) {
            match pspec.name() {
                _ => unimplemented!(),
            }
        }

        fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                _ => unimplemented!(),
            }
        }

        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            obj.set_mouse_absolute(true);

            obj.connect_key_event(clone!(@weak obj => move |_, keyval, keycode, event| {
                log::debug!("key-event: {:?}", (keyval, keycode, event));
                if keyval == *gdk::keys::constants::Pause {
                    unimplemented!()
                }
                if let Some(&xt) = KEYMAP_XORGEVDEV2XTKBD.get(keycode as usize) {
                    log::debug!("xt: {:?}", xt);
                    MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                        let self_ = Self::from_instance(&obj);
                        let flags = if xt & 0x100 > 0 {
                            KbdFlags::EXTENDED
                        } else {
                            KbdFlags::empty()
                        };
                        if event.contains(rdw::KeyEvent::PRESS) {
                            let _ = self_.send_event(Event::Keyboard(flags | KbdFlags::DOWN, xt)).await;
                        }
                        if event.contains(rdw::KeyEvent::RELEASE) {
                            let _ = self_.send_event(Event::Keyboard(flags | KbdFlags::RELEASE, xt)).await;
                        }
                    }));
                }
            }));

            obj.connect_motion(clone!(@weak obj => move |_, x, y| {
                log::debug!("motion: {:?}", (x, y));
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let self_ = Self::from_instance(&obj);
                    self_.last_mouse.set((x, y));
                    let _ = self_.send_event(Event::Mouse(PtrFlags::MOVE, x as _, y as _)).await;
                }));
            }));

            obj.connect_motion_relative(clone!(@weak obj => move |_, dx, dy| {
                log::debug!("motion-relative: {:?}", (dx, dy));
            }));

            obj.connect_mouse_press(clone!(@weak obj => move |_, button| {
                log::debug!("mouse-press: {:?}", button);
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let self_ = Self::from_instance(&obj);
                    let _ = self_.mouse_click(true, button).await;
                }));
            }));

            obj.connect_mouse_release(clone!(@weak obj => move |_, button| {
                log::debug!("mouse-release: {:?}", button);
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let self_ = Self::from_instance(&obj);
                    let _ = self_.mouse_click(false, button).await;
                }));
            }));

            obj.connect_resize_request(clone!(@weak obj => move |_, width, height, wmm, hmm| {
                log::debug!("resize-request: {:?}", (width, height, wmm, hmm));
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let self_ = Self::from_instance(&obj);
                    let _ = self_.send_event(Event::MonitorLayout(vec![MonitorLayout::new(
                        MonitorFlags::PRIMARY,
                        0, 0,
                        width, height,
                        wmm, hmm,
                        Orientation::Landscape,
                        0, 0
                    )])).await;
                }));
            }));
        }
    }

    impl WidgetImpl for Display {
        fn realize(&self, widget: &Self::Type) {
            self.parent_realize(widget);

            let ec = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            widget.add_controller(&ec);
            ec.connect_scroll(clone!(@weak widget => @default-panic, move |_, dx, dy| {
                MainContext::default().spawn_local(glib::clone!(@weak widget => async move {
                    let self_ = Self::from_instance(&widget);
                    let _ = self_.mouse_scroll(PtrFlags::HWHEEL, dx).await;
                    let _ = self_.mouse_scroll(PtrFlags::WHEEL, dy).await;
                }));
                glib::signal::Inhibit(false)
            }));
        }
    }

    impl rdw::DisplayImpl for Display {}

    impl Display {
        pub(crate) fn connect(&self, obj: &super::Display) -> Result<()> {
            let mut rx = self
                .rx
                .take()
                .ok_or_else(|| RdpError::Failed("already started".into()))?;
            MainContext::default().spawn_local(clone!(@weak obj => async move {
                let imp = imp::Display::from_instance(&obj);

                while let Some(e) = rx.next().await {
                    match e {
                        RdpEvent::Authenticate { .. } => {
                            imp.state.replace(Some(e));
                            glib::idle_add_local(glib::clone!(@weak obj => @default-return Continue(false), move || {
                                let res = obj.emit_by_name("rdp-authenticate", &[]).unwrap().unwrap().get::<bool>().unwrap();
                                let imp = imp::Display::from_instance(&obj);
                                match imp.state.take().unwrap() {
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
                            }));
                        }
                        RdpEvent::DesktopResize { w, h } => {
                            obj.set_display_size(Some((w as _, h as _)));
                        }
                        RdpEvent::Update { x, y, w, h } => {
                            let ctxt = imp.context.lock().unwrap();
                            let gdi = ctxt.gdi().unwrap();
                            if let Some(buffer) = gdi.primary_buffer() {
                                let stride = gdi.stride();
                                let start = (x * 4 + y * stride) as _;
                                let end = ((x + w) * 4 + (y + h - 1) * stride) as _;

                                obj.update_area(x as _, y as _, w as _, h as _, stride as _, &buffer[start..end]);
                            }
                        },
                    }
                }
            }));

            let notifier = self.notifier.clone();
            let context = self.context.clone();
            let (tx, rx) = mpsc::channel();
            self.tx.set(tx).unwrap();
            let thread = thread::spawn(move || {
                let mut ctxt = context.lock().unwrap();
                ctxt.instance.connect()?;
                drop(ctxt);

                let notifier_handle = notifier.handle();
                loop {
                    let mut ctxt = context.lock().unwrap();
                    if ctxt.instance.shall_disconnect() {
                        break;
                    }

                    let handles = ctxt.event_handles().unwrap();
                    let mut handles: Vec<_> = handles.iter().collect();
                    handles.push(&notifier_handle);
                    drop(ctxt);
                    wait_for_multiple_objects(&handles, false, None).unwrap();

                    let mut ctxt = context.lock().unwrap();
                    if let WaitResult::Object(_) = notifier_handle.wait(Some(&Duration::ZERO))? {
                        match rx
                            .recv()
                            .map_err(|e| RdpError::Failed(format!("recv(): {}", e)))?
                        {
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
                                if let Some(disp) = ctxt.disp_mut() {
                                    disp.send_monitor_layout(&layout)?;
                                }
                            }
                        }
                        notifier.read_sync()?;
                    }

                    if !ctxt.check_event_handles() {
                        if let Err(e) = ctxt.last_error() {
                            eprintln!("{}", e);
                            break;
                        }
                    }
                }
                Ok(())
            });

            self.thread.set(thread).unwrap();
            Ok(())
        }

        async fn send_event(&self, event: Event) -> Result<()> {
            if let Some(tx) = self.tx.get() {
                tx.send(event)
                    .map_err(|_| RdpError::Failed("send() failed!".into()))?;
                self.notifier.notify().await
            } else {
                Err(RdpError::Failed("No event channel!".into()))
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
}

glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends rdw::Display, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Display {
    pub fn new() -> Self {
        glib::Object::new::<Self>(&[]).unwrap()
    }

    pub fn with_settings(
        &self,
        f: impl FnOnce(&mut freerdp::Settings) -> Result<()>,
    ) -> Result<()> {
        let self_ = imp::Display::from_instance(self);

        self_.with_settings(f)
    }

    pub fn rdp_connect(&mut self) -> Result<()> {
        let self_ = imp::Display::from_instance(self);

        self_.connect(self)
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
            f(&*Display::from_glib_borrow(this).unsafe_cast_ref::<P>())
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

#[derive(Debug)]
struct RdpPointerHandler {
    _test: bool,
}

impl freerdp::graphics::PointerHandler for RdpPointerHandler {
    type ContextHandler = RdpContextHandler;

    fn new(
        &mut self,
        context: &mut freerdp::client::Context<Self::ContextHandler>,
        pointer: &freerdp::graphics::Pointer,
    ) -> Result<()> {
        dbg!(pointer);
        let _h = context.handler_mut();
        Ok(())
    }
}

#[derive(Debug)]
struct RdpUpdateHandler;

impl freerdp::update::UpdateHandler for RdpUpdateHandler {
    type ContextHandler = RdpContextHandler;

    fn begin_paint(context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let mut primary = gdi.primary().ok_or(RdpError::Unsupported)?;
        primary.hdc().hwnd().invalid().set_null(true);
        Ok(())
    }

    fn end_paint(context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let mut primary = gdi.primary().ok_or(RdpError::Unsupported)?;
        let invalid = primary.hdc().hwnd().invalid();
        if invalid.null() {
            return Ok(());
        }
        let (x, y, w, h) = (invalid.x(), invalid.y(), invalid.w(), invalid.h());

        let handler = context.handler_mut().unwrap();
        handler.update_buffer(x, y, w, h)
    }

    fn set_bounds(
        _context: &mut freerdp::client::Context<Self::ContextHandler>,
        bounds: &update::Bounds,
    ) -> Result<()> {
        dbg!(bounds);
        Ok(())
    }

    fn synchronize(_context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
        dbg!();
        Ok(())
    }

    fn desktop_resize(context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
        let mut gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let (w, h) = (
            context.settings.desktop_width(),
            context.settings.desktop_height(),
        );
        dbg!((w, h));
        gdi.resize(w, h)?;
        let handler = context.handler_mut().unwrap();
        handler.desktop_resize(w, h)
    }
}

#[derive(Debug)]
enum RdpEvent {
    Authenticate {
        settings: freerdp::Settings,
        tx: mpsc::Sender<Result<freerdp::Settings>>,
    },
    DesktopResize {
        w: u32,
        h: u32,
    },
    Update {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
}

#[derive(Debug)]
pub(crate) struct RdpContextHandler {
    tx: futures::channel::mpsc::Sender<RdpEvent>,
}

impl RdpContextHandler {
    fn send(&mut self, event: RdpEvent) -> Result<()> {
        block_on(async { self.tx.send(event).await })
            .map_err(|e| RdpError::Failed(format!("{}", e)))?;
        Ok(())
    }

    fn update_buffer(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let x = u32::try_from(x)?;
        let y = u32::try_from(y)?;
        let w = u32::try_from(w)?;
        let h = u32::try_from(h)?;
        self.send(RdpEvent::Update { x, y, w, h })
    }

    fn desktop_resize(&mut self, w: u32, h: u32) -> Result<()> {
        self.send(RdpEvent::DesktopResize { w, h })
    }
}

impl freerdp::client::Handler for RdpContextHandler {
    fn authenticate(&mut self, context: &mut freerdp::client::Context<Self>) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.send(RdpEvent::Authenticate {
            tx,
            settings: context.settings.clone(),
        })?;
        let settings = rx.recv().unwrap()?;
        context.settings.clone_from(&settings);
        Ok(())
    }

    fn post_connect(&mut self, context: &mut freerdp::client::Context<Self>) -> Result<()> {
        context.instance.gdi_init(PIXEL_FORMAT_BGRA32)?;

        let gdi = context.gdi().unwrap();
        let mut graphics = context.graphics().unwrap();
        let mut update = context.update().unwrap();

        let (w, h) = match (gdi.width(), gdi.height()) {
            (Some(w), Some(h)) => (w, h),
            _ => return Err(RdpError::Failed("No GDI dimensions".into())),
        };

        graphics.register_pointer::<RdpPointerHandler>();
        update.register::<RdpUpdateHandler>();

        let _ = keyboard_init_ex(
            context.settings.keyboard_layout(),
            context.settings.keyboard_remapping_list().as_deref(),
        );

        let handler = context.handler_mut().unwrap();
        handler.desktop_resize(w, h)
    }
}

#[derive(Debug)]
struct NotifierInner {
    fd: RawFd,
}

impl Drop for NotifierInner {
    fn drop(&mut self) {
        let _ = nix::unistd::close(self.fd);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Notifier {
    inner: Arc<NotifierInner>,
}

impl Notifier {
    fn new() -> Result<Self> {
        // TODO: non-Linux
        use nix::sys::eventfd::*;
        let fd = eventfd(
            0,
            EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_SEMAPHORE,
        )
        .map_err(|e| RdpError::Failed(format!("eventfd() failed: {}", e)))?;

        Ok(Self {
            inner: Arc::new(NotifierInner { fd }),
        })
    }

    fn handle(&self) -> Handle {
        Handle::new_fd_event(&[], false, false, self.inner.fd, FdMode::READ)
    }

    async fn notify(&self) -> Result<()> {
        let st = unsafe { gio::UnixOutputStream::with_fd(self.inner.fd) };
        let buffer = 1u64.to_ne_bytes();
        st.write_all_async_future(buffer, glib::Priority::default())
            .await
            .map_err(|_| RdpError::Failed("notify() failed".into()))?;
        Ok(())
    }

    fn read_sync(&self) -> Result<()> {
        let st = unsafe { gio::UnixInputStream::with_fd(self.inner.fd) };
        let buffer = 1u64.to_ne_bytes();
        st.read_all(buffer, NONE_CANCELLABLE)
            .map_err(|e| RdpError::Failed(format!("read() failed: {}", e)))?;
        Ok(())
    }
}
