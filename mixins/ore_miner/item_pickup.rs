fn on_packet(
    game: &crate::game::Game<'_>,
    packet: &horizon_protocol::packets::play::Clientbound,
) {
    if let horizon_protocol::packets::play::Clientbound::TakeItemEntity(pickup) = packet {
        crate::game::agent::ore_miner::item_pickup(game, pickup.player, pickup.item, pickup.amount);
    }
}
