fn handle(game: &crate::game::Game<'_>, key: &str, repeat: bool, canceled: &mut bool) {
    crate::game::agent::ore_miner_menu::key(game,key,repeat,canceled);
}
