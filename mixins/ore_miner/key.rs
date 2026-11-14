fn key_event(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::agent::ore_miner_menu::open_key(game,key,pressed);
}
