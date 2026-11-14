fn handle(game: &mut crate::game::Game<'_>) {
    crate::game::agent::dihrill_shop::setup_frame(game);
    crate::game::agent::dihrill_menu::frame(game);
}
