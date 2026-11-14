fn handle(game: &mut crate::game::Game<'_>) {
    crate::game::agent::ore_miner_shop::setup_frame(game);
    crate::game::agent::ore_miner_menu::frame(game);
}
