use std::sync::mpsc;
use tray_item::IconSource;
use tray_item::TrayItem;

use mpris::PlayerFinder;

enum Message {
    Play,
    Pause,
    Next,
    Previous,
}

pub struct SystemTray {
    tray: TrayItem,
}

impl SystemTray {

    pub fn new() -> Self {
        let icon = IconSource::Resource("/usr/share/icons/hicolor/scalable/apps/com.axos-project.Axuralis.svg");
        let tray = TrayItem::new("Axuralis", icon).unwrap();
        SystemTray { tray }
    }

    fn tray(&mut self) {

        let tray = &mut self.tray;

        tray.add_label("Axuralis control").unwrap();

        let (tx, rx) = mpsc::sync_channel::<Message>(2);

        let play_tx = tx.clone();
        let pause_tx = tx.clone();
        let next_tx = tx.clone();
        let prev_tx = tx.clone();

        tray.add_menu_item("Play", move || { play_tx.send(Message::Play).unwrap(); }).unwrap();
        tray.add_menu_item("Pause", move || { pause_tx.send(Message::Pause).unwrap(); }).unwrap();
        tray.add_menu_item("Next", move || { next_tx.send(Message::Next).unwrap(); }).unwrap();
        tray.add_menu_item("Previous", move || { prev_tx.send(Message::Previous).unwrap(); }).unwrap();


        loop {
            let player_finder = PlayerFinder::new().expect("Could not create PlayerFinder");

            let player_result = player_finder.find_by_name("Axuralis");

            match player_result {
                Ok(player) => {
                    if player.is_running() {
                        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                            Ok(Message::Pause) => {
                                player.pause().unwrap_or_else(|e| println!("Could not pause player: {}", e));
                            }
                            Ok(Message::Play) => {
                                player.play().unwrap_or_else(|e| println!("Could not play player: {}", e));
                            }
                            Ok(Message::Next) => {
                                player.next().unwrap_or_else(|e| println!("Could not play player: {}", e));
                            }
                            Ok(Message::Previous) => {
                                player.previous().unwrap_or_else(|e| println!("Could not play player: {}", e));
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                // No message received, continue
                            }
                            Err(e) => {
                                println!("Error receiving message: {:?}", e);
                                break;
                            }
                        }
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(1));

                    match rx.try_recv() {
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn run(mut self) {
        std::thread::spawn(move || {
            self.tray();
        });
    }
}