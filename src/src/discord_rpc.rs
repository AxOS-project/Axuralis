// SPDX-FileCopyrightText: 2025 Ardox
// SPDX-License-Identifier: GPL-3.0-or-later


// TODO: Make this running async. Cannot be run in a thread because of RefCell.


use std::cell::RefCell;
use std::convert::TryInto;
use discord_rich_presence::{activity::{Activity, Assets, Timestamps, ActivityType}, DiscordIpc, DiscordIpcClient};
use std::time::Duration;

use crate::audio::Song;

pub struct DiscordRPC {
    client: DiscordIpcClient,
    song: RefCell<Option<Song>>,
}

impl DiscordRPC {
    pub fn new(application_id: &str) -> Self {
        let mut client = DiscordIpcClient::new(application_id).expect("Failed to create Discord client");
        client.connect()
            .expect("Failed to connect to Discord IPC");
        DiscordRPC { client, song: RefCell::new(None) }
    }

    pub fn update_status(&mut self) {
        let song = self.song.borrow_mut().take();

        if let Some(song) = song {
            let title = song.title();
            let artist = song.artist();
            let length = song.duration().to_string();
            let assets = Assets::new().large_image("dummy");
            let start_time = 0_i64;
            let end_time = if length == "0" {
                0_i64
            } else {
                length.parse::<i64>().unwrap_or(0)
            };

            let timestamps = Timestamps::new()
                .start(start_time.try_into().unwrap())
                .end(end_time.try_into().unwrap());

            let activity = Activity::new()
                .details(&title)
                .state(&artist)
                .assets(assets)
                .activity_type(ActivityType::Listening)
                .timestamps(timestamps);

            self.client.set_activity(activity).expect("TODO: panic message");
        } else {
            let activity = Activity::new()
                .details("No song playing")
                .state("")
                .assets(Assets::new().large_image("dummy"))
                .activity_type(ActivityType::Listening);

            self.client.set_activity(activity).expect("TODO: panic message");
        }
    }

    pub fn run(self) {
        std::thread::spawn(move || {
            let mut rpc = self;
            loop {
                rpc.update_status();
                std::thread::sleep(Duration::from_secs(10));
            }
        });
    }
}
