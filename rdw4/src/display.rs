#[cfg(unix)]
use gdk_wl::prelude::*;
use glib::{signal::SignalHandlerId, subclass::prelude::*, translate::*};
use gtk::{gdk, glib, prelude::*, subclass::prelude::WidgetImpl};

#[cfg(all(unix, not(feature = "bindings")))]
use gdk_wl::wayland_client::{self, protocol::wl_registry};
#[cfg(all(unix, not(feature = "bindings")))]
use wayland_protocols::wp::{
    pointer_constraints::zv1::client::{
        zwp_locked_pointer_v1::ZwpLockedPointerV1,
        zwp_pointer_constraints_v1::{self, ZwpPointerConstraintsV1},
    },
    relative_pointer::zv1::client::{
        zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
        zwp_relative_pointer_v1::{Event as RelEvent, ZwpRelativePointerV1},
    },
};

#[cfg(all(windows, not(feature = "bindings")))]
use gdk_win32::windows;

#[cfg(unix)]
use crate::RdwDmabufScanout;
use crate::{Grab, KeyEvent, Scroll};

#[cfg(all(unix, not(feature = "bindings")))]
use crate::egl;

#[repr(C)]
pub struct RdwDisplayClass {
    pub parent_class: gtk::ffi::GtkWidgetClass,
}

#[repr(C)]
pub struct RdwDisplay {
    parent: gtk::ffi::GtkWidget,
}

impl std::fmt::Debug for RdwDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("RdwDisplay")
            .field("parent", &self.parent)
            .finish()
    }
}

#[cfg(not(feature = "bindings"))]
pub mod imp {
    use super::*;
    use crate::error::Error;
    use crate::util;
    #[cfg(windows)]
    use crate::win32;
    use gl::types::*;
    use glib::{clone, subclass::Signal, SourceId};
    use gtk::{graphene, subclass::prelude::*};
    use once_cell::sync::{Lazy, OnceCell};
    use std::{
        cell::{Cell, RefCell},
        time::Duration,
    };
    #[cfg(unix)]
    use x11::xlib;

    unsafe impl ClassStruct for RdwDisplayClass {
        type Type = Display;
    }

    unsafe impl InstanceStruct for RdwDisplay {
        type Type = Display;
    }

    #[derive(Default)]
    pub struct Display {
        pub(crate) gl_area: OnceCell<gtk::GLArea>,
        pub(crate) layout_manager: OnceCell<gtk::BinLayout>,

        // The remote display size, ex: 1024x768
        pub(crate) display_size: Cell<Option<(usize, usize)>>,
        pub(crate) last_resize_request: Cell<Option<(u32, u32, u32, u32)>>,
        pub(crate) resize_timeout_id: Cell<Option<SourceId>>,
        // The currently defined cursor
        pub(crate) cursor: RefCell<Option<gdk::Cursor>>,
        pub(crate) mouse_absolute: Cell<bool>,
        // position of cursor when drawn by client
        pub(crate) cursor_position: Cell<Option<(usize, usize)>>,
        // press-and-release detection time in ms
        pub(crate) synthesize_delay: Cell<u32>,
        pub(crate) last_key_press: Cell<Option<(u32, u32)>>,
        pub(crate) last_key_press_timeout: Cell<Option<SourceId>>,

        // the shortcut to ungrab key/mouse (to be configurable and extended with ctrl-alt)
        pub(crate) grab_shortcut: OnceCell<gtk::ShortcutTrigger>,
        pub(crate) grabbed: Cell<Grab>,
        pub(crate) shortcuts_inhibited_id: Cell<Option<SignalHandlerId>>,
        pub(crate) grab_ec: glib::WeakRef<gtk::EventControllerKey>,

        #[cfg(unix)]
        pub(crate) egl_ctx: OnceCell<egl::Context>,
        #[cfg(unix)]
        pub(crate) egl_cfg: OnceCell<egl::Config>,
        #[cfg(unix)]
        pub(crate) egl_surf: OnceCell<egl::Surface>,

        pub(crate) texture_id: Cell<GLuint>,
        pub(crate) texture_blit_vao: Cell<GLuint>,
        pub(crate) texture_blit_prog: Cell<GLuint>,
        pub(crate) texture_blit_flip_prog: Cell<GLuint>,
        #[cfg(unix)]
        pub(crate) dmabuf: RefCell<Option<RdwDmabufScanout>>,

        #[cfg(unix)]
        pub(crate) wl_queue: OnceCell<wayland_client::QueueHandle<crate::Display>>,
        #[cfg(unix)]
        pub(crate) wl_source: Cell<Option<glib::SourceId>>,
        #[cfg(unix)]
        pub(crate) wl_rel_manager: OnceCell<ZwpRelativePointerManagerV1>,
        #[cfg(unix)]
        pub(crate) wl_rel_pointer: RefCell<Option<ZwpRelativePointerV1>>,
        #[cfg(unix)]
        pub(crate) wl_pointer_constraints: OnceCell<ZwpPointerConstraintsV1>,
        #[cfg(unix)]
        pub(crate) wl_lock_pointer: RefCell<Option<ZwpLockedPointerV1>>,

        #[cfg(windows)]
        pub(crate) win_mouse: Cell<[isize; 3]>,
        #[cfg(windows)]
        pub(crate) win_mouse_speed: Cell<isize>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwDisplay";
        type Type = super::Display;
        type ParentType = gtk::Widget;
        type Class = RdwDisplayClass;
        type Instance = RdwDisplay;

        fn class_init(_klass: &mut Self::Class) {
            // Load GL pointers from epoxy (GL context management library used by GTK).
            {
                #[cfg(target_os = "macos")]
                let library =
                    unsafe { libloading::os::unix::Library::new("libepoxy.0.dylib") }.unwrap();
                #[cfg(all(unix, not(target_os = "macos")))]
                let library =
                    unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }.unwrap();
                #[cfg(windows)]
                let library =
                    libloading::os::windows::Library::open_already_loaded("libepoxy-0.dll")
                        .or_else(|_| {
                            libloading::os::windows::Library::open_already_loaded("epoxy-0.dll")
                        })
                        .unwrap();

                epoxy::load_with(|name| {
                    unsafe { library.get::<_>(name.as_bytes()) }
                        .map(|symbol| *symbol)
                        .unwrap_or(std::ptr::null())
                });
                gl::load_with(epoxy::get_proc_addr);
            }
        }
    }

    impl ObjectImpl for Display {
        fn constructed(&self) {
            self.parent_constructed();
            self.layout_manager.set(gtk::BinLayout::new()).unwrap();

            let gl_area = gtk::GLArea::new();
            gl_area.set_has_depth_buffer(false);
            gl_area.set_has_stencil_buffer(false);
            gl_area.set_auto_render(false);
            gl_area.set_required_version(3, 2);
            gl_area.connect_render(
                clone!(@weak self as this => @default-return glib::signal::Inhibit(true), move |_, _| {
                    this.obj().render();
                    glib::signal::Inhibit(true)
                }),
            );
            gl_area.connect_realize(clone!(@weak self as this => move |_| {
                if let Err(e) = unsafe { this.realize_gl() } {
                    log::warn!("Failed to realize gl: {}", e);
                    let e = glib::Error::new(Error::GL, &e);
                    this.gl_area().set_error(Some(&e));
                }
            }));

            self.gl_area.set(gl_area).unwrap();

            self.grab_shortcut.get_or_init(|| {
                gtk::ShortcutTrigger::parse_string("<Ctrl>Alt_L|<Alt>Control_L").unwrap()
            });
        }

        fn dispose(&self) {
            #[cfg(unix)]
            if let Some(source) = self.wl_source.take() {
                source.remove();
            }
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn properties() -> &'static [glib::ParamSpec] {
            use glib::ParamFlags as Flags;

            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![
                    glib::ParamSpecObject::new(
                        "grab-shortcut",
                        "Grab shortcut",
                        "Input devices grab/ungrab shortcut",
                        gtk::ShortcutTrigger::static_type(),
                        Flags::READWRITE,
                    ),
                    glib::ParamSpecFlags::new(
                        "grabbed",
                        "grabbed",
                        "Grabbed",
                        Grab::static_type(),
                        Grab::empty().into_glib(),
                        Flags::READABLE,
                    ),
                    glib::ParamSpecUInt::new(
                        "synthesize-delay",
                        "Synthesize delay",
                        "Press-and-release synthesize maximum time in ms",
                        u32::MIN,
                        u32::MAX,
                        100,
                        Flags::READWRITE | Flags::CONSTRUCT,
                    ),
                    glib::ParamSpecBoolean::new(
                        "mouse-absolute",
                        "Mouse absolute",
                        "Whether the mouse is absolute or relative",
                        false,
                        Flags::READWRITE | Flags::CONSTRUCT,
                    ),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "grab-shortcut" => {
                    let shortcut = value.get().unwrap();
                    self.grab_shortcut.set(shortcut).unwrap();
                }
                "synthesize-delay" => {
                    let delay = value.get().unwrap();
                    self.synthesize_delay.set(delay);
                }
                "mouse-absolute" => {
                    let absolute = value.get().unwrap();
                    if absolute {
                        self.ungrab_mouse();
                        self.gl_area().set_cursor(self.cursor.borrow().as_ref());
                    }

                    self.mouse_absolute.set(absolute);
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "grab-shortcut" => self.grab_shortcut.get().to_value(),
                "grabbed" => self.grabbed.get().to_value(),
                "synthesize-delay" => self.synthesize_delay.get().to_value(),
                "mouse-absolute" => self.mouse_absolute.get().to_value(),
                _ => unimplemented!(),
            }
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![
                    Signal::builder("key-event")
                        .param_types([
                            u32::static_type(),
                            u32::static_type(),
                            KeyEvent::static_type(),
                        ])
                        .build(),
                    Signal::builder("motion")
                        .param_types([f64::static_type(), f64::static_type()])
                        .build(),
                    Signal::builder("motion-relative")
                        .param_types([f64::static_type(), f64::static_type()])
                        .build(),
                    Signal::builder("mouse-press")
                        .param_types([u32::static_type()])
                        .build(),
                    Signal::builder("mouse-release")
                        .param_types([u32::static_type()])
                        .build(),
                    Signal::builder("scroll-discrete")
                        .param_types([Scroll::static_type()])
                        .build(),
                    Signal::builder("resize-request")
                        .param_types([
                            u32::static_type(),
                            u32::static_type(),
                            u32::static_type(),
                            u32::static_type(),
                        ])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for Display {
        fn realize(&self) {
            self.parent_realize();

            self.obj().set_sensitive(true);
            self.obj().set_focusable(true);
            self.obj().set_focus_on_click(true);

            if self.realize_egl() {
                if let Err(e) = unsafe { self.realize_gl() } {
                    log::warn!("Failed to realize GL: {}", e);
                }
            } else {
                self.gl_area().set_parent(&*self.obj());
            }

            #[cfg(unix)]
            if let Ok(dpy) = self.obj().display().downcast::<gdk_wl::WaylandDisplay>() {
                self.realize_wl(&dpy);
            }

            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            self.obj().add_controller(&ec);
            ec.connect_key_pressed(
                clone!(@weak self as this => @default-panic, move |ec, keyval, keycode, _state| {
                    this.key_pressed(ec, keyval, keycode);
                    glib::signal::Inhibit(true)
                }),
            );
            ec.connect_key_released(
                clone!(@weak self as this => move |_, keyval, keycode, _state| {
                    this.key_released(keyval, keycode);
                }),
            );

            let ec = gtk::EventControllerMotion::new();
            self.obj().add_controller(&ec);
            ec.connect_motion(clone!(@weak self as this => move |_, x, y| {
                if let Some((x, y)) = this.transform_pos(x, y) {
                    this.obj().emit_by_name::<()>("motion", &[&x, &y]);
                }
            }));
            ec.connect_enter(clone!(@weak self as this => move |_, x, y| {
                if let Some((x, y)) = this.transform_pos(x, y) {
                    this.obj().emit_by_name::<()>("motion", &[&x, &y]);
                }
            }));
            ec.connect_leave(clone!(@weak self as this => move |_| {
                this.ungrab_keyboard();
            }));

            let ec = gtk::GestureClick::new();
            ec.set_button(0);
            self.obj().add_controller(&ec);
            ec.connect_pressed(
                clone!(@weak self as this => @default-panic, move |gesture, _n_press, x, y| {
                    this.try_grab();

                    let button = gesture.current_button();
                    if let Some((x, y)) = this.transform_pos(x, y) {
                        this.obj().emit_by_name::<()>("motion", &[&x, &y]);
                    }
                    this.obj().emit_by_name::<()>("mouse-press", &[&button]);
                }),
            );
            ec.connect_released(
                clone!(@weak self as this => move |gesture, _n_press, x, y| {
                    let button = gesture.current_button();
                    if let Some((x, y)) = this.transform_pos(x, y) {
                        this.obj().emit_by_name::<()>("motion", &[&x, &y]);
                    }
                    this.obj().emit_by_name::<()>("mouse-release", &[&button]);
                }),
            );

            let ec = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::BOTH_AXES
                    | gtk::EventControllerScrollFlags::DISCRETE,
            );
            self.obj().add_controller(&ec);
            ec.connect_scroll(
                clone!(@weak self as this => @default-panic, move |_, dx, dy| {
                    if dy >= 1.0 {
                        this.obj().emit_by_name::<()>("scroll-discrete", &[&Scroll::Down]);
                    } else if dy <= -1.0 {
                        this.obj().emit_by_name::<()>("scroll-discrete", &[&Scroll::Up]);
                    }
                    if dx >= 1.0 {
                        this.obj().emit_by_name::<()>("scroll-discrete", &[&Scroll::Right]);
                    } else if dx <= -1.0 {
                        this.obj().emit_by_name::<()>("scroll-discrete", &[&Scroll::Left]);
                    }
                    glib::signal::Inhibit(false)
                }),
            );
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let (minimum, mut natural, minimum_baseline, natural_baseline) = (128, 128, -1, -1);

            // TODO: doesn't work as expected yet
            if let Some((w, h)) = self.display_size.get() {
                match orientation {
                    gtk::Orientation::Horizontal => {
                        natural = w as _;
                    }
                    gtk::Orientation::Vertical => {
                        natural = h as _;
                    }
                    _ => panic!(),
                }
            }

            (minimum, natural, minimum_baseline, natural_baseline)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.layout_manager
                .get()
                .unwrap()
                .allocate(&*self.obj(), width, height, baseline);

            if let Some(timeout_id) = self.resize_timeout_id.take() {
                timeout_id.remove();
            }
            self.resize_timeout_id.set(Some(glib::timeout_add_local(
                Duration::from_millis(500),
                clone!(@weak self as this => @default-return glib::Continue(false), move || {
                    let sf = this.obj().scale_factor() as u32;
                    let width = width as u32 * sf;
                    let height = height as u32 * sf;
                    let (w_mm, h_mm) = this.surface()
                                   .as_ref()
                                   .map(|s| gdk::traits::DisplayExt::monitor_at_surface(&this.obj().display(), s))
                                   .map(|m| {
                                       let (geom, wmm, hmm) = (m.geometry(), m.width_mm() as u32, m.height_mm() as u32);
                                       (wmm * width / (geom.width() as u32), hmm * height / geom.height() as u32)
                                   }).unwrap_or((0u32, 0u32));
                    if Some((width, height, w_mm, h_mm)) != this.last_resize_request.get() {
                        this.last_resize_request.set(Some((width, height, w_mm, h_mm)));
                        this.obj().emit_by_name::<()>("resize-request", &[&width, &height, &w_mm, &h_mm]);
                    }
                    this.resize_timeout_id.set(None);
                    glib::Continue(false)
                }),
            )));
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            snapshot.save();
            self.parent_snapshot(snapshot);
            snapshot.restore();

            if self.obj().mouse_absolute() {
                return;
            }
            if !self.grabbed.get().contains(Grab::MOUSE) {
                return;
            }
            if let Some(pos) = self.cursor_position.get() {
                if let Some(cursor) = &*self.cursor.borrow() {
                    if let Some(texture) = cursor.texture() {
                        // don't take hotspot as an offset (it's not for hw cursor)
                        if let Some((x, y)) = self.transform_pos_inv(pos.0 as _, pos.1 as _) {
                            let sf = self.obj().scale_factor();

                            snapshot.append_texture(
                                &texture,
                                &graphene::Rect::new(
                                    x as f32,
                                    y as f32,
                                    (texture.width() / sf) as f32,
                                    (texture.height() / sf) as f32,
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    pub(crate) struct ContextGuard<'a>(&'a Display);

    impl Drop for ContextGuard<'_> {
        fn drop(&mut self) {
            self.0.clear_current();
        }
    }

    impl Display {
        pub(crate) fn clear_current(&self) {
            #[cfg(unix)]
            if let (Some(dpy), Some(_)) = (self.egl_display(), self.egl_surface()) {
                let _ = egl::egl().make_current(dpy, None, None, None);
            }
        }

        fn make_current_gl_area(&self) {
            let area = self.gl_area();
            area.make_current();
            area.attach_buffers();
        }

        pub(crate) fn make_current(&self) -> ContextGuard {
            #[cfg(unix)]
            if let (Some(dpy), surf, Some(ctx)) =
                (self.egl_display(), self.egl_surface(), self.egl_context())
            {
                gdk::GLContext::clear_current();
                if let Err(e) = egl::egl().make_current(dpy, surf, surf, Some(ctx)) {
                    log::warn!("Failed to make current context: {}", e);
                }
            } else {
                self.make_current_gl_area();
            }

            #[cfg(not(unix))]
            self.make_current_gl_area();

            ContextGuard(self)
        }

        #[cfg(not(unix))]
        fn realize_egl(&self) -> bool {
            false
        }

        #[cfg(unix)]
        fn realize_egl(&self) -> bool {
            // necessary on X11 to have an EGL context for dmabuf imports
            if let (Some(dpy), Some(_), Some(xid)) =
                (self.egl_display(), self.egl_context(), self.x11_xid())
            {
                match unsafe {
                    egl::egl().create_window_surface(
                        dpy,
                        *self.egl_cfg.get().expect("egl config missing"),
                        xid as _,
                        None,
                    )
                } {
                    Ok(surf) => {
                        log::debug!("Initialized EGL surface successfully");
                        self.egl_surf.set(surf).unwrap();
                    }
                    Err(e) => {
                        log::warn!("Failed to create egl surface: {}", e);
                    }
                }
                true
            } else {
                false
            }
        }

        #[cfg(unix)]
        fn realize_wl(&self, dpy: &gdk_wl::WaylandDisplay) {
            use std::os::unix::io::AsRawFd;
            use wayland_client::{backend::Backend, globals::registry_queue_init, Connection};

            let wl_display =
                unsafe { gdk_wl::ffi::gdk_wayland_display_get_wl_display(dpy.to_glib_none().0) };
            let connection = Connection::from_backend(unsafe {
                Backend::from_foreign_display(wl_display as *mut _)
            });
            let (globals, mut queue) = registry_queue_init::<crate::Display>(&connection).unwrap();

            let rel_manager = globals.bind(&queue.handle(), 1..=1, ()).unwrap();
            self.wl_rel_manager.set(rel_manager).unwrap();
            let pointer_constraints = globals.bind(&queue.handle(), 1..=1, ()).unwrap();
            self.wl_pointer_constraints
                .set(pointer_constraints)
                .unwrap();

            let fd = connection
                .prepare_read()
                .unwrap()
                .connection_fd()
                .as_raw_fd();
            let source = glib::source::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, _| {
                connection.prepare_read().unwrap().read().unwrap();
                glib::Continue(true)
            });

            self.wl_queue.set(queue.handle()).unwrap();
            glib::MainContext::default().spawn_local(
                clone!(@weak self as this => @default-panic, async move {
                    let mut obj = this.obj().clone();
                    std::future::poll_fn(|cx| queue.poll_dispatch_pending(cx, &mut obj)).await.unwrap();
                })
            );
            self.wl_source.set(Some(source))
        }

        pub(crate) fn gl_area(&self) -> &gtk::GLArea {
            self.gl_area.get().unwrap()
        }

        unsafe fn realize_gl(&self) -> Result<(), String> {
            use std::ffi::CString;
            let _ctxt = self.make_current();

            let texture_blit_vs = CString::new(include_str!("texture-blit.vert")).unwrap();
            let texture_blit_flip_vs =
                CString::new(include_str!("texture-blit-flip.vert")).unwrap();
            let texture_blit_fs = CString::new(include_str!("texture-blit.frag")).unwrap();

            let texture_blit_prg =
                util::compile_gl_prog(texture_blit_vs.as_c_str(), texture_blit_fs.as_c_str())?;
            self.texture_blit_prog.set(texture_blit_prg);
            let texture_blit_flip_prg =
                util::compile_gl_prog(texture_blit_flip_vs.as_c_str(), texture_blit_fs.as_c_str())?;
            self.texture_blit_flip_prog.set(texture_blit_flip_prg);

            let mut vao = 0;
            gl::GenVertexArrays(1, &mut vao);
            gl::BindVertexArray(vao);
            let mut vb = 0;
            gl::GenBuffers(1, &mut vb);
            gl::BindBuffer(gl::ARRAY_BUFFER, vb);
            static POS: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of::<[f32; 8]>() as _,
                POS.as_ptr() as _,
                gl::STATIC_DRAW,
            );
            let in_pos = gl::GetAttribLocation(
                texture_blit_prg,
                CString::new("in_position").unwrap().as_c_str().as_ptr(),
            ) as u32;
            gl::VertexAttribPointer(in_pos, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());
            gl::EnableVertexAttribArray(in_pos);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);
            self.texture_blit_vao.set(vao);

            let tex_unit = gl::GetUniformLocation(
                texture_blit_prg,
                CString::new("tex_unit").unwrap().as_c_str().as_ptr(),
            );
            gl::ProgramUniform1i(texture_blit_prg, tex_unit, 0);

            let mut tex_id = 0;
            gl::GenTextures(1, &mut tex_id);
            self.texture_id.set(tex_id);

            Ok(())
        }

        fn ungrab_keyboard(&self) {
            if !self.grabbed.get().contains(Grab::KEYBOARD) {
                return;
            }

            if let Some(ec) = self.grab_ec.upgrade() {
                ec.widget().remove_controller(&ec);
                self.grab_ec.set(None);
            }
            if let Some(toplevel) = self.toplevel() {
                toplevel.restore_system_shortcuts();
                self.grabbed.set(self.grabbed.get() - Grab::KEYBOARD);
                self.obj().notify("grabbed");
            }
        }

        pub(crate) fn ungrab_mouse(&self) {
            if self.grabbed.get().contains(Grab::MOUSE) {
                #[cfg(unix)]
                if let Some(lock) = self.wl_lock_pointer.take() {
                    lock.destroy();
                }
                #[cfg(unix)]
                if let Some(rel_pointer) = self.wl_rel_pointer.take() {
                    rel_pointer.destroy();
                }
                #[cfg(windows)]
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::ClipCursor(None)
                };

                self.restore_accel_mouse();

                self.grabbed.set(self.grabbed.get() - Grab::MOUSE);
                if !self.obj().mouse_absolute() {
                    self.gl_area().set_cursor(None);
                }
                self.obj().queue_draw(); // update cursor
                self.obj().notify("grabbed");
            }
        }

        fn emit_last_key_press(&self) {
            if let Some((keyval, keycode)) = self.last_key_press.take() {
                self.obj()
                    .emit_by_name::<()>("key-event", &[&keyval, &keycode, &KeyEvent::PRESS]);
            }

            if let Some(timeout_id) = self.last_key_press_timeout.take() {
                timeout_id.remove();
            }
        }

        fn key_pressed(&self, ec: &gtk::EventControllerKey, keyval: gdk::Key, keycode: u32) {
            if let Some(ref e) = ec.current_event() {
                if self.grab_shortcut.get().unwrap().trigger(e, false) == gdk::KeyMatch::Exact {
                    if self.grabbed.get().is_empty() {
                        self.try_grab();
                    } else {
                        self.ungrab_keyboard();
                        self.ungrab_mouse();
                    }
                    return;
                }
            }

            // flush pending key event
            self.emit_last_key_press();

            // synthesize press-and-release if within the synthesize-delay boundary, else emit
            self.last_key_press.set(Some((keyval.into_glib(), keycode)));
            self.last_key_press_timeout
                .set(Some(glib::timeout_add_local(
                    Duration::from_millis(self.synthesize_delay.get() as _),
                    glib::clone!(@weak self as this => @default-return glib::Continue(false), move || {
                        this.emit_last_key_press();
                        glib::Continue(false)
                    }),
                )));
        }

        fn key_released(&self, keyval: gdk::Key, keycode: u32) {
            if let Some((last_keyval, last_keycode)) = self.last_key_press.get() {
                if (last_keyval, last_keycode) == (keyval.into_glib(), keycode) {
                    self.last_key_press.set(None);
                    if let Some(timeout_id) = self.last_key_press_timeout.take() {
                        timeout_id.remove();
                    }

                    self.obj().emit_by_name::<()>(
                        "key-event",
                        &[
                            &keyval.into_glib(),
                            &keycode,
                            &(KeyEvent::PRESS | KeyEvent::RELEASE),
                        ],
                    );
                    return;
                }
            }

            // flush pending key event
            self.emit_last_key_press();

            self.obj().emit_by_name::<()>(
                "key-event",
                &[&keyval.into_glib(), &keycode, &KeyEvent::RELEASE],
            )
        }

        fn try_grab_keyboard(&self) -> bool {
            if self.grabbed.get().contains(Grab::KEYBOARD) {
                return false;
            }

            let toplevel = match self.toplevel() {
                Some(toplevel) => toplevel,
                _ => return false,
            };

            toplevel.inhibit_system_shortcuts(None::<&gdk::ButtonEvent>);
            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            ec.connect_key_pressed(clone!(@weak self as this, @weak toplevel => @default-panic, move |ec, keyval, keycode, _state| {
                this.key_pressed(ec, keyval, keycode);
                glib::signal::Inhibit(true)
            }));
            ec.connect_key_released(
                clone!(@weak self as this => @default-panic, move |_ec, keyval, keycode, _state| {
                    this.key_released(keyval, keycode);
                }),
            );
            if let Some(root) = self.obj().root() {
                root.add_controller(&ec);
            }
            self.grab_ec.set(Some(&ec));

            let id = toplevel.connect_shortcuts_inhibited_notify(
                clone!(@weak self as this => @default-panic, move |toplevel| {
                    let inhibited = toplevel.is_shortcuts_inhibited();
                    log::debug!("shortcuts-inhibited: {}", inhibited);
                    if !inhibited {
                        let id = this.shortcuts_inhibited_id.take();
                        toplevel.disconnect(id.unwrap());
                        this.ungrab_keyboard();
                    }
                }),
            );
            self.shortcuts_inhibited_id.set(Some(id));
            true
        }

        #[cfg(unix)]
        fn try_grab_device(&self, device: gdk::Device) -> bool {
            let device = match device.downcast::<gdk_wl::WaylandDevice>() {
                Ok(device) => device,
                _ => return false,
            };
            let pointer = device.wl_pointer().unwrap();
            let handle = self.wl_queue.get().unwrap();

            if self.wl_lock_pointer.borrow().is_none() {
                if let Some(constraints) = self.wl_pointer_constraints.get() {
                    if let Some(surf) = self.wl_surface() {
                        let lock = constraints.lock_pointer(
                            &surf,
                            &pointer,
                            None,
                            zwp_pointer_constraints_v1::Lifetime::Persistent as _,
                            handle,
                            (),
                        );
                        self.wl_lock_pointer.replace(Some(lock));
                    }
                }
            }

            if self.wl_rel_pointer.borrow().is_none() {
                let handle = self.wl_queue.get().unwrap();
                if let Some(rel_manager) = self.wl_rel_manager.get() {
                    let rel_pointer = rel_manager.get_relative_pointer(&pointer, handle, ());
                    self.wl_rel_pointer.replace(Some(rel_pointer));
                }
            }

            true
        }

        #[cfg(windows)]
        fn try_grab_device(&self, _device: gdk::Device) -> bool {
            use windows::Win32::Graphics::Gdi::{
                GetMonitorInfoA, IntersectRect, MonitorFromRect, MONITORINFO,
                MONITOR_DEFAULTTONEAREST,
            };
            use windows::Win32::UI::WindowsAndMessaging::{ClipCursor, GetWindowRect};

            let h = self.win32_handle();
            let mut win_rect = unsafe { std::mem::zeroed() };
            if let Err(e) = unsafe { GetWindowRect(h, &mut win_rect).ok() } {
                log::warn!("Failed to GetWindowRect: {e}");
                return false;
            }

            let h = unsafe { MonitorFromRect(&win_rect, MONITOR_DEFAULTTONEAREST) };
            if h.is_invalid() {
                log::warn!("Failed to MonitorFromRect");
                return false;
            }

            let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of_val(&info) as _;
            if let Err(e) = unsafe { GetMonitorInfoA(h, &mut info).ok() } {
                log::warn!("Failed to GetMonitorInfoA: {e}");
                return false;
            }

            let mut rect = unsafe { std::mem::zeroed() };
            if let Err(e) = unsafe { IntersectRect(&mut rect, &win_rect, &info.rcWork).ok() } {
                log::warn!("Failed to IntersectRect: {e}");
                return false;
            }

            if let Err(e) = unsafe { ClipCursor(Some(&rect)).ok() } {
                log::warn!("Failed to ClipCursor: {e}");
                return false;
            }

            true
        }

        #[cfg(not(any(unix, windows)))]
        fn try_grab_device(&self, _device: gdk::Device) -> bool {
            false
        }

        fn try_grab_mouse(&self) -> bool {
            if self.obj().mouse_absolute() {
                // we could eventually grab the mouse in client mode, but what's the point?
                return false;
            }
            if self.obj().grabbed().contains(Grab::MOUSE) {
                return false;
            }

            if let Some(default_seat) = gdk::traits::DisplayExt::default_seat(&self.obj().display())
            {
                for device in default_seat.devices(gdk::SeatCapabilities::POINTER) {
                    if !self.try_grab_device(device) {
                        return false;
                    }
                }
            }

            self.save_accel_mouse();

            true
        }

        fn save_accel_mouse(&self) {
            #[cfg(windows)]
            {
                match win32::spi_get_mouse() {
                    Ok(mouse) => self.win_mouse.set(mouse),
                    Err(e) => log::warn!("Failed to spi_get_mouse: {e}"),
                }
                match win32::spi_get_mouse_speed() {
                    Ok(speed) => self.win_mouse_speed.set(speed),
                    Err(e) => log::warn!("Failed to spi_get_mouse: {e}"),
                }

                let mouse: [isize; 3] = Default::default();
                if let Err(e) = win32::spi_set_mouse(mouse) {
                    log::warn!("Failed to spi_set_mouse: {e}");
                }
                if let Err(e) = win32::spi_set_mouse_speed(10) {
                    log::warn!("Failed to spi_set_mouse_speed: {e}");
                }
            }
            #[cfg(not(windows))]
            {
                // todo
            }
        }

        fn restore_accel_mouse(&self) {
            #[cfg(windows)]
            {
                if let Err(e) = win32::spi_set_mouse(self.win_mouse.get()) {
                    log::warn!("Failed to spi_set_mouse: {e}");
                }
                if let Err(e) = win32::spi_set_mouse_speed(self.win_mouse_speed.get()) {
                    log::warn!("Failed to spi_set_mouse_speed: {e}");
                }
            }
            #[cfg(not(windows))]
            {
                // todo
            }
        }

        fn try_grab(&self) {
            let mut grabbed = self.obj().grabbed();
            if self.try_grab_keyboard() {
                grabbed |= Grab::KEYBOARD;
            }
            if self.try_grab_mouse() {
                grabbed |= Grab::MOUSE;
                if !self.obj().mouse_absolute() {
                    // hide client mouse
                    self.gl_area().set_cursor_from_name(Some("none"));
                }
                self.obj().queue_draw(); // update cursor
            }
            self.grabbed.set(grabbed);
            self.obj().notify("grabbed");
        }

        pub(crate) fn texture_id(&self) -> GLuint {
            self.texture_id.get()
        }

        pub(crate) fn texture_blit(&self, flip: bool) {
            unsafe {
                gl::UseProgram(if flip {
                    self.texture_blit_flip_prog.get()
                } else {
                    self.texture_blit_prog.get()
                });
                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindTexture(gl::TEXTURE_2D, self.texture_id());
                gl::BindVertexArray(self.texture_blit_vao.get());
                gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
            }
        }

        fn borders(&self) -> (u32, u32) {
            let obj = self.obj();
            let (dw, dh) = match obj.display_size() {
                Some(size) => size,
                None => return (0, 0),
            };
            let sf = obj.scale_factor();
            let (w, h) = (obj.width() * sf, obj.height() * sf);
            let (sw, sh) = (w as f32 / dw as f32, h as f32 / dh as f32);

            if sw < sh {
                let bh = h - (h as f32 * sw / sh) as i32;
                (0, bh as u32 / 2)
            } else {
                let bw = w - (w as f32 * sh / sw) as i32;
                (bw as u32 / 2, 0)
            }
        }

        pub(crate) fn viewport(&self) -> Option<gdk::Rectangle> {
            let obj = self.obj();
            obj.display_size()?;

            let sf = obj.scale_factor();
            let (w, h) = (obj.width() * sf, obj.height() * sf);
            let (borderw, borderh) = self.borders();
            let (borderw, borderh) = (borderw as i32, borderh as i32);
            Some(gdk::Rectangle::new(
                borderw,
                borderh,
                w - borderw * 2,
                h - borderh * 2,
            ))
        }

        // widget -> remote display pos
        fn transform_pos(&self, x: f64, y: f64) -> Option<(f64, f64)> {
            let obj = self.obj();
            let sf = obj.scale_factor() as f64;
            self.viewport().and_then(|vp| {
                let (x, y) = (x * sf, y * sf);
                if !vp.contains_point(x as _, y as _) {
                    return None;
                }
                let (sw, sh) = obj.display_size().unwrap();
                let x = (x - vp.x() as f64) * (sw as f64 / vp.width() as f64);
                let y = (y - vp.y() as f64) * (sh as f64 / vp.height() as f64);
                Some((x, y))
            })
        }

        // remote display pos -> widget pos
        fn transform_pos_inv(&self, x: f64, y: f64) -> Option<(f64, f64)> {
            let obj = self.obj();
            let sf = obj.scale_factor() as f64;
            self.viewport().map(|vp| {
                let (sw, sh) = obj.display_size().unwrap();
                let x = x * (vp.width() as f64 / sw as f64) + vp.x() as f64;
                let y = y * (vp.height() as f64 / sh as f64) + vp.y() as f64;
                (x / sf as f64, y / sf as f64)
            })
        }

        fn toplevel(&self) -> Option<gdk::Toplevel> {
            let obj = self.obj();
            obj.root()
                .and_then(|r| r.native())
                .map(|n| n.surface())
                .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
        }

        fn surface(&self) -> Option<gdk::Surface> {
            let obj = self.obj();
            obj.native().map(|n| n.surface())
        }

        #[cfg(windows)]
        fn win32_handle(&self) -> Option<gdk_win32::HWND> {
            self.surface()
                .and_then(|s| s.downcast::<gdk_win32::Win32Surface>().ok())
                .map(|w| w.handle())
        }

        #[cfg(unix)]
        fn egl_surface(&self) -> Option<egl::Surface> {
            self.egl_surf.get().copied()
        }

        #[cfg(unix)]
        fn wl_surface(&self) -> Option<wayland_client::protocol::wl_surface::WlSurface> {
            self.surface()
                .and_then(|s| s.downcast::<gdk_wl::WaylandSurface>().ok())
                .map(|w| w.wl_surface().unwrap())
        }

        #[cfg(unix)]
        fn x11_xid(&self) -> Option<xlib::Window> {
            self.surface()
                .and_then(|s| s.downcast::<gdk_x11::X11Surface>().ok())
                .map(|s| s.xid())
        }

        #[cfg(unix)]
        pub(crate) fn egl_context(&self) -> Option<egl::Context> {
            self.egl_display().and_then(|_| self.egl_ctx.get().copied())
        }

        #[cfg(unix)]
        pub(crate) fn egl_display(&self) -> Option<egl::Display> {
            let widget = self.obj();

            if let Ok(dpy) = widget.display().downcast::<gdk_wl::WaylandDisplay>() {
                return dpy.egl_display();
            }

            if let Ok(dpy) = widget.display().downcast::<gdk_x11::X11Display>() {
                return dpy.egl_display();
            };

            None
        }
    }
}

impl Display {
    pub fn make_cursor(
        data: &[u8],
        width: i32,
        height: i32,
        hot_x: i32,
        hot_y: i32,
        scale: i32,
    ) -> gdk::Cursor {
        let pb = gdk::gdk_pixbuf::Pixbuf::from_mut_slice(
            data.to_vec(),
            gdk::gdk_pixbuf::Colorspace::Rgb,
            true,
            8,
            width,
            height,
            width * 4,
        );
        let pb = pb
            .scale_simple(
                width * scale,
                height * scale,
                gdk::gdk_pixbuf::InterpType::Bilinear,
            )
            .unwrap();
        let tex = gdk::Texture::for_pixbuf(&pb);
        gdk::Cursor::from_texture(&tex, hot_x * scale, hot_y * scale, None)
    }
}

#[cfg(not(feature = "bindings"))]
#[cfg(unix)]
impl wayland_client::Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents>
    for Display
{
    fn event(
        _state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        log::trace!("{event:?}");
    }
}

#[cfg(not(feature = "bindings"))]
#[cfg(unix)]
impl wayland_client::Dispatch<ZwpRelativePointerManagerV1, ()> for Display {
    fn event(
        _state: &mut Self,
        _: &ZwpRelativePointerManagerV1,
        event: wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_manager_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        log::trace!("{event:?}");
    }
}

#[cfg(not(feature = "bindings"))]
#[cfg(unix)]
impl wayland_client::Dispatch<ZwpRelativePointerV1, ()> for Display {
    fn event(
        obj: &mut Self,
        _: &ZwpRelativePointerV1,
        event: wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        if let RelEvent::RelativeMotion {
            dx_unaccel,
            dy_unaccel,
            ..
        } = event
        {
            let scale = obj.scale_factor() as f64;
            let (dx, dy) = (dx_unaccel / scale, dy_unaccel / scale);
            obj.emit_by_name::<()>("motion-relative", &[&dx, &dy]);
        }
    }
}

#[cfg(not(feature = "bindings"))]
#[cfg(unix)]
impl wayland_client::Dispatch<ZwpPointerConstraintsV1, ()> for Display {
    fn event(
        _state: &mut Self,
        _: &ZwpPointerConstraintsV1,
        event: wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        log::trace!("{event:?}");
    }
}

#[cfg(not(feature = "bindings"))]
#[cfg(unix)]
impl wayland_client::Dispatch<ZwpLockedPointerV1, ()> for Display {
    fn event(
        _state: &mut Self,
        _: &ZwpLockedPointerV1,
        event: wayland_protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        log::trace!("{event:?}");
    }
}

/// cbindgen:ignore
pub const NONE_DISPLAY: Option<&Display> = None;

pub trait DisplayExt: 'static {
    fn display_size(&self) -> Option<(usize, usize)>;

    fn set_display_size(&self, size: Option<(usize, usize)>);

    fn define_cursor(&self, cursor: Option<gdk::Cursor>);

    fn mouse_absolute(&self) -> bool;

    fn set_mouse_absolute(&self, absolute: bool);

    fn set_cursor_position(&self, pos: Option<(usize, usize)>);

    fn grab_shortcut(&self) -> gtk::ShortcutTrigger;

    fn grabbed(&self) -> Grab;

    fn update_area(&self, x: i32, y: i32, w: i32, h: i32, stride: i32, data: &[u8]);

    #[cfg(unix)]
    fn set_dmabuf_scanout(&self, s: RdwDmabufScanout);

    fn render(&self);

    fn set_alternative_text(&self, alt_text: &str);

    fn connect_key_event<F: Fn(&Self, u32, u32, KeyEvent) + 'static>(
        &self,
        f: F,
    ) -> SignalHandlerId;

    fn connect_motion<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_motion_relative<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_mouse_press<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_mouse_release<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_scroll_discrete<F: Fn(&Self, Scroll) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_property_grabbed_notify<F: Fn(&Self) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_resize_request<F: Fn(&Self, u32, u32, u32, u32) + 'static>(
        &self,
        f: F,
    ) -> SignalHandlerId;
}

impl<O: IsA<Display> + IsA<gtk::Widget> + IsA<gtk::Accessible>> DisplayExt for O {
    fn display_size(&self) -> Option<(usize, usize)> {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            let (mut w, mut h) = (
                std::mem::MaybeUninit::uninit(),
                std::mem::MaybeUninit::uninit(),
            );
            ffi::rdw_display_get_display_size(
                self_.to_glib_none().0,
                w.as_mut_ptr(),
                h.as_mut_ptr(),
            )
            .then(|| (w.assume_init(), h.assume_init()))
        }
        #[cfg(not(feature = "bindings"))]
        {
            let imp = imp::Display::from_obj(self_);

            imp.display_size.get()
        }
    }

    fn set_display_size(&self, size: Option<(usize, usize)>) {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            let (w, h) = if let Some(size) = size {
                (size.0, size.1)
            } else {
                (0, 0)
            };
            ffi::rdw_display_set_display_size(self_.to_glib_none().0, w, h);
        }
        #[cfg(not(feature = "bindings"))]
        {
            let imp = imp::Display::from_obj(self_);

            if self.display_size() == size {
                return;
            }

            let _ctx = imp.make_current();
            if let Some((width, height)) = size {
                unsafe {
                    gl::BindTexture(gl::TEXTURE_2D, imp.texture_id());
                    gl::TexImage2D(
                        gl::TEXTURE_2D,
                        0,
                        gl::RGB as _,
                        width as _,
                        height as _,
                        0,
                        gl::BGRA,
                        gl::UNSIGNED_BYTE,
                        std::ptr::null(),
                    );
                }
            }

            imp.display_size.replace(size);
            self.queue_resize();
        }
    }

    fn define_cursor(&self, cursor: Option<gdk::Cursor>) {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            ffi::rdw_display_define_cursor(self_.to_glib_none().0, cursor.to_glib_none().0);
        }
        #[cfg(not(feature = "bindings"))]
        {
            let imp = imp::Display::from_obj(self_);
            if self.mouse_absolute() {
                imp.gl_area().set_cursor(cursor.as_ref());
            }
            imp.cursor.replace(cursor);
        }
    }

    fn mouse_absolute(&self) -> bool {
        self.property("mouse-absolute")
    }

    fn set_mouse_absolute(&self, absolute: bool) {
        glib::ObjectExt::set_property(self, "mouse-absolute", &absolute);
    }

    fn set_cursor_position(&self, pos: Option<(usize, usize)>) {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            let (x, y, enabled) = match pos {
                Some((x, y)) => (x, y, true),
                None => (0, 0, false),
            };
            ffi::rdw_display_set_cursor_position(self_.to_glib_none().0, enabled, x, y);
        }
        #[cfg(not(feature = "bindings"))]
        {
            let imp = imp::Display::from_obj(self_);

            imp.cursor_position.set(pos);
            self.queue_draw();
        }
    }

    fn grab_shortcut(&self) -> gtk::ShortcutTrigger {
        self.property("grab-shortcut")
    }

    fn grabbed(&self) -> Grab {
        self.property("grabbed")
    }

    fn update_area(&self, x: i32, y: i32, w: i32, h: i32, stride: i32, data: &[u8]) {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            ffi::rdw_display_update_area(self_.to_glib_none().0, x, y, w, h, stride, data.as_ptr());
        }
        #[cfg(not(feature = "bindings"))]
        {
            let imp = imp::Display::from_obj(self_);
            let _ctx = imp.make_current();

            // TODO: check data boundaries
            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, imp.texture_id());
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as _);
                gl::PixelStorei(gl::UNPACK_ROW_LENGTH, stride / 4);
                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    x,
                    y,
                    w,
                    h,
                    gl::BGRA,
                    gl::UNSIGNED_BYTE,
                    data.as_ptr() as _,
                );
            }

            #[cfg(unix)]
            imp.dmabuf.replace(None);
            imp.gl_area().queue_render();
        }
    }

    #[cfg(unix)]
    fn set_dmabuf_scanout(&self, s: RdwDmabufScanout) {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            ffi::rdw_display_set_dmabuf_scanout(self_.to_glib_none().0, &s);
        }
        #[cfg(all(unix, not(feature = "bindings")))]
        {
            let imp = imp::Display::from_obj(self_);
            let _ctx = imp.make_current();

            let egl = egl::egl();
            let egl_image_target = match egl::image_target_texture_2d_oes() {
                Some(func) => func,
                _ => {
                    log::warn!("ImageTargetTexture2DOES support missing");
                    return;
                }
            };

            let egl_dpy = match imp.egl_display() {
                Some(dpy) => dpy,
                None => {
                    log::warn!("Unsupported display kind (or not egl)");
                    return;
                }
            };

            let attribs = vec![
                egl::WIDTH as usize,
                s.width as usize,
                egl::HEIGHT as usize,
                s.height as usize,
                egl::LINUX_DRM_FOURCC_EXT as usize,
                s.fourcc as usize,
                egl::DMA_BUF_PLANE0_FD_EXT as usize,
                s.fd as usize,
                egl::DMA_BUF_PLANE0_PITCH_EXT as usize,
                s.stride as usize,
                egl::DMA_BUF_PLANE0_OFFSET_EXT as usize,
                0,
                egl::DMA_BUF_PLANE0_MODIFIER_LO_EXT as usize,
                (s.modifier & 0xffffffff) as usize,
                egl::DMA_BUF_PLANE0_MODIFIER_HI_EXT as usize,
                (s.modifier >> 32 & 0xffffffff) as usize,
                egl::NONE as usize,
            ];

            let img = match egl.create_image(
                egl_dpy,
                egl::no_context(),
                egl::LINUX_DMA_BUF_EXT,
                egl::no_client_buffer(),
                &attribs,
            ) {
                Ok(img) => img,
                Err(e) => {
                    log::warn!("eglCreateImage() failed: {}", e);
                    return;
                }
            };

            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, imp.texture_id());
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as _);
                egl_image_target(gl::TEXTURE_2D, img.as_ptr() as gl::types::GLeglImageOES);
            }

            imp.dmabuf.replace(Some(s));

            if let Err(e) = egl.destroy_image(egl_dpy, img) {
                log::warn!("eglDestroyImage() failed: {}", e);
            }
        }
    }

    fn render(&self) {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            ffi::rdw_display_render(self_.to_glib_none().0);
        }
        #[cfg(not(feature = "bindings"))]
        {
            let imp = imp::Display::from_obj(self_);
            let _ctx = imp.make_current();

            unsafe {
                gl::ClearColor(0.1, 0.1, 0.1, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::BLEND);

                if let Some(vp) = imp.viewport() {
                    gl::Viewport(vp.x(), vp.y(), vp.width(), vp.height());
                    #[cfg(not(unix))]
                    let flip = false;
                    #[cfg(unix)]
                    let flip = imp.dmabuf.borrow().as_ref().map_or(false, |d| d.y0_top);
                    imp.texture_blit(flip);
                }
            }

            imp.gl_area().queue_draw();
        }
    }

    fn set_alternative_text(&self, alt_text: &str) {
        self.update_property(&[gtk::accessible::Property::Description(alt_text)]);
    }

    fn connect_key_event<F: Fn(&Self, u32, u32, KeyEvent) + 'static>(
        &self,
        f: F,
    ) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, u32, u32, KeyEvent) + 'static>(
            this: *mut RdwDisplay,
            keyval: u32,
            keycode: u32,
            event: KeyEvent,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                keyval,
                keycode,
                event,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"key-event\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_motion<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, f64, f64) + 'static>(
            this: *mut RdwDisplay,
            x: f64,
            y: f64,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(Display::from_glib_borrow(this).unsafe_cast_ref::<P>(), x, y)
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"motion\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_motion_relative<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, f64, f64) + 'static>(
            this: *mut RdwDisplay,
            dx: f64,
            dy: f64,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                dx,
                dy,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"motion-relative\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_mouse_press<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, u32) + 'static>(
            this: *mut RdwDisplay,
            button: u32,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                button,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"mouse-press\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_mouse_release<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, u32) + 'static>(
            this: *mut RdwDisplay,
            button: u32,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                button,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"mouse-release\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_scroll_discrete<F: Fn(&Self, Scroll) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, Scroll) + 'static>(
            this: *mut RdwDisplay,
            scroll: Scroll,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                scroll,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"scroll-discrete\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_property_grabbed_notify<F: Fn(&Self) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn notify_trampoline<P, F: Fn(&P) + 'static>(
            this: *mut RdwDisplay,
            _param_spec: glib::ffi::gpointer,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f: &F = &*(f as *const F);
            f(Display::from_glib_borrow(this).unsafe_cast_ref())
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut _,
                b"notify::grabbed\0".as_ptr() as *const _,
                Some(std::mem::transmute::<_, unsafe extern "C" fn()>(
                    notify_trampoline::<Self, F> as *const (),
                )),
                Box::into_raw(f),
            )
        }
    }
    fn connect_resize_request<F: Fn(&Self, u32, u32, u32, u32) + 'static>(
        &self,
        f: F,
    ) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, u32, u32, u32, u32) + 'static>(
            this: *mut RdwDisplay,
            width: u32,
            height: u32,
            width_mm: u32,
            height_mm: u32,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                width,
                height,
                width_mm,
                height_mm,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"resize-request\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }
}

pub trait DisplayImpl: DisplayImplExt + WidgetImpl {}

pub trait DisplayImplExt: ObjectSubclass {}

impl<T: DisplayImpl> DisplayImplExt for T {}

unsafe impl<T: DisplayImpl> IsSubclassable<T> for Display {
    fn class_init(class: &mut glib::Class<Self>) {
        <gtk::Widget as IsSubclassable<T>>::class_init(class);
    }

    fn instance_init(instance: &mut glib::subclass::InitializingObject<T>) {
        <gtk::Widget as IsSubclassable<T>>::instance_init(instance);
    }
}

#[cfg(not(feature = "bindings"))]
glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// cbindgen:ignore
#[cfg(feature = "bindings")]
mod ffi {
    use super::*;

    extern "C" {
        pub fn rdw_display_get_type() -> glib::ffi::GType;

        pub fn rdw_display_get_display_size(
            dpy: *mut RdwDisplay,
            width: *mut usize,
            height: *mut usize,
        ) -> bool;

        pub fn rdw_display_set_display_size(dpy: *mut RdwDisplay, width: usize, height: usize);

        pub fn rdw_display_define_cursor(dpy: *mut RdwDisplay, cursor: *const gdk::ffi::GdkCursor);

        pub fn rdw_display_set_cursor_position(
            dpy: *mut RdwDisplay,
            enabled: bool,
            x: usize,
            y: usize,
        );

        pub fn rdw_display_update_area(
            dpy: *mut RdwDisplay,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            stride: i32,
            data: *const u8,
        );

        pub fn rdw_display_render(dpy: *mut RdwDisplay);

        #[cfg(unix)]
        pub fn rdw_display_set_dmabuf_scanout(
            dpy: *mut RdwDisplay,
            dmabuf: *const RdwDmabufScanout,
        );
    }
}

#[cfg(feature = "bindings")]
glib::wrapper! {
    pub struct Display(Object<RdwDisplay, RdwDisplayClass>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;

    match fn {
        type_ => || ffi::rdw_display_get_type(),
    }
}
