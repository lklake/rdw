use gdk_wl::prelude::*;
use glib::{signal::SignalHandlerId, subclass::prelude::*, translate::*};
use gtk::{gdk, glib, prelude::*, subclass::prelude::WidgetImpl};

use crate::{Grab, KeyEvent, RdwDmabufScanout, Scroll};

#[cfg(not(feature = "bindings"))]
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
    use crate::{error::Error, util};
    use gdk_wl::wayland_client::{self, GlobalManager};
    use gl::types::*;
    use glib::{clone, subclass::Signal, SourceId};
    use gtk::{graphene, subclass::prelude::*};
    use once_cell::sync::{Lazy, OnceCell};
    use std::{
        cell::{Cell, RefCell},
        time::Duration,
    };
    use wayland_protocols::unstable::{
        pointer_constraints::v1::client::{
            zwp_locked_pointer_v1::ZwpLockedPointerV1,
            zwp_pointer_constraints_v1::{Lifetime, ZwpPointerConstraintsV1},
        },
        relative_pointer::v1::client::{
            zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
            zwp_relative_pointer_v1::{Event as RelEvent, ZwpRelativePointerV1},
        },
    };
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
        pub(crate) last_resize_request: Cell<Option<(u32, u32, (u32, u32))>>,
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

        // Option, because None means failed to init
        pub(crate) egl_ctx: OnceCell<egl::Context>,
        pub(crate) egl_cfg: OnceCell<egl::Config>,
        pub(crate) egl_surf: OnceCell<egl::Surface>,

        pub(crate) texture_id: Cell<GLuint>,
        pub(crate) texture_blit_vao: Cell<GLuint>,
        pub(crate) texture_blit_prog: Cell<GLuint>,
        pub(crate) texture_blit_flip_prog: Cell<GLuint>,
        pub(crate) dmabuf: RefCell<Option<RdwDmabufScanout>>,

        pub(crate) wl_source: Cell<Option<glib::SourceId>>,
        pub(crate) wl_rel_manager: OnceCell<wayland_client::Main<ZwpRelativePointerManagerV1>>,
        pub(crate) wl_rel_pointer: RefCell<Option<wayland_client::Main<ZwpRelativePointerV1>>>,
        pub(crate) wl_pointer_constraints: OnceCell<wayland_client::Main<ZwpPointerConstraintsV1>>,
        pub(crate) wl_lock_pointer: RefCell<Option<wayland_client::Main<ZwpLockedPointerV1>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwDisplay";
        type Type = super::Display;
        type ParentType = gtk::Widget;
        type Class = RdwDisplayClass;
        type Instance = RdwDisplay;

        fn class_init(_klass: &mut Self::Class) {
            // Assume EGL for now, done at class init time but could be done lazily?
            let egl = egl::egl();

            gl::load_with(|s| {
                egl.get_proc_address(s)
                    .map(|f| f as _)
                    .unwrap_or(std::ptr::null())
            });
        }
    }

    impl ObjectImpl for Display {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);
            self.layout_manager.set(gtk::BinLayout::new()).unwrap();

            let gl_area = gtk::GLArea::new();
            gl_area.set_has_depth_buffer(false);
            gl_area.set_has_stencil_buffer(false);
            gl_area.set_auto_render(false);
            gl_area.set_required_version(3, 2);
            gl_area.connect_render(
                clone!(@weak obj => @default-return glib::signal::Inhibit(true), move |_, _| {
                    obj.render();
                    glib::signal::Inhibit(true)
                }),
            );
            gl_area.connect_realize(clone!(@weak obj => move |_| {
                let imp = Self::from_instance(&obj);
                if let Err(e) = unsafe { imp.realize_gl() } {
                    log::warn!("Failed to realize gl: {}", e);
                    let e = glib::Error::new(Error::GL, &e);
                    imp.gl_area().set_error(Some(&e));
                }
            }));

            self.gl_area.set(gl_area).unwrap();

            self.grab_shortcut.get_or_init(|| {
                gtk::ShortcutTrigger::parse_string("<Ctrl>Alt_L|<Alt>Control_L").unwrap()
            });
        }

        fn dispose(&self, obj: &Self::Type) {
            if let Some(source) = self.wl_source.take() {
                source.remove();
            }
            while let Some(child) = obj.first_child() {
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

        fn set_property(
            &self,
            _obj: &Self::Type,
            _id: usize,
            value: &glib::Value,
            pspec: &glib::ParamSpec,
        ) {
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

        fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
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
                    Signal::builder(
                        "key-event",
                        &[
                            u32::static_type().into(),
                            u32::static_type().into(),
                            KeyEvent::static_type().into(),
                        ],
                        <()>::static_type().into(),
                    )
                    .build(),
                    Signal::builder(
                        "motion",
                        &[f64::static_type().into(), f64::static_type().into()],
                        <()>::static_type().into(),
                    )
                    .build(),
                    Signal::builder(
                        "motion-relative",
                        &[f64::static_type().into(), f64::static_type().into()],
                        <()>::static_type().into(),
                    )
                    .build(),
                    Signal::builder(
                        "mouse-press",
                        &[u32::static_type().into()],
                        <()>::static_type().into(),
                    )
                    .build(),
                    Signal::builder(
                        "mouse-release",
                        &[u32::static_type().into()],
                        <()>::static_type().into(),
                    )
                    .build(),
                    Signal::builder(
                        "scroll-discrete",
                        &[Scroll::static_type().into()],
                        <()>::static_type().into(),
                    )
                    .build(),
                    Signal::builder(
                        "resize-request",
                        &[
                            u32::static_type().into(),
                            u32::static_type().into(),
                            u32::static_type().into(),
                            u32::static_type().into(),
                        ],
                        <()>::static_type().into(),
                    )
                    .build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for Display {
        fn realize(&self, widget: &Self::Type) {
            self.parent_realize(widget);

            widget.set_sensitive(true);
            widget.set_focusable(true);
            widget.set_focus_on_click(true);

            if self.realize_egl() {
                if let Err(e) = unsafe { self.realize_gl() } {
                    log::warn!("Failed to realize GL: {}", e);
                }
            } else {
                self.gl_area().set_parent(widget);
            }

            if let Ok(dpy) = widget.display().downcast::<gdk_wl::WaylandDisplay>() {
                self.realize_wl(&dpy);
            }

            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            widget.add_controller(&ec);
            ec.connect_key_pressed(
                clone!(@weak widget => @default-panic, move |ec, keyval, keycode, _state| {
                    let imp = Self::from_instance(&widget);
                    imp.key_pressed(ec, keyval, keycode);
                    glib::signal::Inhibit(true)
                }),
            );
            ec.connect_key_released(clone!(@weak widget => move |_, keyval, keycode, _state| {
                let imp = Self::from_instance(&widget);
                imp.key_released(keyval, keycode);
            }));

            let ec = gtk::EventControllerMotion::new();
            widget.add_controller(&ec);
            ec.connect_motion(clone!(@weak widget => move |_, x, y| {
                let imp = Self::from_instance(&widget);
                if let Some((x, y)) = imp.transform_pos(x, y) {
                    widget.emit_by_name::<()>("motion", &[&x, &y]);
                }
            }));
            ec.connect_enter(clone!(@weak widget => move |_, x, y| {
                let imp = Self::from_instance(&widget);
                if let Some((x, y)) = imp.transform_pos(x, y) {
                    widget.emit_by_name::<()>("motion", &[&x, &y]);
                }
            }));
            ec.connect_leave(clone!(@weak widget => move |_| {
                let imp = Self::from_instance(&widget);
                imp.ungrab_keyboard();
            }));

            let ec = gtk::GestureClick::new();
            ec.set_button(0);
            widget.add_controller(&ec);
            ec.connect_pressed(
                clone!(@weak widget => @default-panic, move |gesture, _n_press, x, y| {
                    let imp = Self::from_instance(&widget);

                    imp.try_grab();

                    let button = gesture.current_button();
                    if let Some((x, y)) = imp.transform_pos(x, y) {
                        widget.emit_by_name::<()>("motion", &[&x, &y]);
                    }
                    widget.emit_by_name::<()>("mouse-press", &[&button]);
                }),
            );
            ec.connect_released(clone!(@weak widget => move |gesture, _n_press, x, y| {
                let imp = Self::from_instance(&widget);
                let button = gesture.current_button();
                if let Some((x, y)) = imp.transform_pos(x, y) {
                    widget.emit_by_name::<()>("motion", &[&x, &y]);
                }
                widget.emit_by_name::<()>("mouse-release", &[&button]);
            }));

            let ec = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::BOTH_AXES
                    | gtk::EventControllerScrollFlags::DISCRETE,
            );
            widget.add_controller(&ec);
            ec.connect_scroll(clone!(@weak widget => @default-panic, move |_, dx, dy| {
                if dy >= 1.0 {
                    widget.emit_by_name::<()>("scroll-discrete", &[&Scroll::Down]);
                } else if dy <= -1.0 {
                    widget.emit_by_name::<()>("scroll-discrete", &[&Scroll::Up]);
                }
                if dx >= 1.0 {
                    widget.emit_by_name::<()>("scroll-discrete", &[&Scroll::Right]);
                } else if dx <= -1.0 {
                    widget.emit_by_name::<()>("scroll-discrete", &[&Scroll::Left]);
                }
                glib::signal::Inhibit(false)
            }));
        }

        fn measure(
            &self,
            _widget: &Self::Type,
            orientation: gtk::Orientation,
            _for_size: i32,
        ) -> (i32, i32, i32, i32) {
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

        fn size_allocate(&self, widget: &Self::Type, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(widget, width, height, baseline);
            self.layout_manager
                .get()
                .unwrap()
                .allocate(widget, width, height, baseline);

            if let Some(timeout_id) = self.resize_timeout_id.take() {
                timeout_id.remove();
            }
            self.resize_timeout_id.set(Some(glib::timeout_add_local(
                Duration::from_millis(500),
                clone!(@weak widget => @default-return glib::Continue(false), move || {
                    let imp = Self::from_instance(&widget);
                    let sf = widget.scale_factor() as u32;
                    let width = width as u32 * sf;
                    let height = height as u32 * sf;
                    let mm = imp.surface()
                                   .as_ref()
                                   .and_then(|s| Some(gdk::traits::DisplayExt::monitor_at_surface(&widget.display(), s)))
                                   .map(|m| {
                                       let (geom, wmm, hmm) = (m.geometry(), m.width_mm() as u32, m.height_mm() as u32);
                                       (wmm * width / (geom.width() as u32), hmm * height / geom.height() as u32)
                                   }).unwrap_or((0u32, 0u32));
                    if Some((width, height, mm)) != imp.last_resize_request.get() {
                        imp.last_resize_request.set(Some((width, height, mm)));
                        widget.emit_by_name::<()>("resize-request", &[&width, &height, &mm.0, &mm.1]);
                    }
                    imp.resize_timeout_id.set(None);
                    glib::Continue(false)
                }),
            )));
        }

        fn snapshot(&self, widget: &Self::Type, snapshot: &gtk::Snapshot) {
            snapshot.save();
            self.parent_snapshot(widget, snapshot);
            snapshot.restore();

            if widget.mouse_absolute() {
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
                            let sf = widget.scale_factor();

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
            if let (Some(dpy), Some(_)) = (self.egl_display(), self.egl_surface()) {
                let _ = egl::egl().make_current(dpy, None, None, None);
            }
        }

        pub(crate) fn make_current(&self) -> ContextGuard {
            if let (Some(dpy), surf, Some(ctx)) =
                (self.egl_display(), self.egl_surface(), self.egl_context())
            {
                gdk::GLContext::clear_current();
                if let Err(e) = egl::egl().make_current(dpy, surf, surf, Some(ctx)) {
                    log::warn!("Failed to make current context: {}", e);
                }
            } else {
                let area = self.gl_area();
                area.make_current();
                area.attach_buffers();
            }
            ContextGuard(self)
        }

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

        fn realize_wl(&self, dpy: &gdk_wl::WaylandDisplay) {
            let display = dpy.wl_display();
            let mut event_queue = display.create_event_queue();
            let attached_display = display.attach(event_queue.token());
            let globals = GlobalManager::new(&attached_display);
            event_queue
                .sync_roundtrip(&mut (), |_, _, _| unreachable!())
                .unwrap();
            let rel_manager = globals
                .instantiate_exact::<ZwpRelativePointerManagerV1>(1)
                .unwrap();
            self.wl_rel_manager.set(rel_manager).unwrap();
            let pointer_constraints = globals
                .instantiate_exact::<ZwpPointerConstraintsV1>(1)
                .unwrap();
            self.wl_pointer_constraints
                .set(pointer_constraints)
                .unwrap();
            let fd = display.get_connection_fd();
            // I can't find a better way to hook it to gdk...
            let source = glib::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, _| {
                event_queue
                    .sync_roundtrip(&mut (), |_, _, _| unreachable!())
                    .unwrap();
                glib::Continue(true)
            });
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
            let display = self.instance();

            if !self.grabbed.get().contains(Grab::KEYBOARD) {
                return;
            }

            glib::idle_add_local(clone!(@strong display => @default-panic, move || {
                let imp = Self::from_instance(&display);
                if let Some(ec) = imp.grab_ec.upgrade() {
                    display.remove_controller(&ec);
                    imp.grab_ec.set(None);
                }
                if let Some(toplevel) = imp.toplevel() {
                    toplevel.restore_system_shortcuts();
                    imp.grabbed.set(imp.grabbed.get() - Grab::KEYBOARD);
                    display.notify("grabbed");
                }
                glib::Continue(false)
            }));
        }

        pub(crate) fn ungrab_mouse(&self) {
            let display = self.instance();

            if self.grabbed.get().contains(Grab::MOUSE) {
                if let Some(lock) = self.wl_lock_pointer.take() {
                    lock.destroy();
                }
                if let Some(rel_pointer) = self.wl_rel_pointer.take() {
                    rel_pointer.destroy();
                }
                self.grabbed.set(self.grabbed.get() - Grab::MOUSE);
                if !display.mouse_absolute() {
                    self.gl_area().set_cursor(None);
                }
                display.queue_draw(); // update cursor
                display.notify("grabbed");
            }
        }

        fn emit_last_key_press(&self) {
            let display = self.instance();

            if let Some((keyval, keycode)) = self.last_key_press.take() {
                display.emit_by_name::<()>("key-event", &[&keyval, &keycode, &KeyEvent::PRESS]);
            }

            if let Some(timeout_id) = self.last_key_press_timeout.take() {
                timeout_id.remove();
            }
        }

        fn key_pressed(&self, ec: &gtk::EventControllerKey, keyval: gdk::Key, keycode: u32) {
            let display = self.instance();

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
                    glib::clone!(@weak display => @default-return glib::Continue(false), move || {
                        let imp = Self::from_instance(&display);
                        imp.emit_last_key_press();
                        glib::Continue(false)
                    }),
                )));
        }

        fn key_released(&self, keyval: gdk::Key, keycode: u32) {
            let display = self.instance();

            if let Some((last_keyval, last_keycode)) = self.last_key_press.get() {
                if (last_keyval, last_keycode) == (keyval.into_glib(), keycode) {
                    self.last_key_press.set(None);
                    if let Some(timeout_id) = self.last_key_press_timeout.take() {
                        timeout_id.remove();
                    }

                    display.emit_by_name::<()>(
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

            display.emit_by_name::<()>(
                "key-event",
                &[&keyval.into_glib(), &keycode, &KeyEvent::RELEASE],
            )
        }

        fn try_grab_keyboard(&self) -> bool {
            if self.grabbed.get().contains(Grab::KEYBOARD) {
                return false;
            }

            let obj = self.instance();
            let toplevel = match self.toplevel() {
                Some(toplevel) => toplevel,
                _ => return false,
            };

            toplevel.inhibit_system_shortcuts(None::<&gdk::ButtonEvent>);
            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            ec.connect_key_pressed(clone!(@weak obj, @weak toplevel => @default-panic, move |ec, keyval, keycode, _state| {
                let imp = Self::from_instance(&obj);
                imp.key_pressed(ec, keyval, keycode);
                glib::signal::Inhibit(true)
            }));
            ec.connect_key_released(
                clone!(@weak obj => @default-panic, move |_ec, keyval, keycode, _state| {
                    let imp = Self::from_instance(&obj);
                    imp.key_released(keyval, keycode);
                }),
            );
            if let Some(root) = obj.root() {
                root.add_controller(&ec);
            }
            self.grab_ec.set(Some(&ec));

            let id = toplevel.connect_shortcuts_inhibited_notify(
                clone!(@weak obj => @default-panic, move |toplevel| {
                    let inhibited = toplevel.is_shortcuts_inhibited();
                    log::debug!("shortcuts-inhibited: {}", inhibited);
                    if !inhibited {
                        let imp = Self::from_instance(&obj);
                        let id = imp.shortcuts_inhibited_id.take();
                        toplevel.disconnect(id.unwrap());
                        imp.ungrab_keyboard();
                    }
                }),
            );
            self.shortcuts_inhibited_id.set(Some(id));
            true
        }

        fn try_grab_device(&self, device: gdk::Device) -> bool {
            let device = match device.downcast::<gdk_wl::WaylandDevice>() {
                Ok(device) => device,
                _ => return false,
            };
            let pointer = device.wl_pointer();
            let obj = self.instance();

            if self.wl_lock_pointer.borrow().is_none() {
                if let Some(constraints) = self.wl_pointer_constraints.get() {
                    if let Some(surf) = self.wl_surface() {
                        let lock = constraints.lock_pointer(
                            &surf,
                            &pointer,
                            None,
                            Lifetime::Persistent as _,
                        );
                        lock.quick_assign(move |_, event, _| {
                            log::debug!("wl lock pointer event: {:?}", event);
                        });
                        self.wl_lock_pointer.replace(Some(lock));
                    }
                }
            }

            if self.wl_rel_pointer.borrow().is_none() {
                if let Some(rel_manager) = self.wl_rel_manager.get() {
                    let rel_pointer = rel_manager.get_relative_pointer(&pointer);
                    rel_pointer.quick_assign(
                        clone!(@weak obj => @default-panic, move |_, event, _| {
                            if let RelEvent::RelativeMotion { dx_unaccel, dy_unaccel, .. } = event {
                                let scale = obj.scale_factor() as f64;
                                let (dx, dy) = (dx_unaccel / scale, dy_unaccel / scale);
                                obj.emit_by_name::<()>("motion-relative", &[&dx, &dy]);
                            }
                        }),
                    );
                    self.wl_rel_pointer.replace(Some(rel_pointer));
                }
            }

            true
        }

        fn try_grab_mouse(&self) -> bool {
            let obj = self.instance();
            if obj.mouse_absolute() {
                // we could eventually grab the mouse in client mode, but what's the point?
                return false;
            }
            if obj.grabbed().contains(Grab::MOUSE) {
                return false;
            }

            if let Some(default_seat) = gdk::traits::DisplayExt::default_seat(&obj.display()) {
                for device in default_seat.devices(gdk::SeatCapabilities::POINTER) {
                    self.try_grab_device(device);
                }
            }

            true
        }

        fn try_grab(&self) {
            let display = self.instance();
            let mut grabbed = display.grabbed();
            if self.try_grab_keyboard() {
                grabbed |= Grab::KEYBOARD;
            }
            if self.try_grab_mouse() {
                grabbed |= Grab::MOUSE;
                if !display.mouse_absolute() {
                    // hide client mouse
                    self.gl_area().set_cursor_from_name(Some("none"));
                }
                display.queue_draw(); // update cursor
            }
            self.grabbed.set(grabbed);
            display.notify("grabbed");
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
            let display = self.instance();
            let (dw, dh) = match display.display_size() {
                Some(size) => size,
                None => return (0, 0),
            };
            let sf = display.scale_factor();
            let (w, h) = (display.width() * sf, display.height() * sf);
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
            let display = self.instance();
            display.display_size()?;

            let sf = display.scale_factor();
            let (w, h) = (display.width() * sf, display.height() * sf);
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
            let display = self.instance();
            let sf = display.scale_factor() as f64;
            self.viewport().and_then(|vp| {
                let (x, y) = (x * sf, y * sf);
                if !vp.contains_point(x as _, y as _) {
                    return None;
                }
                let (sw, sh) = display.display_size().unwrap();
                let x = (x - vp.x() as f64) * (sw as f64 / vp.width() as f64);
                let y = (y - vp.y() as f64) * (sh as f64 / vp.height() as f64);
                Some((x, y))
            })
        }

        // remote display pos -> widget pos
        fn transform_pos_inv(&self, x: f64, y: f64) -> Option<(f64, f64)> {
            let display = self.instance();
            let sf = display.scale_factor() as f64;
            self.viewport().map(|vp| {
                let (sw, sh) = display.display_size().unwrap();
                let x = x * (vp.width() as f64 / sw as f64) + vp.x() as f64;
                let y = y * (vp.height() as f64 / sh as f64) + vp.y() as f64;
                (x / sf as f64, y / sf as f64)
            })
        }

        fn toplevel(&self) -> Option<gdk::Toplevel> {
            let display = self.instance();
            display
                .root()
                .and_then(|r| r.native())
                .map(|n| n.surface())
                .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
        }

        fn surface(&self) -> Option<gdk::Surface> {
            let display = self.instance();
            display.native().map(|n| n.surface())
        }

        fn egl_surface(&self) -> Option<egl::Surface> {
            self.egl_surf.get().copied()
        }

        fn wl_surface(&self) -> Option<wayland_client::protocol::wl_surface::WlSurface> {
            self.surface()
                .and_then(|s| s.downcast::<gdk_wl::WaylandSurface>().ok())
                .map(|w| w.wl_surface())
        }

        fn x11_xid(&self) -> Option<xlib::Window> {
            self.surface()
                .and_then(|s| s.downcast::<gdk_x11::X11Surface>().ok())
                .map(|s| s.xid())
        }

        pub(crate) fn egl_context(&self) -> Option<egl::Context> {
            self.egl_display().and_then(|_| self.egl_ctx.get().copied())
        }

        pub(crate) fn egl_display(&self) -> Option<egl::Display> {
            let widget = self.instance();

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
            let imp = imp::Display::from_instance(self_);

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
            let imp = imp::Display::from_instance(self_);

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
            let imp = imp::Display::from_instance(self_);
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
            let imp = imp::Display::from_instance(self_);

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
            let imp = imp::Display::from_instance(self_);
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

            imp.dmabuf.replace(None);
            imp.gl_area().queue_render();
        }
    }

    fn set_dmabuf_scanout(&self, s: RdwDmabufScanout) {
        // Safety: safe because IsA<Display>
        let self_: &Display = unsafe { self.unsafe_cast_ref::<Display>() };

        #[cfg(feature = "bindings")]
        unsafe {
            ffi::rdw_display_set_dmabuf_scanout(self_.to_glib_none().0, &s);
        }
        #[cfg(not(feature = "bindings"))]
        {
            let imp = imp::Display::from_instance(self_);
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
            let imp = imp::Display::from_instance(self_);
            let _ctx = imp.make_current();

            unsafe {
                gl::ClearColor(0.1, 0.1, 0.1, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::BLEND);

                if let Some(vp) = imp.viewport() {
                    gl::Viewport(vp.x(), vp.y(), vp.width(), vp.height());
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
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
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
            f(
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                x,
                y,
            )
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
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
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
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
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
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
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
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
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
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
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
