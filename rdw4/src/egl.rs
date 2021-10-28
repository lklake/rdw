use std::os::unix::prelude::RawFd;

pub use khronos_egl::*;

#[cfg(not(feature = "bindings"))]
mod imp {
    use super::*;
    use once_cell::sync::OnceCell;

    type EglInstance =
        khronos_egl::Instance<khronos_egl::Dynamic<libloading::Library, khronos_egl::EGL1_5>>;

    pub(crate) fn egl() -> &'static EglInstance {
        static INSTANCE: OnceCell<EglInstance> = OnceCell::new();
        INSTANCE.get_or_init(|| unsafe {
            let lib = libloading::Library::new("libEGL.so").expect("unable to find libEGL.so");
            khronos_egl::DynamicInstance::<khronos_egl::EGL1_5>::load_required_from(lib)
                .expect("unable to load libEGL.so")
        })
    }

    pub(crate) const LINUX_DMA_BUF_EXT: Enum = 0x3270;
    pub(crate) const LINUX_DRM_FOURCC_EXT: Int = 0x3271;
    pub(crate) const DMA_BUF_PLANE0_FD_EXT: Int = 0x3272;
    pub(crate) const DMA_BUF_PLANE0_OFFSET_EXT: Int = 0x3273;
    pub(crate) const DMA_BUF_PLANE0_PITCH_EXT: Int = 0x3274;
    pub(crate) const DMA_BUF_PLANE0_MODIFIER_LO_EXT: Int = 0x3443;
    pub(crate) const DMA_BUF_PLANE0_MODIFIER_HI_EXT: Int = 0x3444;

    // GLAPI void APIENTRY glEGLImageTargetTexture2DOES (GLenum target, GLeglImageOES image);

    pub(crate) type ImageTargetTexture2DOesFn =
        extern "C" fn(gl::types::GLenum, gl::types::GLeglImageOES);

    pub(crate) fn image_target_texture_2d_oes() -> Option<ImageTargetTexture2DOesFn> {
        unsafe {
            egl()
                .get_proc_address("glEGLImageTargetTexture2DOES")
                .map(|f| std::mem::transmute::<_, ImageTargetTexture2DOesFn>(f))
        }
    }

    pub(crate) fn no_context() -> Context {
        unsafe { Context::from_ptr(NO_CONTEXT) }
    }

    pub(crate) fn no_client_buffer() -> ClientBuffer {
        unsafe { ClientBuffer::from_ptr(std::ptr::null_mut()) }
    }
}

#[cfg(not(feature = "bindings"))]
pub(crate) use imp::*;

/// RdwDmabufScanout:
/// @fd: DMABUF fd, ownership is taken.
///
/// A DMABUF file descriptor along with the associated details.
#[derive(Debug)]
#[repr(C)]
pub struct RdwDmabufScanout {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub fd: RawFd,
    pub y0_top: bool,
}

impl Drop for RdwDmabufScanout {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}
