fn handle(game: &mut Game<'_>, key: winit::keyboard::KeyCode) -> std::ops::ControlFlow<()> {
    if crate::game::agent::dihrill_commands::tab(game, key) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
