fn tick(game: &mut crate::game::Game<'_>) {
    crate::game::agent::dihrill::tick(game);
}
