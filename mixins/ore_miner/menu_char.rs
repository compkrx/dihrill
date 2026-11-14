fn handle(game: &crate::game::Game<'_>, c: char, canceled: &mut bool) {
    crate::game::agent::ore_miner_menu::character(game,c,canceled);
}
