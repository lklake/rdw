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
use glib::{clone, subclass::prelude::*, translate::*};
use gtk::{gio, glib, prelude::*};
use rdw::gtk::{self, gio::NONE_CANCELLABLE};

// use keycodemap::KEYMAP_XORGEVDEV2QNUM;
use rdw::DisplayExt;

mod imp {
    use super::*;
    use freerdp::input::KbdFlags;
    use gtk::subclass::prelude::*;
    use once_cell::sync::{Lazy, OnceCell};
    use std::{
        sync::{mpsc::Sender, Arc, Mutex},
        thread::JoinHandle,
    };

    #[derive(Debug)]
    enum Event {
        Keyboard(KbdFlags, u16),
    }

    #[repr(C)]
    pub struct RdwRdpDisplayClass {
        pub parent_class: rdw::RdwDisplayClass,
    }

    unsafe impl ClassStruct for RdwRdpDisplayClass {
        type Type = Display;
    }

    #[repr(C)]
    pub struct RdwRdpDisplay {
        parent: rdw::RdwDisplay,
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
        thread: OnceCell<JoinHandle<Result<()>>>,
        tx: OnceCell<Sender<Event>>,
        notifier: Notifier,
    }

    impl Default for Display {
        fn default() -> Self {
            Self {
                context: Arc::new(Mutex::new(freerdp::client::Context::new(
                    RdpContextHandler { test: 42 },
                ))),
                thread: OnceCell::new(),
                tx: OnceCell::new(),
                notifier: Notifier::new().unwrap(),
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
                let self_ = Self::from_instance(&obj);
                log::debug!("key-event: {:?}", (keyval, keycode));
                glib::MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let self_ = Self::from_instance(&obj);
                    let flags = KbdFlags::empty();
                    let code = 0;
                    let _ = self_.send_event(Event::Keyboard(flags, code)).await;
                }));
            }));

            obj.connect_motion(clone!(@weak obj => move |_, x, y| {
                let self_ = Self::from_instance(&obj);
                log::debug!("motion: {:?}", (x, y));
            }));

            obj.connect_motion_relative(clone!(@weak obj => move |_, dx, dy| {
                let self_ = Self::from_instance(&obj);
                log::debug!("motion-relative: {:?}", (dx, dy));
            }));

            obj.connect_mouse_press(clone!(@weak obj => move |_, button| {
                let self_ = Self::from_instance(&obj);
                log::debug!("mouse-press: {:?}", button);
            }));

            obj.connect_mouse_release(clone!(@weak obj => move |_, button| {
                let self_ = Self::from_instance(&obj);
                log::debug!("mouse-release: {:?}", button);
            }));

            obj.connect_scroll_discrete(clone!(@weak obj => move |_, scroll| {
                let self_ = Self::from_instance(&obj);
                log::debug!("scroll-discrete: {:?}", scroll);
            }));

            obj.connect_resize_request(clone!(@weak obj => move |_, width, height, wmm, hmm| {
                log::debug!("resize-request: {:?}", (width, height, wmm, hmm));
            }));
        }
    }

    impl WidgetImpl for Display {}

    impl rdw::DisplayImpl for Display {}

    impl Display {
        pub(crate) fn connect(&self) {
            let notifier = self.notifier.clone();
            let context = self.context.clone();
            let (tx, rx) = mpsc::channel();
            self.tx.set(tx).unwrap();
            let thread = thread::spawn(move || {
                let mut ctxt = context.lock().unwrap();
                ctxt.instance.connect()?;

                let notifier_handle = notifier.handle();
                while !ctxt.instance.shall_disconnect() {
                    let handles = ctxt.event_handles().unwrap();
                    let mut handles: Vec<_> = handles.iter().collect();
                    handles.push(&notifier_handle);
                    wait_for_multiple_objects(&handles, false, None).unwrap();

                    if let WaitResult::Object(_) = notifier_handle.wait(Some(&Duration::ZERO))? {
                        match rx.recv() {
                            Ok(e) => {
                                dbg!(e);
                            }
                            _ => return Err(RdpError::Failed("recv() failed!".into())),
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
        &mut self,
        f: impl FnOnce(&mut freerdp::Settings) -> Result<()>,
    ) -> Result<()> {
        let self_ = imp::Display::from_instance(self);

        f(&mut self_.context.lock().unwrap().settings)
    }

    pub fn rdp_connect(&mut self) {
        let self_ = imp::Display::from_instance(self);

        self_.connect();
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
        dbg!(self);
        dbg!(pointer);
        let h = context.handler_mut();
        dbg!(h);
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
        gdi.resize(
            context.settings.desktop_width(),
            context.settings.desktop_height(),
        )?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct RdpContextHandler {
    test: u32,
}

impl RdpContextHandler {
    fn update_buffer(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let x = u32::try_from(x)?;
        let y = u32::try_from(y)?;
        let w = u32::try_from(w)?;
        let h = u32::try_from(h)?;
        dbg!((x, y, w, h));
        Ok(())
    }
}

impl freerdp::client::Handler for RdpContextHandler {
    fn post_connect(&mut self, context: &mut freerdp::client::Context<Self>) -> Result<()> {
        context.instance.gdi_init(PIXEL_FORMAT_BGRA32)?;

        let gdi = context.gdi().unwrap();
        let mut graphics = context.graphics().unwrap();
        let mut update = context.update().unwrap();

        let (w, h) = match (gdi.width(), gdi.height()) {
            (Some(w), Some(h)) => (w, h),
            _ => return Err(RdpError::Failed("No GDI dimensions".into())),
        };
        dbg!((w, h));

        graphics.register_pointer::<RdpPointerHandler>();
        update.register::<RdpUpdateHandler>();

        let _ = keyboard_init_ex(
            context.settings.keyboard_layout(),
            context.settings.keyboard_remapping_list().as_deref(),
        );

        Ok(())
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
        let fd = eventfd(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK)
            .map_err(|e| RdpError::Failed(format!("eventfd failed: {}", e)))?;

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
            .map_err(|e| RdpError::Failed(format!("notify() failed")))?;
        Ok(())
    }

    fn read_sync(&self) -> Result<()> {
        let st = unsafe { gio::UnixInputStream::with_fd(self.inner.fd) };
        let buffer = 1u64.to_ne_bytes();
        st.read_all(buffer, NONE_CANCELLABLE)
            .map_err(|e| RdpError::Failed(format!("read() failed")))?;
        Ok(())
    }
}
