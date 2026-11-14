use super::{Game, S, play};

/// The shop owns the inventory changes. Send the standard left-click
/// action with no predicted changes, as the client does for unmodeled menus.
pub(in crate::game) fn shop_left_click(game: &mut Game<'_>, window: i32, slot: usize) -> bool {
    let (open, menu) = game.remote_menu();
    if window != open { return false; }
    let Some(slot) = Game::menu_slot_index(&menu, slot) else { return false; };
    let Some(remote) = game.remote.as_ref() else { return false; };
    game.send(S::ContainerClick(play::ContainerClick {
        container: window,
        state: remote.state_id,
        slot: slot as i16,
        button: 0,
        input: 0,
        changed: Vec::new(),
        carried: None,
    }));
    true
}

pub(in crate::game) fn block_pending(game: &super::Game<'_>, pos: [i32; 3]) -> bool {
    game.remote.as_ref().is_some_and(|remote| remote.predictions.contains_key(&pos))
}

pub(in crate::game) fn player_id(game: &super::Game<'_>) -> Option<i32> {
    game.remote.as_ref().map(|remote| remote.entity_id)
}
