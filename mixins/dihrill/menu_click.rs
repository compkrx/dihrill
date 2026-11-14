fn handle(game: &crate::game::Game<'_>, screen: &horizon_client::mod_api::NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !*canceled { crate::game::agent::dihrill::shop_manual(); }
    crate::game::agent::dihrill_menu::click(game,screen.mouse,button,canceled);
}
