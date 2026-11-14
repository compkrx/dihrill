fn handle(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    if !*ran {
        *ran = crate::game::agent::dihrill_commands::slash_command(game, command);
    }
}
