use cargo_metadata::*;
use std::path::*;

use cbindgen::Builder;

fn main() {
    let path = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let meta = MetadataCommand::new()
        .manifest_path("./Cargo.toml")
        .current_dir(&path)
        .exec()
        .unwrap();

    println!("{:?}", &meta);

    if cfg!(feature = "capi") {
        let version = &meta.root_package().unwrap().version;
        let name = &meta.root_package().unwrap().metadata["capi"]["header"]["name"]
            .as_str()
            .unwrap();
        let out = std::env::var("OUT_DIR").unwrap();
        let out = Path::new(&out);
        let out_include = out.join("capi/include/");
        std::fs::create_dir_all(&out_include).unwrap();

        let mut config = cbindgen::Config::default();
        let warning = config.autogen_warning.unwrap_or_default();
        let version_info = format!(
            r"
#define {0}_MAJOR_VERSION {1}
#define {0}_MINOR_VERSION {2}
#define {0}_PATCH_VERSION {3}

#define {0}_CHECK_VERSION(major,minor,path)    \
    ({0}_MAJOR_VERSION > (major) || \
     ({0}_MAJOR_VERSION == (major) && {0}_MINOR_VERSION > (minor)) || \
     ({0}_MAJOR_VERSION == (major) && {0}_MINOR_VERSION == (minor) && \
      {0}_PATCH_VERSION >= (patch)))
",
            name.to_uppercase(),
            version.major,
            version.minor,
            version.patch
        );
        config.autogen_warning = Some(warning + &version_info);

        Builder::new()
            .with_crate(&path)
            .with_config(config)
            .with_gobject(true)
            .with_include_version(true)
            .with_include_guard(format!("{}_H", name.to_uppercase()))
            .with_sys_include("gtk/gtk.h")
            .generate()
            .unwrap()
            .write_to_file(out_include.join(format!("{}.h", name)));
    }

    if cfg!(feature = "bindings") {
        println!("cargo:rustc-link-lib=dylib=rdw4");
    }
}
