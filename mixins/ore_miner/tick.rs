fn tick(game: &mut crate::game::Game<'_>) {
    crate::game::agent::ore_miner::tick(game);
}
