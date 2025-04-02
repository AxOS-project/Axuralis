// SPDX-FileCopyrightText: 2025 Ardox
// SPDX-License-Identifier: GPL-3.0-or-later

// I know this code sucks, but it works

use std::convert::TryInto;
use discord_rich_presence::{activity::{Activity, Assets, ActivityType}, DiscordIpc, DiscordIpcClient};
use std::time::{Duration, SystemTime};
use discord_rich_presence::activity::Timestamps;
use mpris::PlayerFinder;
use pickledb::{PickleDb, PickleDbDumpPolicy, SerializationMethod};

pub struct DiscordRPC {
    client: DiscordIpcClient,
    db: Option<PickleDb>,
}

impl DiscordRPC {
    pub fn new(application_id: &str) -> Self {
        let db = PickleDb::new(
            "cover_art_cache.db",
            PickleDbDumpPolicy::AutoDump,
            SerializationMethod::Json,
        );
        let mut client = DiscordIpcClient::new(application_id).expect("Failed to create Discord client");
        client.connect()
            .expect("Failed to connect to Discord IPC");
        DiscordRPC {
            client,
            db: Some(db),
        }
    }


    fn get_cover(&mut self, artist: &str, title: &str) -> String {

        let cache_key = format!("cover_{}_{}", artist, title);

        if let Some(db) = &self.db {
            if db.exists(&cache_key) {
                if let Some(cached_url) = db.get::<String>(&cache_key) {
                    return cached_url;
                }
            }
        }

        let get_mbid = || -> String {
            let url = format!("https://musicbrainz.org/ws/2/release/?query=artist={}%20AND%20title={}&fmt=json", artist, title);
            let headers = {
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::USER_AGENT,
                    "Axuralis/1.2".parse().unwrap(),
                );
                headers
            };
            let client = reqwest::blocking::Client::builder()
                .default_headers(headers)
                .build()
                .unwrap();
            let response = client.get(&url).send();
            if let Ok(response) = response {
                if let Ok(json) = response.json::<serde_json::Value>() {
                    return json["releases"][0]["id"].as_str().unwrap().to_string();
                };
            };
            "".to_string()
        };

        let mbid = get_mbid();
        println!("MBID: {}", mbid);
        if mbid.is_empty() {
            return "".to_string();
        }
        let cover_url = reqwest::blocking::get(&format!(
            "https://coverartarchive.org/release/{}/front",
            mbid
        ));
        if cover_url.is_ok() {
            println!("Cover Archive URL: {:?}", cover_url);
            println!("Doing a request to coverartarchive");
            let url = cover_url.unwrap().url().to_string();

            // Cache the result if we have a valid URL and database
            if !url.is_empty() && self.db.is_some() {
                if let Some(db) = &mut self.db {
                    db.set(&cache_key, &url).unwrap_or(());
                    db.dump().unwrap_or(());
                }
            }
            return url;
        }
        "".to_string()
    }

    fn update_status(&mut self) {

        let player_finder = PlayerFinder::new().expect("Could not create PlayerFinder");
        let player = player_finder.find_by_name("Axuralis").expect("Could not find player");

        let metadata = match player.get_metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                eprintln!("Failed to get metadata: {:?}", err);
                return;
            }
        };

        if player.is_running() {

            let title = metadata.title().unwrap_or("Unknown Title").to_string();
            let artist = metadata.artists().unwrap_or(vec!["Unknown Artist"])[0].to_string();

            let cover_url = self.get_cover(&artist, &title);
            let assets = Assets::new().large_image(&cover_url)
                .large_text(&title)
                .small_image("play-button");

            let track_position = match player.get_position() {
                Ok(position) => position.as_secs(),
                Err(_) => Duration::new(0, 0).as_secs(),
            };

            let start_time: u64 = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                Ok(n) => n.as_secs() - track_position,
                Err(_) => 0,
            };

            let track_duration = metadata.length().unwrap_or(Duration::new(0, 0)).as_secs();

            let end_time: u64 = start_time + track_duration;

            let timestamps = Timestamps::new()
                .start(start_time.try_into().unwrap())
                .end(end_time.try_into().unwrap());

            let activity = Activity::new()
                .details(&title)
                .state(&artist)
                .assets(assets)
                .activity_type(ActivityType::Listening)
                .timestamps(timestamps);

            self.client.set_activity(activity).expect("Failed to set Discord activity");
        } else {
            let activity = Activity::new()
                .details("No song playing")
                .state("")
                .assets(Assets::new().large_image("dummy"))
                .activity_type(ActivityType::Listening);

            self.client.set_activity(activity).expect("Failed to set Discord activity");
        }
    }

    pub fn run(mut self) {
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                self.update_status();
            }
        });
    }
}
