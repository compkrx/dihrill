fn handle(game: &mut Game<'_>, message: &str) -> std::ops::ControlFlow<()> {
    if crate::game::agent::dihrill_commands::hash_command(game, message) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
