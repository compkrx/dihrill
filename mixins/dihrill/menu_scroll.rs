fn handle(game: &crate::game::Game<'_>, scroll: [f64; 2], canceled: &mut bool) {
    crate::game::agent::dihrill_menu::scroll(game,scroll[1],canceled);
}
