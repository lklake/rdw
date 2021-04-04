use gl::types::*;
use glib::subclass::prelude::*;
use gtk::{gdk, glib, prelude::*};
use std::cell::Cell;

use crate::{egl, error::Error, util};

pub mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

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

    impl ObjectImpl for Display {}

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
        }
    }

    impl GLAreaImpl for Display {
        fn render(&self, gl_area: &Self::Type, _context: &gdk::GLContext) -> bool {
            unsafe {
                gl::ClearColor(0.1, 0.1, 0.1, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::BLEND);

                if let Some(vp) = self.viewport(gl_area) {
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

        fn texture_id(&self) -> GLuint {
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

        fn borders(&self, gl_area: &super::Display) -> (u32, u32) {
            let (dw, dh) = match gl_area.display_size() {
                Some(size) => size,
                None => return (0, 0),
            };
            let sf = gl_area.get_scale_factor();
            let (w, h) = (gl_area.get_width() * sf, gl_area.get_height() * sf);
            let (sw, sh) = (w as f32 / dw as f32, h as f32 / dh as f32);

            if sw < sh {
                let bh = h - (h as f32 * sw / sh) as i32;
                (0, bh as u32 / 2)
            } else {
                let bw = w - (w as f32 * sh / sw) as i32;
                (bw as u32 / 2, 0)
            }
        }

        fn viewport(&self, gl_area: &super::Display) -> Option<gdk::Rectangle> {
            gl_area.display_size()?;

            let sf = gl_area.get_scale_factor();
            let (w, h) = (gl_area.get_width() * sf, gl_area.get_height() * sf);
            let (borderw, borderh) = self.borders(gl_area);
            let (borderw, borderh) = (borderw as i32, borderh as i32);
            Some(gdk::Rectangle {
                x: borderw,
                y: borderh,
                width: w - borderw * 2,
                height: h - borderh * 2,
            })
        }
    }

    pub trait DisplayImpl: DisplayImplExt + GLAreaImpl {}

    pub trait DisplayImplExt: ObjectSubclass {}

    unsafe impl<T: GLAreaImpl> IsSubclassable<T> for super::Display {
        fn class_init(class: &mut glib::Class<Self>) {
            <gtk::Widget as IsSubclassable<T>>::class_init(class);
        }

        fn instance_init(instance: &mut glib::subclass::InitializingObject<T>) {
            <gtk::Widget as IsSubclassable<T>>::instance_init(instance);
        }
    }
}

impl Display {
    pub fn display_size(&self) -> Option<(u32, u32)> {
        let self_ = imp::Display::from_instance(self);

        self_.display_size.get()
    }
}

glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends gtk::GLArea, gtk::Widget;
}
