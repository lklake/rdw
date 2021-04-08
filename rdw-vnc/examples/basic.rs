use std::{cell::RefCell, sync::Arc};

use gio::ApplicationFlags;
use glib::{clone, translate::ToGlib};
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
    let has_error = Arc::new(RefCell::new(false));

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
                let mut port = uri.get_port();
                if port == -1 {
                    port = 5900;
                }
                let host = uri.get_host().unwrap_or("localhost".into());
                *addr2.borrow_mut() = (host.to_string(), port);
                app.activate();
                -1
            }
            None => 1,
        }
    });

    let has_error2 = has_error.clone();
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
            if !d.grabbed().is_empty() {
                title = format!("{} - grabbed {:?}", title, d.grabbed())
            }
            window.set_title(Some(title.as_str()));
        }));

        let has_error = has_error2.clone();
        display
            .connection()
            .connect_vnc_error(clone!(@weak app => move |_, msg| {
                *has_error.borrow_mut() = true;
                let mut dialog = gtk::MessageDialogBuilder::new()
                    .modal(true)
                    .buttons(gtk::ButtonsType::Ok)
                    .text(msg);
                if let Some(parent) = app.get_active_window() {
                    dialog = dialog.transient_for(&parent);
                }
                let dialog = dialog.build();
                let run_dialog = async move {
                    dialog.run_future().await;
                    app.quit();
                };
                glib::MainContext::default().spawn_local(run_dialog);
            }));

        let has_error = has_error2.clone();
        display
            .connection()
            .connect_vnc_disconnected(clone!(@weak app => move |_| {
                if !*has_error.borrow() {
                    app.quit();
                }
            }));

        display
            .connection()
            .connect_vnc_auth_credential(clone!(@weak app => move |conn, va|{
                use gvnc::ConnectionCredential::*;

                let creds: Vec<_> = va.iter().map(|v| v.get_some::<gvnc::ConnectionCredential>().unwrap()).collect();
                let mut dialog = gtk::MessageDialogBuilder::new()
                    .modal(true)
                    .buttons(gtk::ButtonsType::Ok)
                    .text("Credentials required");
                if let Some(parent) = app.get_active_window() {
                    dialog = dialog.transient_for(&parent);
                }
                let dialog = dialog.build();
                let content = dialog.get_content_area();
                let grid = gtk::GridBuilder::new()
                    .hexpand(true)
                    .vexpand(true)
                    .halign(gtk::Align::Center)
                    .valign(gtk::Align::Center)
                    .row_spacing(6)
                    .column_spacing(6)
                    .build();
                content.append(&grid);
                let username = gtk::Entry::new();
                if creds.contains(&Username) {
                    grid.attach(&gtk::Label::new(Some("Username")), 0, 0, 1, 1);
                    grid.attach(&username, 1, 0, 1, 1);
                }
                let password = gtk::Entry::new();
                if creds.contains(&Password) {
                    grid.attach(&gtk::Label::new(Some("Password")), 0, 1, 1, 1);
                    grid.attach(&password, 1, 1, 1, 1);
                }
                let run_dialog = clone!(@weak conn, @strong username, @strong password => async move {
                    dialog.run_future().await;
                    if creds.contains(&Username) {
                        conn.set_credential(Username.to_glib(), &username.get_text()).unwrap();
                    }
                    if creds.contains(&Password) {
                        conn.set_credential(Password.to_glib(), &password.get_text()).unwrap();
                    }
                    if creds.contains(&Clientname) {
                        conn.set_credential(Clientname.to_glib(), "rdw-vnc").unwrap();
                    }
                    dialog.destroy();
                });

                glib::MainContext::default().spawn_local(run_dialog);
            }));
        window.set_child(Some(&display));

        window.show();
    });

    app.run();
}
