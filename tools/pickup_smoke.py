"""Verify that Dihrill collects all ore and waits for a delayed drop."""
import argparse
from pathlib import Path
import tempfile

from extended_smoke import command, has_block, local_command, seed_config, status
from smoke import CLIENT_FLAGS, game_process, ready


def diamond_count(game):
    return game.run("inventory")["counts"].get("minecraft:diamond", 0)


def diamond_drops(game):
    return [
        entity for entity in game.run("entities 16 item")["entities"]
        if entity["item"]["item"] == "minecraft:diamond"
    ]


def test_pickup(executable, home):
    seed_config(home, explore=False, radius=16)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Pickup QA", "--seed", "42"]
    with game_process(executable, home, flags) as game:
        ready(game)
        for line in (
            "fill -2 97 -2 25 105 3 minecraft:bedrock",
            "fill 0 100 0 21 101 0 minecraft:air",
            "setblock 3 100 0 minecraft:diamond_ore",
            "setblock 4 100 0 minecraft:deepslate_diamond_ore",
            "setblock 4 101 0 minecraft:diamond_ore",
            "gamerule spawn_mobs false",
            "tp @s 0.5 100 0.5 -90 0",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe",
        ):
            local_command(game, line)
        game.run("wait 20", timeout=30)
        command(game, "#dihrill diamond")
        game.wait_for("all three drops collected from a connected vein",
                      lambda: diamond_count(game) == 3, timeout=30)
        assert not diamond_drops(game), diamond_drops(game)
        for pos in ((3, 100, 0), (4, 100, 0), (4, 101, 0)):
            assert has_block(game, pos, "air"), pos
        print("PASS: picked up all three diamonds from a connected vein.", flush=True)

        command(game, "#dihrill stop")
        for line in (
            "clear @s minecraft:diamond",
            "tp @s 10.5 100 0.5 -90 0",
            "setblock 13 100 0 minecraft:diamond_ore",
        ):
            local_command(game, line)
        game.run("wait 10", timeout=30)
        local_command(game, 'summon minecraft:item 12.5 100.1 0.5 {Item:{id:"minecraft:diamond",count:1},PickupDelay:70s}')
        game.wait_for("delayed diamond entity arrives", lambda: len(diamond_drops(game)) == 1, timeout=5)
        command(game, "#dihrill diamond 1")
        game.wait_for("block goal after delayed pickup",
                      lambda: "Mining goal reached" in status(game), timeout=30)
        assert diamond_count(game) == 2, (diamond_count(game), diamond_drops(game))
        assert not diamond_drops(game), diamond_drops(game)
        print("PASS: finished the block quota only after collecting the delayed diamond.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    options = parser.parse_args()
    options.out.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="dihrill-pickup-", dir=options.out.resolve()))
    print(directory, flush=True)
    test_pickup(options.client.resolve(), directory)


if __name__ == "__main__":
    main()
