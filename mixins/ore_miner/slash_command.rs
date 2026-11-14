fn handle(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    if !*ran {
        *ran = crate::game::agent::ore_miner_commands::slash_command(game, command);
    }
}
