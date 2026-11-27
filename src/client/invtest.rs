//! The inventory against a real NeoForge 1.21.1 server, headless: the
//! client's own connection, translation and click code, checked after
//! every step against what the server says the player holds.
//!
//! Ignored by default. Needs a NeoForge 21.1 server folder with
//! Sophisticated Backpacks and Core (`TORCH_INVTEST_SERVER`; the test
//! rewrites its `server.properties` and world) and Java 21 (`java`, or
//! `TORCH_INVTEST_JAVA`):
//!
//! `cargo test --release --lib invtest -- --ignored --nocapture`

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use azalea::entity::inventory::Inventory;
use azalea_inventory::ItemStack;
use azalea_inventory::operations::{ClickOperation, PickupClick, QuickMoveClick};

use crate::session::InvAction;

const NAME: &str = "InvTest";
const PORT: u16 = 25599;
const WORLD: &str = "invtest-world";

/// The server, driven through its console.
struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Server {
    fn start(dir: &Path) -> Server {
        let props = dir.join("server.properties");
        let mut text = std::fs::read_to_string(&props).unwrap_or_default();
        for (key, value) in [
            ("server-port", PORT.to_string()),
            ("online-mode", "false".into()),
            ("level-name", WORLD.into()),
            ("level-type", "minecraft\\:flat".into()),
            (
                "generator-settings",
                r#"{"layers":[{"block":"minecraft:bedrock","height":1},{"block":"minecraft:stone","height":3}],"biome":"minecraft:plains"}"#.into(),
            ),
            ("spawn-protection", "0".into()),
            ("difficulty", "peaceful".into()),
            ("gamemode", "survival".into()),
            ("generate-structures", "false".into()),
            ("spawn-monsters", "false".into()),
        ] {
            text = set_property(&text, key, &value);
        }
        std::fs::write(&props, text).unwrap();
        std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();
        let _ = std::fs::remove_dir_all(dir.join(WORLD));

        let java = std::env::var("TORCH_INVTEST_JAVA").unwrap_or_else(|_| "java".into());
        let mut child = Command::new(java)
            .current_dir(dir)
            .args([
                "@user_jvm_args.txt",
                "@libraries/net/neoforged/neoforge/21.1.264/win_args.txt",
                "nogui",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("java starts the server");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                eprintln!("[server] {line}");
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut server = Server {
            child,
            stdin,
            lines,
        };
        server.wait_for(|l| l.contains("Done ("), Duration::from_secs(600));
        server
    }

    fn cmd(&mut self, command: &str) {
        eprintln!("[test] /{command}");
        writeln!(self.stdin, "{command}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn wait_for(&mut self, want: impl Fn(&str) -> bool, within: Duration) -> String {
        let end = Instant::now() + within;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if want(&line) => return line,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("the server didn't say it in time"),
                Err(RecvTimeoutError::Disconnected) => panic!("the server stopped"),
            }
        }
    }

    fn drain(&mut self) {
        while self.lines.try_recv().is_ok() {}
    }

    /// The player's items as the server has them: `(menu slot, id, count)`
    /// in the player menu's numbering.
    fn inventory(&mut self) -> Vec<(usize, String, u32)> {
        self.drain();
        self.cmd(&format!("data get entity {NAME} Inventory"));
        let line = self.wait_for(
            |l| l.contains("has the following entity data"),
            Duration::from_secs(10),
        );
        let list = &line[line.find("data: ").unwrap() + 6..];
        let mut items: Vec<_> = entries(list)
            .iter()
            .filter_map(|e| {
                let slot = menu_slot(key(e, "Slot")?.trim_end_matches('b').parse().ok()?)?;
                let id = key(e, "id")?.trim_matches('"').to_owned();
                let count = key(e, "count")?.parse().ok()?;
                Some((slot, id, count))
            })
            .collect();
        items.sort();
        items
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "stop");
        let _ = self.stdin.flush();
        let end = Instant::now() + Duration::from_secs(60);
        while Instant::now() < end {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = self.child.kill();
    }
}

fn set_property(text: &str, key: &str, value: &str) -> String {
    let line = format!("{key}={value}");
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            if l.split('=').next() == Some(key) {
                found = true;
                line.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !found {
        out.push(line);
    }
    out.join("\n") + "\n"
}

/// The top-level `{...}` entries of an SNBT list.
fn entries(list: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut quoted) = (0, 0, false);
    for (i, c) in list.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '{' | '[' if !quoted => {
                depth += 1;
                if depth == 2 && c == '{' {
                    start = i;
                }
            }
            '}' | ']' if !quoted => {
                if depth == 2 && c == '}' {
                    out.push(list[start..=i].to_owned());
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    out
}

/// A top-level key's value in one SNBT entry (nested values skipped).
fn key(entry: &str, name: &str) -> Option<String> {
    let mut flat = String::new();
    let (mut depth, mut quoted) = (0, false);
    for c in entry.chars() {
        match c {
            '"' => quoted = !quoted,
            '{' | '[' if !quoted => depth += 1,
            '}' | ']' if !quoted => depth -= 1,
            _ => {}
        }
        if depth == 1 && !matches!(c, '{' | '[') {
            flat.push(c);
        }
    }
    flat.split(", ").find_map(|part| {
        let (k, v) = part.split_once(": ")?;
        (k.trim() == name).then(|| v.trim().to_owned())
    })
}

/// The inventory's `Slot` in the player menu's numbering.
fn menu_slot(slot: i32) -> Option<usize> {
    Some(match slot {
        0..=8 => 36 + slot as usize,
        9..=35 => slot as usize,
        100..=103 => (108 - slot) as usize,
        -106 => 45,
        _ => return None,
    })
}

fn name(stack: &ItemStack) -> Option<(String, u32)> {
    let ItemStack::Present(d) = stack else {
        return None;
    };
    let id = match d.modded_kind {
        Some(raw) => crate::protocol::modded_item(raw.get())
            .map_or_else(|| format!("modded#{raw}"), str::to_owned),
        None => format!(
            "minecraft:{}",
            d.kind.to_str().trim_start_matches("minecraft:")
        ),
    };
    Some((id, d.count as u32))
}

fn inventory() -> Inventory {
    let client = crate::client::bot::live_client().expect("connected");
    client
        .component::<Inventory>()
        .expect("an inventory")
        .clone()
}

/// The client's player menu, as `(menu slot, id, count)`.
fn client_items(inv: &Inventory) -> Vec<(usize, String, u32)> {
    (5..=45)
        .filter_map(|i| {
            let (id, count) = name(inv.inventory_menu.slot(i)?)?;
            Some((i, id, count))
        })
        .collect()
}

/// The open mod menu's player slots, renumbered as the player menu's.
fn menu_player_items(inv: &Inventory) -> Vec<(usize, String, u32)> {
    let menu = inv.menu();
    let start = *menu.player_slots_range().start();
    (0..36)
        .filter_map(|i| {
            let (id, count) = name(menu.slot(start + i)?)?;
            Some((9 + i, id, count))
        })
        .collect()
}

/// Queues `action` for the client's tick and waits until the tick has sent
/// it, so a packet written after it can't overtake it.
fn queue(action: InvAction) {
    let shared = crate::SHARED.get().unwrap();
    let pending = || shared.lock().unwrap().session.inventory.inv_actions.len();
    shared
        .lock()
        .unwrap()
        .session
        .inventory
        .inv_actions
        .push(action);
    let end = Instant::now() + Duration::from_secs(5);
    while pending() > 0 {
        assert!(Instant::now() < end, "the tick never took the action");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn click(slot: u16) {
    queue(InvAction::Click(ClickOperation::Pickup(
        PickupClick::Left { slot: Some(slot) },
    )));
    std::thread::sleep(Duration::from_millis(300));
}

/// Waits until `check` passes; on time out, fails with what it last said.
fn eventually(what: &str, within: Duration, mut check: impl FnMut() -> Result<(), String>) {
    let end = Instant::now() + within;
    loop {
        match check() {
            Ok(()) => {
                eprintln!("[test] ok: {what}");
                return;
            }
            Err(e) if Instant::now() > end => panic!("{what}: {e}"),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// The client's player inventory equals the server's, and the cursor is
/// empty.
fn agrees(server: &mut Server, what: &str) {
    eventually(what, Duration::from_secs(8), || {
        let truth = server.inventory();
        let inv = inventory();
        let mine = client_items(&inv);
        if mine != truth {
            return Err(format!("client {mine:?}\nserver {truth:?}"));
        }
        if inv.carried.is_present() {
            return Err(format!("cursor holds {:?}", name(&inv.carried)));
        }
        Ok(())
    });
}

fn connect_and_join() {
    let shared = std::sync::Arc::new(crate::session::SharedMutex::default());
    crate::SHARED.set(shared.clone()).ok();
    crate::client::bot::set_username(NAME.into());
    // 1.21.1 is an experimental version.
    crate::protocol::set_experimental(true);
    crate::protocol::set_forced(Some(767));
    let address = format!("127.0.0.1:{PORT}");
    crate::client::bot::start_bot(address);
    // NeoForge's negotiation teaches the mods' channels over a few joins.
    let end = Instant::now() + Duration::from_secs(180);
    loop {
        assert!(Instant::now() < end, "never joined");
        if let Some(again) = crate::client::neoforge::take_retry() {
            while crate::client::bot::is_started() {
                std::thread::sleep(Duration::from_millis(100));
            }
            crate::client::bot::start_bot(again);
        } else if !crate::client::bot::is_started() {
            let status = shared.lock().unwrap().session.status.clone();
            panic!("disconnected with nothing to retry: {status:?}");
        }
        let joined = shared.lock().unwrap().in_world
            && crate::client::bot::live_client().and_then(|c| {
                c.component::<Inventory>()
                    .ok()
                    .map(|i| i.inventory_menu.len())
            }) == Some(46);
        if joined {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Opens the first backpack the player carries (Java's `BackpackOpenPayload()`).
fn open_backpack() {
    use azalea_protocol::packets::game::s_custom_payload::ServerboundCustomPayload;
    let client = crate::client::bot::live_client().unwrap();
    client.write_packet(ServerboundCustomPayload {
        identifier: "sophisticatedbackpacks:backpack_open".into(),
        data: vec![0xFF, 0xFF, 0xFF, 0xFF, 0, 0].into(),
    });
}

fn close() {
    queue(InvAction::Close);
    std::thread::sleep(Duration::from_millis(500));
}

/// The open backpack's storage, as `(slot, id, count)`.
fn storage(inv: &Inventory) -> Vec<(usize, String, u32)> {
    let menu = inv.menu();
    let start = *menu.player_slots_range().start();
    (0..start)
        .filter_map(|i| {
            let (id, count) = name(menu.slot(i)?)?;
            Some((i, id, count))
        })
        .collect()
}

fn wait_backpack(storage_slots: usize) {
    eventually("the backpack opens", Duration::from_secs(8), || {
        let inv = inventory();
        let Some(azalea_inventory::Menu::Modded { slots, player, .. }) = &inv.container_menu else {
            return Err("no mod menu".into());
        };
        match player {
            Some(p) if *p == storage_slots => Ok(()),
            _ => Err(format!(
                "{} slots, player slots from {player:?}",
                slots.len()
            )),
        }
    });
}

#[test]
#[ignore = "needs a NeoForge server (TORCH_INVTEST_SERVER) and Java"]
fn inventory_matches_a_neoforge_server() {
    let dir = std::env::var("TORCH_INVTEST_SERVER").expect("TORCH_INVTEST_SERVER");
    let mut server = Server::start(Path::new(&dir));
    connect_and_join();
    server.wait_for(
        |l| l.contains(&format!("{NAME} joined")),
        Duration::from_secs(30),
    );

    server.cmd(&format!("gamemode survival {NAME}"));
    server.cmd(&format!("clear {NAME}"));
    for give in [
        "minecraft:stone 10",
        "minecraft:dirt 5",
        "minecraft:diamond_sword[enchantments={levels:{\"minecraft:sharpness\":2}}]",
        "minecraft:potion[potion_contents={potion:\"minecraft:swiftness\"}]",
        "sophisticatedbackpacks:backpack",
    ] {
        server.cmd(&format!("give {NAME} {give}"));
    }
    agrees(&mut server, "given items");

    // The player's own inventory: move, swap, put back, shift-click.
    click(36); // stone
    click(9);
    agrees(&mut server, "stone moved to the main inventory");
    click(37); // dirt
    click(9); // swaps: stone on the cursor
    click(10);
    agrees(&mut server, "dirt and stone swapped");
    queue(InvAction::Click(ClickOperation::QuickMove(
        QuickMoveClick::Left { slot: 38 },
    )));
    agrees(&mut server, "sword shift-clicked");
    assert!(
        server
            .inventory()
            .contains(&(11, "minecraft:diamond_sword".to_owned(), 1)),
        "the server moved the sword to main slot 11"
    );

    // A leather backpack: 27 storage slots, the player's 36, its upgrade.
    open_backpack();
    wait_backpack(27);
    // A plugin's screen counting the player's slots from the end (28, past
    // the upgrade): the client's match must win, or the wrong slots sync
    // into the inventory (the in-game bug of 2026-11-27).
    let inv = inventory();
    crate::modapi::screens::open_for_test(
        inv.id,
        torch_mod_api::Screen {
            player_start: Some(inv.menu().len() - 36),
            ..Default::default()
        },
    );
    std::thread::sleep(Duration::from_millis(500));
    wait_backpack(27);
    eventually(
        "the backpack shows the inventory",
        Duration::from_secs(5),
        || {
            let truth: Vec<_> = server
                .inventory()
                .into_iter()
                .filter(|e| e.0 >= 9)
                .collect();
            let shown = menu_player_items(&inventory());
            (shown == truth)
                .then_some(())
                .ok_or(format!("shown {shown:?}\nserver {truth:?}"))
        },
    );
    // Player-menu slot `s` is the backpack menu's `27 + s - 9`: the stone
    // (10), the potion (hotbar 3 = 39), the sword (11), into storage.
    for from in [27 + 1, 27 + 30, 27 + 2] {
        eventually("a stack to move", Duration::from_secs(3), || {
            let inv = inventory();
            if inv.menu().slot(from).is_some_and(ItemStack::is_present) {
                return Ok(());
            }
            Err(format!(
                "slot {from} is empty\nshown {:?}\nserver {:?}\nstorage {:?}",
                menu_player_items(&inv),
                server.inventory(),
                storage(&inv)
            ))
        });
        let free = (0..27)
            .find(|&i| inventory().menu().slot(i).is_some_and(ItemStack::is_empty))
            .unwrap() as u16;
        click(from as u16);
        click(free);
    }
    close();
    crate::modapi::screens::reset();
    agrees(&mut server, "three stacks put in the backpack");

    open_backpack();
    wait_backpack(27);
    eventually("the backpack kept them", Duration::from_secs(5), || {
        let ids: Vec<String> = storage(&inventory()).into_iter().map(|e| e.1).collect();
        let want = [
            "minecraft:stone",
            "minecraft:potion",
            "minecraft:diamond_sword",
        ];
        (ids == want)
            .then_some(())
            .ok_or(format!("storage {ids:?}"))
    });
    // Take the stone back out.
    click(0);
    click(27 + 5);
    close();
    agrees(&mut server, "stone taken back out");

    // The open key over the backpack in the inventory screen: Java's
    // `BackpackOpenPayload(4, "", "main")` (hotbar slot 4).
    {
        use azalea_protocol::packets::game::s_custom_payload::ServerboundCustomPayload;
        let mut body = 4i32.to_be_bytes().to_vec();
        body.extend_from_slice(&[0, 4]);
        body.extend_from_slice(b"main");
        crate::client::bot::live_client()
            .unwrap()
            .write_packet(ServerboundCustomPayload {
                identifier: "sophisticatedbackpacks:backpack_open".into(),
                data: body.into(),
            });
    }
    wait_backpack(27);
    close();
}

#[test]
fn snbt_entries_read_their_own_keys() {
    let list = r#"[{count: 10, Slot: 0b, id: "minecraft:stone"}, {count: 1, Slot: 2b, components: {"minecraft:enchantments": {levels: {"minecraft:sharpness": 2}}, "x": [{id: "a"}]}, id: "minecraft:diamond_sword"}]"#;
    let e = entries(list);
    assert_eq!(e.len(), 2);
    assert_eq!(
        key(&e[1], "id").as_deref(),
        Some("\"minecraft:diamond_sword\"")
    );
    assert_eq!(key(&e[1], "Slot").as_deref(), Some("2b"));
    assert_eq!(menu_slot(0), Some(36));
    assert_eq!(menu_slot(103), Some(5));
}
