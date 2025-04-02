// SPDX-FileCopyrightText: 2025 Ardox
// SPDX-License-Identifier: GPL-3.0-or-later

mod application;
mod audio;
mod config;
mod cover_picture;
mod drag_overlay;
mod i18n;
mod marquee;
mod playback_control;
mod playlist_view;
mod queue_row;
mod search;
mod song_cover;
mod song_details;
mod sort;
mod utils;
mod volume_control;
mod waveform_view;
mod window;
mod discord_rpc;

use std::env;

use config::{APPLICATION_ID, GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR, PROFILE};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};
use gtk::{gio, glib, prelude::*};
use log::{debug, error, LevelFilter};

use discord_rpc::DiscordRPC;

use sysinfo::{ProcessesToUpdate, System};

use self::application::Application;

fn main() -> glib::ExitCode {
    let mut builder = pretty_env_logger::formatted_builder();
    if APPLICATION_ID.ends_with("Devel") {
        builder.filter(Some("axuralis"), LevelFilter::Debug);
    } else {
        builder.filter(Some("axuralis"), LevelFilter::Info);
    }
    builder.init();

    // Set up gettext translations
    debug!("Setting up locale data");
    setlocale(LocaleCategory::LcAll, "");

    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    debug!("Setting up pulseaudio environment");
    let app_id = APPLICATION_ID.trim_end_matches(".Devel");
    env::set_var("PULSE_PROP_application.icon_name", app_id);
    env::set_var("PULSE_PROP_application.metadata().name", "Axuralis");
    env::set_var("PULSE_PROP_media.role", "music");

    debug!("Loading resources");
    let resources = match env::var("MESON_DEVENV") {
        Err(_) => gio::Resource::load(PKGDATADIR.to_owned() + "/axuralis.gresource")
            .expect("Unable to find axuralis.gresource"),
        Ok(_) => match env::current_exe() {
            Ok(path) => {
                let mut resource_path = path;
                resource_path.pop();
                resource_path.push("axuralis.gresource");
                gio::Resource::load(&resource_path)
                    .expect("Unable to find axuralis.gresource in devenv")
            }
            Err(err) => {
                error!("Unable to find the current path: {}", err);
                return glib::ExitCode::FAILURE;
            }
        },
    };
    gio::resources_register(&resources);

    debug!("Setting up application (profile: {})", &PROFILE);
    glib::set_application_name("Axuralis");
    glib::set_program_name(Some("axuralis"));

    gst::init().expect("Failed to initialize gstreamer");

    let ctx = glib::MainContext::default();
    let _guard = ctx.acquire().unwrap();

    fn is_discord_running() -> bool {
        let mut system = System::new_all();
        system.refresh_processes(ProcessesToUpdate::All, false);

        for process in system.processes().values() {
            let name = process.name().to_string_lossy().to_lowercase();
            if name.contains("discord") {
                return true;
            }
        }
        false
    }

    if is_discord_running() {
        let client = DiscordRPC::new("1355915802166956182");
        client.run();
    } else {
        println!("Discord RPC not available")
    }

    Application::new().run()
}