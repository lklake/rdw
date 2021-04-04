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
