//! Upgrade Peios: the release this machine runs, whether a newer one is
//! offered, and the upgrade to it.
//!
//! upgrade-peios is the only thing that moves a release, and it has no
//! daemon: Upgrade Peios runs it, as the person, reading its status's JSON
//! and holding a conversation with its driven mode for an upgrade.

use libgxwi::App;

mod manager;
mod page;
mod tool;

use manager::Manager;

// What this program looks like, to whatever lists it. The icon itself is
// `gxwi-upgrade-peios.svg` at the repo root, installed as the base theme's.
libgxwi::icon!(b"dev.peios.gxwi-upgrade-peios");

fn main() {
    if std::env::args().nth(1).is_some() {
        eprintln!("gxwi-upgrade-peios: usage: gxwi-upgrade-peios (given {:?})", std::env::args().skip(1).collect::<Vec<_>>());
        std::process::exit(64);
    }
    let mut app = match App::connect() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("gxwi-upgrade-peios: no desktop to open on: {e}");
            std::process::exit(1);
        }
    };
    libgxwi::settings::stylesheet(&mut app);
    app.stylesheet("/gxwi-upgrade-peios.css", include_str!("gxwi-upgrade-peios.css"));
    let window = app.live("Upgrade Peios", Manager::new());
    let aside = std::sync::Arc::downgrade(&window);
    window.update(|manager, _| {
        manager.window = aside;
        manager.reread(true);
    });
    if let Err(e) = app.run() {
        eprintln!("gxwi-upgrade-peios: {e}");
        std::process::exit(1);
    }
}
