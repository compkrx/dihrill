"""Verify cave and hostile-mob avoidance in scratch Survival worlds."""
import argparse
import json
from pathlib import Path
import tempfile
import time

from extended_smoke import command, config_file, has_block, local_command, seed_config, status
from smoke import CLIENT_FLAGS, game_process, ready


def setup(game, commands):
    ready(game)
    for line in (*commands, "clear @s",
                 "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe",
                 "gamerule spawn_mobs false", "gamemode survival @s"):
        local_command(game, line)
    game.run("wait 20")


def cave(client, home):
    seed_config(home, profile="hunter", explore=False, radius=16, targets=["minecraft:diamond_ore"])
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "StrataDrill Cave Guard", "--seed", "42"]
    with game_process(client, home, flags) as game:
        setup(game, (
            "fill -2 149 -4 15 157 7 minecraft:bedrock",
            "fill 0 151 -1 4 152 1 minecraft:air",
            "fill 6 151 -1 12 153 3 minecraft:air",
            "setblock 5 151 0 minecraft:stone",
            "setblock 5 152 0 minecraft:stone",
            "setblock 3 151 0 minecraft:diamond_ore",
            "setblock 8 151 1 minecraft:diamond_ore",
            "tp @s 0.5 151 0.5 -90 0",
        ))
        command(game, "#mine forever")
        game.wait_for("ore inside starting tunnel mined",
                      lambda: has_block(game, (3, 151, 0), "air"), timeout=25)
        game.wait_for("cave path abandoned",
                      lambda: "No reachable" in status(game), timeout=30)
        assert has_block(game, (5, 151, 0), "stone")
        assert has_block(game, (5, 152, 0), "stone")
        assert has_block(game, (8, 151, 1), "diamond_ore")
        game.run("options-set guiScale 1")
        command(game, "#mine settings")
        game.wait_for("menu opens", lambda: game.run("screen")["screen"] == "mod")
        for x, y in ((475, 192), (395, 345), (395, 218)):
            game.run(f"click-at {x} {y}")
            time.sleep(0.25)
        cfg = json.loads(config_file(home).read_text(encoding="utf8"))
        assert cfg["avoid_caves"] is False and cfg["avoid_mobs"] is True, cfg
        game.run("key key.keyboard.escape tap")
        command(game, "#mine forever")
        game.wait_for("same chamber becomes reachable with cave guard disabled",
                      lambda: has_block(game, (8, 151, 1), "air"), timeout=35)
        print("PASS: mined ore in a three-wide tunnel, left both wall blocks and the cave ore intact,"
              " then mined the cave ore after the Safety toggle was disabled.", flush=True)


def shaft(client, home):
    seed_config(home, profile="hunter", targets=["minecraft:deepslate_diamond_ore"],
                radius=32, explore=True, continuous_explore=True, avoid_caves=True)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "StrataDrill Shaft Guard", "--seed", "42"]
    with game_process(client, home, flags) as game:
        setup(game, (
            "fill -3 148 -3 18 160 3 minecraft:bedrock",
            "fill 0 151 0 0 159 0 minecraft:air",
            "fill 1 151 0 12 152 0 minecraft:deepslate",
            "setblock 9 151 0 minecraft:deepslate_diamond_ore",
            "fill 13 151 0 14 155 0 minecraft:air",
            "setblock 15 151 0 minecraft:deepslate_diamond_ore",
            "tp @s 0.5 151 0.5 -90 0",
        ))
        command(game, "#mine forever")
        try:
            game.wait_for("exit the narrow shaft and collect the diamond", lambda:
                          game.run("inventory")["counts"].get("minecraft:diamond") == 1,
                          timeout=45)
            game.wait_for("real cave ends the mining run",
                          lambda: "Stopped" in status(game), timeout=45)
        except RuntimeError as error:
            raise RuntimeError(f"{error}. StrataDrill status: {status(game)}") from error
        assert has_block(game, (12, 151, 0), "deepslate")
        assert has_block(game, (12, 152, 0), "deepslate")
        assert has_block(game, (15, 151, 0), "deepslate_diamond_ore")
        print("PASS: mined out of a one-block shaft and left the wall into a tall cave intact.", flush=True)


def mobs(client, home):
    seed_config(home, profile="hunter", explore=False, radius=28, targets=["minecraft:diamond_ore"])
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "StrataDrill Mob Guard", "--seed", "42"]
    with game_process(client, home, flags) as game:
        setup(game, (
            "fill -2 149 -4 25 157 5 minecraft:bedrock",
            "fill 0 151 0 22 152 0 minecraft:air",
            "fill 4 151 2 5 152 2 minecraft:air",
            "summon minecraft:skeleton 4.5 151 2.5 {NoAI:1b,Tags:[\"hidden\"]}",
            "fill 10 151 0 18 152 0 minecraft:stone",
            "setblock 20 151 0 minecraft:diamond_ore",
            "tp @s 0.5 151 0.5 -90 0",
        ))
        command(game, "#mine forever")
        def position():
            return game.run("state")["player"]["position"][0]
        game.wait_for("miner walks past shielded skeleton", lambda: position() >= 6.0, timeout=25)
        # This mob has a clear underground line of sight once its tunnel is opened.
        local_command(game, "summon minecraft:creeper 12.5 151 0.5 {NoAI:1b,Tags:[\"visible\"]}")
        local_command(game, "fill 10 151 0 13 152 0 minecraft:air")
        def retreat_finished():
            state = status(game)
            assert "Paused because" not in state, (state, position())
            return "Retreated from a hostile mob" in state

        game.wait_for("retreat and pause", retreat_finished, timeout=25)
        end_x = position()
        assert end_x < 5.6, end_x
        assert has_block(game, (20,151,0), "diamond_ore")
        assert game.run("state")["player"]["health"] == 20
        local_command(game, "kill @e[tag=visible]")
        game.wait_for("client receives the creeper removal",
                      lambda: not game.run("entities 24 creeper")["entities"], timeout=8)
        command(game, "#mine resume")
        game.wait_for("mine again after the threat is removed",
                      lambda: has_block(game, (20,151,0), "air"), timeout=45)
        print(f"PASS: ignored a skeleton behind a wall, retreated to x={end_x:.1f} from a visible"
              " creeper, paused without damage, and mined again after manual resume.", flush=True)

        command(game, "#mine stop")
        local_command(game, "setblock 21 151 0 minecraft:diamond_ore")
        local_command(game, "tp @s 1.5 151 0.5 -90 0")
        game.run("wait 8")
        command(game, "#mine forever")
        game.wait_for("miner starts another trip", lambda: position() >= 3.0, timeout=20)
        local_command(game, "damage @s 2 minecraft:generic")
        game.wait_for("damage triggers a pause", lambda: "you took damage" in status(game), timeout=10)
        assert has_block(game, (21,151,0), "diamond_ore")
        print("PASS: taking damage pauses the miner before the next target.", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--case", choices=["all", "cave", "shaft", "mobs"], default="all")
    options = parser.parse_args()
    options.out.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="stratadrill-safety-", dir=options.out.resolve()))
    print(directory, flush=True)
    for name, check in (("cave", cave), ("shaft", shaft), ("mobs", mobs)):
        if options.case in ("all", name):
            check(options.client.resolve(), directory / name)
