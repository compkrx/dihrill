fn handle(game: &crate::game::Game<'_>, c: char, canceled: &mut bool) {
    crate::game::agent::dihrill_menu::character(game,c,canceled);
}
