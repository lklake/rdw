use std::{cell::RefCell, sync::Arc};

use gio::ApplicationFlags;
use glib::clone;
use gtk::{gio, glib, prelude::*};
use rdw::DisplayExt;

fn main() {
    env_logger::init();

    let app = gtk::Application::new(
        Some("org.gnome.rdw-vnc.basic"),
        ApplicationFlags::NON_UNIQUE | ApplicationFlags::HANDLES_COMMAND_LINE,
    )
    .expect("Initialization failed...");
    app.add_main_option(
        "version",
        glib::Char(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Show program version",
        None,
    );
    app.add_main_option(
        "debug",
        glib::Char(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Enable gtk-vnc debugging",
        None,
    );
    app.add_main_option(
        &glib::OPTION_REMAINING,
        glib::Char(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::StringArray,
        "URI",
        Some("URI"),
    );
    app.connect_handle_local_options(|_, opt| {
        if opt.lookup_value("version", None).is_some() {
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        if opt.lookup_value("debug", None).is_some() {
            gvnc::set_debug(true);
        }
        -1
    });

    let addr = Arc::new(RefCell::new(("localhost".to_string(), 5900)));

    let addr2 = addr.clone();
    app.connect_command_line(move |app, cl| {
        match cl
            .get_options_dict()
            .lookup_value(&glib::OPTION_REMAINING, None)
        {
            Some(args) => {
                let mut arg = args.get_child_value(0).get::<String>().unwrap();
                if !arg.starts_with("vnc://") {
                    arg = format!("vnc://{}", arg);
                }
                let uri = glib::Uri::parse(&arg, glib::UriFlags::NONE).unwrap();
                *addr2.borrow_mut() = (uri.get_host().unwrap().to_string(), uri.get_port());
                app.activate();
                -1
            }
            None => 1,
        }
    });

    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::new(app);

        window.set_title(Some("rdw-vnc example"));
        window.set_default_size(1024, 768);

        let display = rdw_vnc::DisplayVnc::new();
        let (host, port) = &*addr.borrow();
        display
            .connection()
            .open_host(&host, &format!("{}", port))
            .unwrap();
        display.connect_property_grabbed_notify(clone!(@weak window => move |d| {
            let mut title = "rdw-vnc example".to_string();
            if !d.get_grabbed().is_empty() {
                title = format!("{} - grabbed {:?}", title, d.get_grabbed())
            }
            window.set_title(Some(title.as_str()));
        }));
        display
            .connection()
            .connect_vnc_error(clone!(@weak app => move |_, msg|{
                eprintln!("{}", msg);
            }));
        display
            .connection()
            .connect_vnc_disconnected(clone!(@weak app => move |_|{
                app.quit();
            }));
        window.set_child(Some(&display));

        window.show();
    });

    app.run();
}
