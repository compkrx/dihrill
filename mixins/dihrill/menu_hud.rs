fn handle(game: &crate::game::Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    crate::game::agent::dihrill_menu::render_hud(game, batch);
}
