use std::{sync::mpsc, thread, time::Duration};

use freerdp::{
    winpr::{wait_for_multiple_objects, WaitResult},
    RdpError, Result,
};
use futures::stream::StreamExt;
use glib::{clone, subclass::prelude::*, translate::*, SignalHandlerId};
use gtk::{glib, prelude::*};

use rdw::{gtk, DisplayExt};

use crate::{
    handlers::{RdpContextHandler, RdpEvent},
    notifier::Notifier,
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
            let mut context = freerdp::client::Context::new(RdpContextHandler::new(tx));
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
                        let imp = Self::from_instance(&obj);
                        let flags = if xt & 0x100 > 0 {
                            KbdFlags::EXTENDED
                        } else {
                            KbdFlags::empty()
                        };
                        if event.contains(rdw::KeyEvent::PRESS) {
                            let _ = imp.send_event(Event::Keyboard(flags | KbdFlags::DOWN, xt)).await;
                        }
                        if event.contains(rdw::KeyEvent::RELEASE) {
                            let _ = imp.send_event(Event::Keyboard(flags | KbdFlags::RELEASE, xt)).await;
                        }
                    }));
                }
            }));

            obj.connect_motion(clone!(@weak obj => move |_, x, y| {
                log::debug!("motion: {:?}", (x, y));
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let imp = Self::from_instance(&obj);
                    imp.last_mouse.set((x, y));
                    let _ = imp.send_event(Event::Mouse(PtrFlags::MOVE, x as _, y as _)).await;
                }));
            }));

            obj.connect_motion_relative(clone!(@weak obj => move |_, dx, dy| {
                log::debug!("motion-relative: {:?}", (dx, dy));
            }));

            obj.connect_mouse_press(clone!(@weak obj => move |_, button| {
                log::debug!("mouse-press: {:?}", button);
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let imp = Self::from_instance(&obj);
                    let _ = imp.mouse_click(true, button).await;
                }));
            }));

            obj.connect_mouse_release(clone!(@weak obj => move |_, button| {
                log::debug!("mouse-release: {:?}", button);
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let imp = Self::from_instance(&obj);
                    let _ = imp.mouse_click(false, button).await;
                }));
            }));

            obj.connect_resize_request(clone!(@weak obj => move |_, width, height, wmm, hmm| {
                log::debug!("resize-request: {:?}", (width, height, wmm, hmm));
                MainContext::default().spawn_local(glib::clone!(@weak obj => async move {
                    let imp = Self::from_instance(&obj);
                    let _ = imp.send_event(Event::MonitorLayout(vec![MonitorLayout::new(
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
                    let imp = Self::from_instance(&widget);
                    let _ = imp.mouse_scroll(PtrFlags::HWHEEL, dx).await;
                    let _ = imp.mouse_scroll(PtrFlags::WHEEL, dy).await;
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
        let imp = imp::Display::from_instance(self);

        imp.with_settings(f)
    }

    pub fn rdp_connect(&mut self) -> Result<()> {
        let imp = imp::Display::from_instance(self);

        imp.connect(self)
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
