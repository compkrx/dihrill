fn handle(game: &mut crate::game::Game<'_>) {
    if crate::game::agent::dihrill::finish_emergency_exit(game) { return; }
    crate::game::agent::dihrill_shop::setup_frame(game);
    crate::game::agent::dihrill_menu::frame(game);
}
