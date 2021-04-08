use gl::types::*;
use glib::{
    clone,
    signal::SignalHandlerId,
    subclass::prelude::*,
    translate::{FromGlibPtrBorrow, ToGlib},
};
use gtk::{gdk, glib, graphene, prelude::*, subclass::prelude::GLAreaImpl};
use std::cell::Cell;

use wayland_client::{Display as WlDisplay, GlobalManager};
use wayland_protocols::unstable::pointer_constraints::v1::client::zwp_locked_pointer_v1::ZwpLockedPointerV1;
use wayland_protocols::unstable::pointer_constraints::v1::client::zwp_pointer_constraints_v1::{
    Lifetime, ZwpPointerConstraintsV1,
};
use wayland_protocols::unstable::relative_pointer::v1::client::zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1;
use wayland_protocols::unstable::relative_pointer::v1::client::zwp_relative_pointer_v1::{
    Event as RelEvent, ZwpRelativePointerV1,
};

use crate::{egl, error::Error, util, Grab, Scroll};

pub mod imp {
    use std::cell::RefCell;

    use super::*;
    use glib::subclass::Signal;
    use gtk::subclass::prelude::*;
    use once_cell::sync::{Lazy, OnceCell};

    #[repr(C)]
    pub struct RdwDisplayClass {
        pub parent_class: gtk::ffi::GtkGLAreaClass,
    }

    unsafe impl ClassStruct for RdwDisplayClass {
        type Type = Display;
    }

    #[repr(C)]
    pub struct RdwDisplay {
        parent: gtk::ffi::GtkGLArea,
    }

    impl std::fmt::Debug for RdwDisplay {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwDisplay")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwDisplay {
        type Type = Display;
    }

    #[derive(Default)]
    pub struct Display {
        // The remote display size, ex: 1024x768
        pub display_size: Cell<Option<(u32, u32)>>,
        // The currently defined cursor
        pub cursor: RefCell<Option<gdk::Cursor>>,
        pub mouse_absolute: Cell<bool>,
        // position of cursor when drawn by client
        pub cursor_position: Cell<Option<(u32, u32)>>,

        // the shortcut to ungrab key/mouse (to be configurable and extended with ctrl-alt)
        pub grab_shortcut: OnceCell<gtk::ShortcutTrigger>,
        pub grabbed: Cell<Grab>,
        pub shortcuts_inhibited_id: Cell<Option<SignalHandlerId>>,

        pub texture_id: Cell<GLuint>,
        pub texture_blit_vao: Cell<GLuint>,
        pub texture_blit_prog: Cell<GLuint>,
        pub texture_blit_flip_prog: Cell<GLuint>,

        pub wl_source: Cell<Option<glib::SourceId>>,
        pub wl_event_queue: OnceCell<wayland_client::EventQueue>,
        pub wl_rel_manager: OnceCell<wayland_client::Main<ZwpRelativePointerManagerV1>>,
        pub wl_rel_pointer: RefCell<Option<wayland_client::Main<ZwpRelativePointerV1>>>,
        pub wl_pointer_constraints: OnceCell<wayland_client::Main<ZwpPointerConstraintsV1>>,
        pub wl_lock_pointer: RefCell<Option<wayland_client::Main<ZwpLockedPointerV1>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwDisplay";
        type Type = super::Display;
        type ParentType = gtk::GLArea;
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
            self.grab_shortcut
                .get_or_init(|| gtk::ShortcutTrigger::parse_string("<ctrl><alt>g").unwrap());
        }

        fn dispose(&self, _obj: &Self::Type) {
            if let Some(source) = self.wl_source.take() {
                glib::source_remove(source);
            }
        }

        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![glib::ParamSpec::flags(
                    "grabbed",
                    "grabbed",
                    "Grabbed",
                    Grab::static_type(),
                    Grab::empty().to_glib(),
                    glib::ParamFlags::READABLE,
                )]
            });
            PROPERTIES.as_ref()
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![
                    Signal::builder(
                        "key-press",
                        &[u32::static_type().into(), u32::static_type().into()],
                        <()>::static_type().into(),
                    )
                    .build(),
                    Signal::builder(
                        "key-release",
                        &[u32::static_type().into(), u32::static_type().into()],
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
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for Display {
        fn realize(&self, widget: &Self::Type) {
            widget.set_has_depth_buffer(false);
            widget.set_has_stencil_buffer(false);
            widget.set_auto_render(false);
            widget.set_required_version(3, 2);
            widget.set_sensitive(true);
            widget.set_focusable(true);
            widget.set_focus_on_click(true);

            self.parent_realize(widget);
            widget.make_current();

            if let Err(e) = unsafe { self.realize_gl() } {
                let e = glib::Error::new(Error::GL, &e);
                widget.set_error(Some(&e));
            }

            if let Ok(dpy) = widget.get_display().downcast::<gdk_wl::WaylandDisplay>() {
                self.realize_wl(&dpy);
            }

            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            widget.add_controller(&ec);
            ec.connect_key_pressed(
                clone!(@weak widget => @default-panic, move |ec, keyval, keycode, _state| {
                    let self_ = Self::from_instance(&widget);
                    self_.key_pressed(ec, keyval, keycode);
                    glib::signal::Inhibit(true)
                }),
            );
            ec.connect_key_released(clone!(@weak widget => move |_, keyval, keycode, _state| {
                let self_ = Self::from_instance(&widget);
                self_.key_released(keyval, keycode);
            }));

            let ec = gtk::EventControllerMotion::new();
            widget.add_controller(&ec);
            ec.connect_motion(clone!(@weak widget => move |_, x, y| {
                let self_ = Self::from_instance(&widget);
                if let Some((x, y)) = self_.transform_input(x, y) {
                    widget.emit_by_name("motion", &[&x, &y]).unwrap();
                }
            }));

            let ec = gtk::GestureClick::new();
            ec.set_button(0);
            widget.add_controller(&ec);
            ec.connect_pressed(
                clone!(@weak widget => @default-panic, move |gesture, _n_press, x, y| {
                    let self_ = Self::from_instance(&widget);

                    self_.try_grab();

                    let button = gesture.get_current_button();
                    if let Some((x, y)) = self_.transform_input(x, y) {
                        widget.emit_by_name("motion", &[&x, &y]).unwrap();
                    }
                    widget.emit_by_name("mouse-press", &[&button]).unwrap();
                }),
            );
            ec.connect_released(clone!(@weak widget => move |gesture, _n_press, x, y| {
                let self_ = Self::from_instance(&widget);
                let button = gesture.get_current_button();
                if let Some((x, y)) = self_.transform_input(x, y) {
                    widget.emit_by_name("motion", &[&x, &y]).unwrap();
                }
                widget.emit_by_name("mouse-release", &[&button]).unwrap();
            }));

            let ec = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::BOTH_AXES
                    | gtk::EventControllerScrollFlags::DISCRETE,
            );
            widget.add_controller(&ec);
            ec.connect_scroll(clone!(@weak widget => @default-panic, move |_, dx, dy| {
                if dy >= 1.0 {
                    widget.emit_by_name("scroll-discrete", &[&Scroll::Down]).unwrap();
                } else if dy <= -1.0 {
                    widget.emit_by_name("scroll-discrete", &[&Scroll::Up]).unwrap();
                }
                if dx >= 1.0 {
                    widget.emit_by_name("scroll-discrete", &[&Scroll::Right]).unwrap();
                } else if dx <= -1.0 {
                    widget.emit_by_name("scroll-discrete", &[&Scroll::Left]).unwrap();
                }
                glib::signal::Inhibit(false)
            }));
        }

        fn snapshot(&self, widget: &Self::Type, snapshot: &gtk::Snapshot) {
            self.parent_snapshot(widget, snapshot);

            if !self.mouse_absolute.get() {
                if let Some(pos) = self.cursor_position.get() {
                    if let Some(cursor) = &*self.cursor.borrow() {
                        if let Some(texture) = cursor.get_texture() {
                            let sf = widget.get_scale_factor();
                            snapshot.append_texture(
                                &texture,
                                &graphene::Rect::new(
                                    (pos.0 as i32 - cursor.get_hotspot_x() / sf) as f32,
                                    (pos.0 as i32 - cursor.get_hotspot_y() / sf) as f32,
                                    (texture.get_width() / sf) as f32,
                                    (texture.get_height() / sf) as f32,
                                ),
                            )
                        }
                    }
                }
            }
        }
    }

    impl GLAreaImpl for Display {
        fn render(&self, _gl_area: &Self::Type, _context: &gdk::GLContext) -> bool {
            unsafe {
                gl::ClearColor(0.1, 0.1, 0.1, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::BLEND);

                if let Some(vp) = self.viewport() {
                    gl::Viewport(vp.x, vp.y, vp.width, vp.height);
                    self.texture_blit(false);
                }
            }
            false
        }
    }

    impl Display {
        fn realize_wl(&self, dpy: &gdk_wl::WaylandDisplay) {
            let display = unsafe {
                WlDisplay::from_external_display(dpy.get_wl_display().as_ref().c_ptr() as *mut _)
            };
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

        unsafe fn realize_gl(&self) -> Result<(), String> {
            use std::ffi::CString;

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

        fn ungrab_keyboard(&self, ec: &gtk::EventControllerKey) {
            let display = self.get_instance();

            if self.grabbed.get().contains(Grab::KEYBOARD) {
                //display.remove_controller(ec); here crashes badly
                glib::idle_add_local(clone!(@weak ec, @weak display => @default-panic, move || {
                    let self_ = Self::from_instance(&display);
                    if let Some(widget) = ec.get_widget() {
                        widget.remove_controller(&ec);
                    }
                    if let Some(toplevel) = self_.get_toplevel() {
                        // will also notify the grab change
                        toplevel.restore_system_shortcuts();
                    }
                    glib::Continue(false)
                }));
            }
        }

        pub(crate) fn ungrab_mouse(&self) {
            let display = self.get_instance();

            if self.grabbed.get().contains(Grab::MOUSE) {
                if let Some(lock) = self.wl_lock_pointer.take() {
                    lock.destroy();
                }
                if let Some(rel_pointer) = self.wl_rel_pointer.take() {
                    rel_pointer.destroy();
                }
                self.grabbed.set(self.grabbed.get() - Grab::MOUSE);
                display.notify("grabbed");
            }
        }

        fn key_pressed(&self, ec: &gtk::EventControllerKey, keyval: gdk::keys::Key, keycode: u32) {
            let display = self.get_instance();

            if let Some(ref e) = ec.get_current_event() {
                if self.grab_shortcut.get().unwrap().trigger(e, false) == gdk::KeyMatch::Exact {
                    if self.grabbed.get().is_empty() {
                        self.try_grab();
                    } else {
                        self.ungrab_keyboard(ec);
                        self.ungrab_mouse();
                    }
                    return;
                }
            }

            display
                .emit_by_name("key-press", &[&*keyval, &keycode])
                .unwrap();
        }

        fn key_released(&self, keyval: gdk::keys::Key, keycode: u32) {
            let display = self.get_instance();

            display
                .emit_by_name("key-release", &[&*keyval, &keycode])
                .unwrap();
        }

        fn try_grab_keyboard(&self) -> bool {
            if self.grabbed.get().contains(Grab::KEYBOARD) {
                return false;
            }

            let obj = self.get_instance();
            let toplevel = match self.get_toplevel() {
                Some(toplevel) => toplevel,
                _ => return false,
            };

            toplevel.inhibit_system_shortcuts::<gdk::ButtonEvent>(None);
            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            ec.connect_key_pressed(clone!(@weak obj, @weak toplevel => @default-panic, move |ec, keyval, keycode, _state| {
                let self_ = Self::from_instance(&obj);
                self_.key_pressed(ec, keyval, keycode);
                glib::signal::Inhibit(true)
            }));
            ec.connect_key_released(
                clone!(@weak obj => @default-panic, move |_ec, keyval, keycode, _state| {
                    let self_ = Self::from_instance(&obj);
                    self_.key_released(keyval, keycode);
                }),
            );
            if let Some(root) = obj.get_root() {
                root.add_controller(&ec);
            }

            let id = toplevel.connect_property_shortcuts_inhibited_notify(
                clone!(@weak obj => @default-panic, move |toplevel| {
                    let inhibited = toplevel.get_property_shortcuts_inhibited();
                    log::debug!("shortcuts-inhibited: {}", inhibited);
                    if !inhibited {
                        let self_ = Self::from_instance(&obj);
                        let id = self_.shortcuts_inhibited_id.take();
                        toplevel.disconnect(id.unwrap());
                        self_.grabbed.set(self_.grabbed.get() - Grab::KEYBOARD);
                        obj.notify("grabbed");
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
            let pointer = device.get_wl_pointer();
            let obj = self.get_instance();

            if self.wl_lock_pointer.borrow().is_none() {
                if let Some(constraints) = self.wl_pointer_constraints.get() {
                    if let Some(surf) = self.get_wl_surface() {
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
                                let scale = obj.get_scale_factor() as f64;
                                let (dx, dy) = (dx_unaccel / scale, dy_unaccel / scale);
                                obj.emit_by_name("motion-relative", &[&dx, &dy]).unwrap();
                            }
                        }),
                    );
                    self.wl_rel_pointer.replace(Some(rel_pointer));
                }
            }

            true
        }

        fn try_grab_mouse(&self) -> bool {
            let obj = self.get_instance();
            if obj.mouse_absolute() {
                return false;
            }
            if obj.grabbed().contains(Grab::MOUSE) {
                return false;
            }

            let default_seat = obj.get_display().get_default_seat();

            for device in default_seat.get_devices(gdk::SeatCapabilities::POINTER) {
                self.try_grab_device(device);
            }

            true
        }

        fn try_grab(&self) {
            let display = self.get_instance();
            let mut grabbed = display.grabbed();
            if self.try_grab_keyboard() {
                grabbed |= Grab::KEYBOARD;
            }
            if self.try_grab_mouse() {
                grabbed |= Grab::MOUSE;
            }
            self.grabbed.set(grabbed);
            display.notify("grabbed");
        }

        pub(crate) fn texture_id(&self) -> GLuint {
            self.texture_id.get()
        }

        unsafe fn texture_blit(&self, flip: bool) {
            gl::UseProgram(if flip {
                todo!();
                //self.texture_blit_flip_prog.get()
            } else {
                self.texture_blit_prog.get()
            });
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.texture_id());
            gl::BindVertexArray(self.texture_blit_vao.get());
            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        }

        fn borders(&self) -> (u32, u32) {
            let display = self.get_instance();
            let (dw, dh) = match display.display_size() {
                Some(size) => size,
                None => return (0, 0),
            };
            let sf = display.get_scale_factor();
            let (w, h) = (display.get_width() * sf, display.get_height() * sf);
            let (sw, sh) = (w as f32 / dw as f32, h as f32 / dh as f32);

            if sw < sh {
                let bh = h - (h as f32 * sw / sh) as i32;
                (0, bh as u32 / 2)
            } else {
                let bw = w - (w as f32 * sh / sw) as i32;
                (bw as u32 / 2, 0)
            }
        }

        fn viewport(&self) -> Option<gdk::Rectangle> {
            let display = self.get_instance();
            display.display_size()?;

            let sf = display.get_scale_factor();
            let (w, h) = (display.get_width() * sf, display.get_height() * sf);
            let (borderw, borderh) = self.borders();
            let (borderw, borderh) = (borderw as i32, borderh as i32);
            Some(gdk::Rectangle {
                x: borderw,
                y: borderh,
                width: w - borderw * 2,
                height: h - borderh * 2,
            })
        }

        fn transform_input(&self, x: f64, y: f64) -> Option<(f64, f64)> {
            let display = self.get_instance();
            let sf = display.get_scale_factor() as f64;
            self.viewport().and_then(|vp| {
                let (x, y) = (x * sf, y * sf);
                if !vp.contains_point(x as _, y as _) {
                    return None;
                }
                let (sw, sh) = display.display_size().unwrap();
                let x = (x - vp.x as f64) * (sw as f64 / vp.width as f64);
                let y = (y - vp.y as f64) * (sh as f64 / vp.height as f64);
                Some((x, y))
            })
        }

        fn get_toplevel(&self) -> Option<gdk::Toplevel> {
            let display = self.get_instance();
            display
                .get_root()
                .and_then(|r| r.get_native())
                .and_then(|n| n.get_surface())
                .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
        }

        fn get_wl_surface(&self) -> Option<wayland_client::protocol::wl_surface::WlSurface> {
            let display = self.get_instance();
            display
                .get_native()
                .and_then(|n| n.get_surface())
                .and_then(|s| s.downcast::<gdk_wl::WaylandSurface>().ok())
                .map(|w| w.get_wl_surface())
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
        let tex = gdk::Texture::new_for_pixbuf(&pb);
        gdk::Cursor::from_texture(&tex, hot_x * scale, hot_y * scale, None)
    }
}

pub const NONE_DISPLAY: Option<&Display> = None;

pub trait DisplayExt: 'static {
    fn display_size(&self) -> Option<(u32, u32)>;

    fn set_display_size(&self, size: Option<(u32, u32)>);

    fn define_cursor(&self, cursor: Option<gdk::Cursor>);

    fn mouse_absolute(&self) -> bool;

    fn set_mouse_absolute(&self, absolute: bool);

    fn set_cursor_position(&self, pos: Option<(u32, u32)>);

    fn grabbed(&self) -> Grab;

    fn update_area(&self, x: i32, y: i32, w: i32, h: i32, stride: i32, data: &[u8]);

    fn connect_key_press<F: Fn(&Self, u32, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_key_release<F: Fn(&Self, u32, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_motion<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_motion_relative<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_mouse_press<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_mouse_release<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_scroll_discrete<F: Fn(&Self, Scroll) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_property_grabbed_notify<F: Fn(&Self) + 'static>(&self, f: F) -> SignalHandlerId;
}

impl<O: IsA<Display> + IsA<gtk::GLArea> + IsA<gtk::Widget>> DisplayExt for O {
    fn display_size(&self) -> Option<(u32, u32)> {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });

        self_.display_size.get()
    }

    fn set_display_size(&self, size: Option<(u32, u32)>) {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });
        self.make_current();

        if let Some((width, height)) = size {
            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, self_.texture_id());
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

        self_.display_size.replace(size);
    }

    fn define_cursor(&self, cursor: Option<gdk::Cursor>) {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });

        // TODO: for now client side only
        self.set_cursor(cursor.as_ref());
        self_.cursor.replace(cursor);
    }

    fn mouse_absolute(&self) -> bool {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });

        self_.mouse_absolute.get()
    }

    fn set_mouse_absolute(&self, absolute: bool) {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });

        if absolute {
            self_.ungrab_mouse();
        }

        self_.mouse_absolute.set(absolute);
    }

    fn set_cursor_position(&self, pos: Option<(u32, u32)>) {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });

        if pos.is_some() {
            self.set_cursor_from_name(Some("none"));
        } else {
            self.set_cursor(self_.cursor.borrow().as_ref());
        }
        self_.cursor_position.set(pos);
    }

    fn grabbed(&self) -> Grab {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });

        self_.grabbed.get()
    }

    fn update_area(&self, x: i32, y: i32, w: i32, h: i32, stride: i32, data: &[u8]) {
        // Safety: safe because IsA<Display>
        let self_ = imp::Display::from_instance(unsafe { self.unsafe_cast_ref::<Display>() });
        self.make_current();

        // TODO: check data boundaries
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self_.texture_id());
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

        self.queue_render();
    }

    fn connect_key_press<F: Fn(&Self, u32, u32) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, u32, u32) + 'static>(
            this: *mut imp::RdwDisplay,
            keyval: u32,
            keycode: u32,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                keyval,
                keycode,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"key-press\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_key_release<F: Fn(&Self, u32, u32) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, u32, u32) + 'static>(
            this: *mut imp::RdwDisplay,
            keyval: u32,
            keycode: u32,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f = &*(f as *const F);
            f(
                &*Display::from_glib_borrow(this).unsafe_cast_ref::<P>(),
                keyval,
                keycode,
            )
        }
        unsafe {
            let f: Box<F> = Box::new(f);
            glib::signal::connect_raw(
                self.as_ptr() as *mut glib::gobject_ffi::GObject,
                b"key-release\0".as_ptr() as *const _,
                Some(std::mem::transmute(connect_trampoline::<Self, F> as usize)),
                Box::into_raw(f),
            )
        }
    }

    fn connect_motion<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId {
        unsafe extern "C" fn connect_trampoline<P, F: Fn(&P, f64, f64) + 'static>(
            this: *mut imp::RdwDisplay,
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
            this: *mut imp::RdwDisplay,
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
            this: *mut imp::RdwDisplay,
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
            this: *mut imp::RdwDisplay,
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
            this: *mut imp::RdwDisplay,
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
            this: *mut imp::RdwDisplay,
            _param_spec: glib::ffi::gpointer,
            f: glib::ffi::gpointer,
        ) where
            P: IsA<Display>,
        {
            let f: &F = &*(f as *const F);
            f(&Display::from_glib_borrow(this).unsafe_cast_ref())
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
}

pub trait DisplayImpl: DisplayImplExt + GLAreaImpl {}

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

glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends gtk::GLArea, gtk::Widget;
}
