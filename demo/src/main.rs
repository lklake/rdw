use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use gio::ApplicationFlags;
use glib::{clone, translate::IntoGlib};
use gtk::{gdk, gio, glib, prelude::*};
use rdw::{gtk, DisplayExt};
use rdw_spice::spice::{self, prelude::*};
use rdw_vnc::{gvnc, rdw};

fn show_error(app: gtk::Application, msg: &str) {
    let mut dialog = gtk::MessageDialogBuilder::new()
        .modal(true)
        .buttons(gtk::ButtonsType::Ok)
        .text(msg);
    if let Some(parent) = app.active_window() {
        dialog = dialog.transient_for(&parent);
    }
    let dialog = dialog.build();
    let run_dialog = async move {
        dialog.run_future().await;
        app.quit();
    };
    glib::MainContext::default().spawn_local(run_dialog);
}

fn rdp_display(app: &gtk::Application, uri: glib::Uri) -> rdw::Display {
    let mut rdp = rdw_rdp::Display::new();

    let mut port = uri.port();
    if port == -1 {
        port = 3389;
    }
    let host = uri.host().unwrap_or_else(|| "localhost".into());

    rdp.rdp_settings().set_server_port(port as _);
    rdp.rdp_settings()
        .set_server_hostname(Some(host.as_str()))
        .unwrap();
    rdp.rdp_connect().unwrap();

    rdp.upcast()
}

fn vnc_display(app: &gtk::Application, uri: glib::Uri) -> rdw::Display {
    let has_error = Arc::new(AtomicBool::new(false));

    let mut port = uri.port();
    if port == -1 {
        port = 5900;
    }
    let host = uri.host().unwrap_or_else(|| "localhost".into());
    let vnc = rdw_vnc::Display::new();
    vnc.connection()
        .open_host(&host, &format!("{}", port))
        .unwrap();

    let has_error2 = has_error.clone();
    vnc.connection()
        .connect_vnc_error(clone!(@weak app => move |_, msg| {
            has_error2.store(true, Ordering::Relaxed);
            show_error(app, msg);
        }));

    vnc.connection()
        .connect_vnc_disconnected(clone!(@weak app => move |_| {
            if !has_error.load(Ordering::Relaxed) {
                app.quit();
            }
        }));

    vnc
        .connection()
        .connect_vnc_auth_credential(clone!(@weak app => move |conn, va|{
            use gvnc::ConnectionCredential::*;

            let creds: Vec<_> = va.iter().map(|v| v.get::<gvnc::ConnectionCredential>().unwrap()).collect();
            let mut dialog = gtk::MessageDialogBuilder::new()
                .modal(true)
                .buttons(gtk::ButtonsType::Ok)
                .text("Credentials required");
            if let Some(parent) = app.active_window() {
                dialog = dialog.transient_for(&parent);
            }
            let dialog = dialog.build();
            let content = dialog.content_area();
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
                    conn.set_credential(Username.into_glib(), &username.text()).unwrap();
                }
                if creds.contains(&Password) {
                    conn.set_credential(Password.into_glib(), &password.text()).unwrap();
                }
                if creds.contains(&Clientname) {
                    conn.set_credential(Clientname.into_glib(), "rdw-vnc").unwrap();
                }
                dialog.destroy();
            });

            glib::MainContext::default().spawn_local(run_dialog);
        }));

    vnc.upcast()
}

fn spice_display(app: &gtk::Application, uri: glib::Uri) -> rdw::Display {
    let spice = rdw_spice::Display::new();
    let session = spice.session();

    session.set_uri(Some(&uri.to_string()));

    session.connect_channel_new(clone!(@weak app => move |_, channel| {
        if let Ok(main) = channel.clone().downcast::<spice::MainChannel>() {
            main.connect_channel_event(clone!(@weak app => move |channel, event| {
                use spice::ChannelEvent::*;
                if event == ErrorConnect {
                    if let Some(err) = channel.error() {
                        show_error(app, &err.to_string());
                    }
                }
            }));
        }
    }));

    session.connect_disconnected(clone!(@weak app => move |_| {
        app.quit();
    }));

    session.connect();
    spice.upcast()
}

fn make_display(app: &gtk::Application, mut uri: String) -> rdw::Display {
    if glib::Uri::peek_scheme(&uri).is_none() {
        uri = format!("vnc://{}", uri);
    }

    let uri = glib::Uri::parse(&uri, glib::UriFlags::NONE).unwrap();

    match uri.scheme().as_str() {
        "vnc" => vnc_display(app, uri),
        "rdp" => rdp_display(app, uri),
        spice if spice.starts_with("spice") => spice_display(app, uri),
        scheme => panic!("Unhandled scheme {}", scheme),
    }
}

fn main() {
    env_logger::init();

    let app = gtk::Application::new(
        Some("org.gnome.rdw.demo"),
        ApplicationFlags::NON_UNIQUE | ApplicationFlags::HANDLES_COMMAND_LINE,
    );
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
        "Enable debugging",
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
            spice::set_debug(true);
        }
        -1
    });

    let display = Arc::new(RefCell::new(None));

    let dpy = display.clone();
    app.connect_command_line(move |app, cl| {
        let uri = cl
            .options_dict()
            .lookup_value(&glib::OPTION_REMAINING, None)
            .and_then(|args| args.child_value(0).get::<String>())
            .unwrap_or_else(|| "vnc://localhost".to_string());
        let display = make_display(app, uri);
        dpy.replace(Some(display));
        app.activate();
        -1
    });

    let action_quit = gio::SimpleAction::new("quit", None);
    action_quit.connect_activate(clone!(@weak app => move |_, _| {
        app.quit();
    }));
    app.add_action(&action_quit);

    let action_usb = gio::SimpleAction::new("usb", None);
    let dpy = display.clone();
    action_usb.connect_activate(clone!(@weak app => move |_, _| {
        let display = match &*dpy.borrow() {
            Some(display) => display.clone(),
            _ => return,
        };

        if let Ok(spice) = display.downcast::<rdw_spice::Display>() {
            let usbredir = match rdw_spice::UsbRedir::build(spice.session()) {
                Ok(it) => it,
                Err(e) => {
                    panic!("Failed to open USB dialog: {}", e);
                }
            };
            let dialog = gtk::Dialog::new();
            dialog.set_transient_for(app.active_window().as_ref());
            dialog.set_child(Some(&usbredir));
            dialog.show();
        }
    }));
    app.add_action(&action_usb);

    app.connect_activate(move |app| {
        build_ui(app, display.clone());
    });
    app.run();
}

fn build_ui(app: &gtk::Application, display: Arc<RefCell<Option<rdw::Display>>>) {
    let ui_src = include_str!("demo.ui");
    let builder = gtk::Builder::new();
    builder
        .add_from_string(ui_src)
        .expect("Couldn't add from string");
    let window: gtk::ApplicationWindow = builder.object("window").expect("Couldn't get window");
    window.set_application(Some(app));

    if let Some(display) = &*display.borrow() {
        display.connect_property_grabbed_notify(clone!(@weak window => move |d| {
            let mut title = "rdw demo".to_string();
            if !d.grabbed().is_empty() {
                title = format!("{} - {}", title, d.grab_shortcut().to_label(&gdk::Display::default().unwrap()))
            }
            window.set_title(Some(title.as_str()));
        }));

        window.set_child(Some(display));
    }

    window.show();
}
