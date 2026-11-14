fn handle(mods: &mut Vec<horizon_client::mod_api::NativeMod>) {
    crate::game::agent::ore_miner_menu::register(mods);
}
