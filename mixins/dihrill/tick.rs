fn tick(game: &mut crate::game::Game<'_>) {
    crate::game::agent::dihrill_shelter::restore_escape_click(game);
    crate::game::agent::dihrill::tick(game);
    crate::game::agent::dihrill_shelter::tick(game);
}
