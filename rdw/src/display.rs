use gl::types::*;
use glib::{clone, signal::SignalHandlerId, subclass::prelude::*, translate::FromGlibPtrBorrow};
use gtk::{gdk, glib, prelude::*, subclass::prelude::GLAreaImpl};
use std::cell::Cell;

use crate::{egl, error::Error, util};

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
        pub key_controller: OnceCell<gtk::EventControllerKey>,
        // The remote display size, ex: 1024x768
        pub display_size: Cell<Option<(u32, u32)>>,
        // The currently defined cursor
        pub cursor: RefCell<Option<gdk::Cursor>>,
        pub texture_id: Cell<GLuint>,
        pub texture_blit_vao: Cell<GLuint>,
        pub texture_blit_prog: Cell<GLuint>,
        pub texture_blit_flip_prog: Cell<GLuint>,
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
            self.parent_realize(widget);
            widget.make_current();

            if let Err(e) = unsafe { self.realize_gl() } {
                let e = glib::Error::new(Error::GL, &e);
                widget.set_error(Some(&e));
            }

            widget.set_sensitive(true);
            widget.set_focusable(true);
            widget.set_focus_on_click(true);

            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            widget.add_controller(&ec);
            ec.connect_key_pressed(
                clone!(@weak widget => @default-panic, move |_, keyval, keycode, _state| {
                    widget.emit_by_name("key-press", &[&*keyval, &keycode]).unwrap();
                    glib::signal::Inhibit(true)
                }),
            );
            ec.connect_key_released(clone!(@weak widget => move |_, keyval, keycode, _state| {
                widget.emit_by_name("key-release", &[&*keyval, &keycode]).unwrap();
            }));
            self.key_controller.set(ec).unwrap();

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

    fn update_area(&self, x: i32, y: i32, w: i32, h: i32, stride: i32, data: &[u8]);

    fn connect_key_press<F: Fn(&Self, u32, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_key_release<F: Fn(&Self, u32, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_motion<F: Fn(&Self, f64, f64) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_mouse_press<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId;

    fn connect_mouse_release<F: Fn(&Self, u32) + 'static>(&self, f: F) -> SignalHandlerId;
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
