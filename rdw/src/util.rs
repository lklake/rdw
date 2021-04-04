use gl::types::*;
use std::ffi::{CStr, CString};

unsafe fn compile_shader(type_: GLenum, src: &CStr) -> GLuint {
    let shader = gl::CreateShader(type_);
    gl::ShaderSource(shader, 1, &src.as_ptr(), std::ptr::null());
    gl::CompileShader(shader);
    shader
}

fn cstring_new_len(len: usize) -> CString {
    let buffer: Vec<u8> = Vec::with_capacity(len + 1);
    unsafe { CString::from_vec_unchecked(buffer) }
}

pub(crate) unsafe fn compile_gl_prog(vs: &CStr, fs: &CStr) -> Result<GLuint, String> {
    let vs = compile_shader(gl::VERTEX_SHADER, vs);
    let fs = compile_shader(gl::FRAGMENT_SHADER, fs);
    let prog = gl::CreateProgram();

    gl::AttachShader(prog, vs);
    gl::AttachShader(prog, fs);
    gl::LinkProgram(prog);

    let mut status: i32 = 0;
    gl::GetProgramiv(prog, gl::LINK_STATUS, &mut status);
    if status == 0 {
        let mut len: GLint = 0;
        gl::GetProgramiv(prog, gl::INFO_LOG_LENGTH, &mut len);
        let error = cstring_new_len(len as usize);
        gl::GetProgramInfoLog(
            prog,
            len,
            std::ptr::null_mut(),
            error.as_ptr() as *mut gl::types::GLchar,
        );
        return Err(error.to_string_lossy().into_owned());
    }
    gl::DeleteShader(vs);
    gl::DeleteShader(fs);
    Ok(prog)
}
