fn handle(game: &crate::game::Game<'_>, key: &str, repeat: bool, canceled: &mut bool) {
    crate::game::agent::dihrill_menu::key(game,key,repeat,canceled);
}
