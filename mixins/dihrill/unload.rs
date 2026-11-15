fn unload(game: &mut crate::game::Game<'_>) {
    crate::game::agent::dihrill_shop::clear_abandoned();
    crate::game::agent::dihrill_shelter::cancel(game);
    crate::game::agent::dihrill::unload(game);
}
