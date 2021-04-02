use gtk::prelude::*;

use std::env::args;

fn build_ui(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title(Some("Rdw-Vnc basic test"));
    window.set_default_size(1024, 768);

    let display = rdw_vnc::DisplayVnc::new();
    window.set_child(Some(&display));

    window.show();
}

fn main() {
    let application = gtk::Application::new(Some("org.gnome.rdw-vnc.basic"), Default::default())
        .expect("Initialization failed...");

    application.connect_activate(build_ui);

    application.run(&args().collect::<Vec<_>>());
}
