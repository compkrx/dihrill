use super::{Game, Screen};
use super::dihrill;
use super::dihrill_settings::ShopMenu;
use crate::game::remote::dihrill_remote;
use std::cell::RefCell;

pub(super) const DIAMOND: &str = "minecraft:diamond";
const RESPONSE_TICKS: u64 = 70;

// Setup opens the live menu for inspection and remembers its signature without
// touching any slots. Automatic visits only act in this exact kind of window.
thread_local! {
    static SETUP: RefCell<Option<(String, u64)>> = const { RefCell::new(None) };
}

pub(super) fn server<'a>(game: &'a Game<'_>) -> Option<&'a str> {
    game.server_address().filter(|_| game.remote.is_some() && game.title.is_none())
}

fn open_menu(game: &Game<'_>) -> Option<(i32, ShopMenu)> {
    if game.screen != Screen::Inventory || game.container != Some((i32::MIN, 0, 0)) {
        return None;
    }
    let (window, menu) = game.remote_menu();
    if window <= 0 || !matches!(menu.as_str(),
        "minecraft:generic_9x1" | "minecraft:generic_9x2" | "minecraft:generic_9x3" |
        "minecraft:generic_9x4" | "minecraft:generic_9x5" | "minecraft:generic_9x6")
    {
        return None;
    }
    let title = game.remote_menu_title()?.plain();
    Some((window, ShopMenu { address: server(game)?.to_owned(), title, menu }))
}

fn say(game: &mut Game<'_>, message: &str) {
    game.native_chat(&format!("[Dihrill] {message}"));
}

pub(super) fn start_setup(game: &mut Game<'_>) -> String {
    let Some(address) = server(game).map(str::to_owned) else {
        return "Connect to the server that provides /shop.".into();
    };
    if game.carried.is_some() || game.remote_menu_id() != 0 {
        return "Close the existing inventory and empty your cursor first.".into();
    }
    if dihrill::snapshot(game).running { dihrill::pause_from_menu(game); }
    game.native_mod_screen_close("dihrill");
    SETUP.with(|s| *s.borrow_mut() = Some((address, game.ticks + RESPONSE_TICKS)));
    game.handle_chat_input("/shop");
    "Waiting for /shop.".into()
}

pub(in crate::game) fn setup_frame(game: &mut Game<'_>) {
    let result = SETUP.with(|s| {
        let mut setup = s.borrow_mut();
        let (address, deadline) = setup.as_ref()?;
        if server(game) != Some(address) {
            setup.take();
            return None;
        }
        if let Some((_, menu)) = open_menu(game) {
            setup.take();
            return Some(Ok(menu));
        }
        if game.ticks >= *deadline || game.native_mod_screen().is_some() {
            setup.take();
            return Some(Err("/shop did not open a supported menu. Diamond selling was not enabled."));
        }
        None
    });
    match result {
        Some(Ok(menu)) => {
            dihrill::update_settings(game, |settings| settings.diamond_shop = Some(menu));
            say(game, "Diamond selling is enabled on this server. Close /shop and start or resume Dihrill.");
        }
        Some(Err(message)) => say(game, message),
        None => (),
    }
}

fn diamonds(game: &Game<'_>, slot: usize) -> u32 {
    game.inventory[slot].as_ref().filter(|stack|
        stack.item.as_ref() == DIAMOND && stack.damage == 0 && stack.extra.is_none()
    ).map_or(0, |stack| stack.count)
}

const OPEN_SETTLE_TICKS: u64 = 4;
const CLICK_INTERVAL_TICKS: u64 = 4;

enum Phase {
    Open { deadline: u64 },
    Sell { window: i32, ready: u64 },
}

pub(super) struct Visit {
    menu: ShopMenu,
    phase: Phase,
    slots: Vec<usize>,
    next: usize,
    manual: bool,
}

pub(super) enum Result {
    Active,
    Finished,
    Unavailable(&'static str),
    Interrupted(&'static str),
}

impl Visit {
    pub(super) fn start(game: &mut Game<'_>, menu: &ShopMenu, minimum: u32) -> Option<Self> {
        if game.screen != Screen::Playing || game.carried.is_some() || game.remote_menu_id() != 0
            || server(game) != Some(&menu.address)
        {
            return None;
        }
        let mut stacks: Vec<_> = (0..36).filter_map(|slot| {
            let count = diamonds(game, slot);
            (count > 0).then_some((std::cmp::Reverse(count), slot))
        }).collect();
        if stacks.iter().map(|(count, _)| count.0).sum::<u32>() < minimum {
            return None;
        }
        stacks.sort_unstable();
        let slots = stacks.into_iter().take(2).map(|(_, slot)| slot).collect();

        game.handle_chat_input("/shop");
        Some(Self {
            menu: menu.clone(),
            phase: Phase::Open { deadline: game.ticks + RESPONSE_TICKS },
            slots,
            next: 0,
            manual: false,
        })
    }

    pub(super) fn manual_click(&mut self) {
        self.manual = true;
    }

    fn verified(&self, game: &Game<'_>, window: i32) -> bool {
        open_menu(game).is_some_and(|(id, menu)| id == window && menu == self.menu)
    }

    pub(super) fn close(&self, game: &mut Game<'_>) {
        if !self.manual && let Phase::Sell { window, .. } = self.phase
            && game.carried.is_none() && self.verified(game, window)
        {
            game.close_container();
            game.screen = Screen::Playing;
            game.capture(true);
        }
    }

    pub(super) fn tick(&mut self, game: &mut Game<'_>) -> Result {
        if self.manual { return Result::Interrupted("Diamond selling stopped for manual inventory input."); }
        if server(game) != Some(&self.menu.address) {
            return Result::Interrupted("Diamond selling stopped because the server changed.");
        }
        let now = game.ticks;
        if let Phase::Open { deadline } = self.phase {
            if let Some((window, menu)) = open_menu(game) {
                if menu != self.menu {
                    return Result::Unavailable("The server opened a different shop menu. Run diamond shop setup again.");
                }
                if game.carried.is_none() {
                    self.phase = Phase::Sell { window, ready: now + OPEN_SETTLE_TICKS };
                    return Result::Active;
                }
            }
            if now >= deadline || !matches!(game.screen, Screen::Playing | Screen::Inventory) {
                return Result::Unavailable("The configured /shop did not open. Run diamond shop setup again.");
            }
            return Result::Active;
        }

        let Phase::Sell { window, .. } = self.phase else { unreachable!() };
        if !self.verified(game, window) {
            return Result::Interrupted("Diamond selling stopped because the shop window changed.");
        }
        let Phase::Sell { ready, .. } = &mut self.phase else { unreachable!() };
        if now < *ready { return Result::Active; }
        if game.carried.is_some() {
            return Result::Interrupted("Diamond selling stopped because the inventory cursor is occupied.");
        }
        while let Some(&slot) = self.slots.get(self.next) {
            self.next += 1;
            if diamonds(game, slot) == 0 { continue; }
            if !dihrill_remote::shop_left_click(game, window, slot) {
                return Result::Interrupted("The shop did not accept a click on the diamond slot.");
            }
            *ready = now + CLICK_INTERVAL_TICKS;
            return Result::Active;
        }
        self.close(game);
        say(game, "Shop visit complete. Mining resumed.");
        Result::Finished
    }
}
