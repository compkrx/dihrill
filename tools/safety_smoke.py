"""Verify cave and hostile-mob avoidance in scratch Survival worlds."""
import argparse
from contextlib import nullcontext
import json
from pathlib import Path
import tempfile
import time

from extended_smoke import command, config_file, has_block, local_command, seed_config, status
from smoke import CLIENT_FLAGS, INSTALLED, free_port, game_process, ready


def setup(game, commands):
    ready(game)
    for line in (*commands, "clear @s",
                 "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe",
                 "gamerule spawn_mobs false", "gamemode survival @s"):
        local_command(game, line)
    game.run("wait 20")


def cave(client, home):
    seed_config(home, profile="hunter", explore=False, radius=16, targets=["minecraft:diamond_ore"])
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Cave Guard", "--seed", "42"]
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
        command(game, "#dihrill forever")
        game.wait_for("ore inside starting tunnel mined",
                      lambda: has_block(game, (3, 151, 0), "air"), timeout=25)
        game.wait_for("cave path abandoned",
                      lambda: "No reachable" in status(game), timeout=30)
        assert has_block(game, (5, 151, 0), "stone")
        assert has_block(game, (5, 152, 0), "stone")
        assert has_block(game, (8, 151, 1), "diamond_ore")
        game.run("options-set guiScale 1")
        command(game, "#dihrill settings")
        game.wait_for("menu opens", lambda: game.run("screen")["screen"] == "mod")
        for x, y in ((475, 192), (395, 345), (395, 218)):
            game.run(f"click-at {x} {y}")
            time.sleep(0.25)
        cfg = json.loads(config_file(home).read_text(encoding="utf8"))
        assert cfg["avoid_caves"] is False and cfg["avoid_mobs"] is True, cfg
        game.run("key key.keyboard.escape tap")
        command(game, "#dihrill forever")
        game.wait_for("same chamber becomes reachable with cave guard disabled",
                      lambda: has_block(game, (8, 151, 1), "air"), timeout=35)
        print("PASS: mined ore in a three-wide tunnel, left both wall blocks and the cave ore intact,"
              " then mined the cave ore after the Safety toggle was disabled.", flush=True)


def shaft(client, home):
    seed_config(home, profile="hunter", targets=["minecraft:deepslate_diamond_ore"],
                radius=32, explore=True, continuous_explore=True, avoid_caves=True)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Shaft Guard", "--seed", "42"]
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
        command(game, "#dihrill forever")
        try:
            game.wait_for("exit the narrow shaft and collect the diamond", lambda:
                          game.run("inventory")["counts"].get("minecraft:diamond") == 1,
                          timeout=45)
            game.wait_for("real cave ends the mining run",
                          lambda: "Stopped" in status(game), timeout=45)
        except RuntimeError as error:
            raise RuntimeError(f"{error}. Dihrill status: {status(game)}") from error
        assert has_block(game, (12, 151, 0), "deepslate")
        assert has_block(game, (12, 152, 0), "deepslate")
        assert has_block(game, (15, 151, 0), "deepslate_diamond_ore")
        print("PASS: mined out of a one-block shaft and left the wall into a tall cave intact.", flush=True)


def crossed_tunnels(y):
    return (
        f"fill -2 {y - 2} -3 24 {y + 6} 29 minecraft:bedrock",
        f"fill 0 {y} 0 22 {y + 1} 0 minecraft:air",
        f"fill 0 {y} 0 0 {y + 1} 26 minecraft:air",
        f"setblock 20 {y} 0 minecraft:diamond_ore",
        f"setblock 0 {y} 25 minecraft:diamond_ore",
    )


def player(game):
    current = game.run("state")["player"]
    assert current and current["health"] > 0, f"Player left the world or died: {current}"
    return current


def approach_and_retreat(game, spawn_mob, y, summon, *, ai=False, spawn_x=11.5):
    command(game, "#dihrill forever")
    game.wait_for("walk towards the first ore", lambda: player(game)["position"][0] >= 6.5, timeout=25)
    before = player(game)["position"][0]
    nbt = "" if ai else " {NoAI:1b}"
    spawn_mob(f"summon minecraft:{summon} {spawn_x} {y} 0.5{nbt}")
    game.wait_for("retreat from the hostile mob", lambda: player(game)["position"][0] <= before - 1.0,
                  timeout=15)


def check_alternate_tunnel(game, y, *, full_health=True):
    try:
        game.wait_for("mine the alternate ore without manual resume",
                      lambda: has_block(game, (0, y, 25), "air"), timeout=55)
    except RuntimeError as error:
        raise RuntimeError(f"{error}. Dihrill status: {status(game)}; player: {player(game)}") from error
    assert has_block(game, (20, y, 0), "diamond_ore")
    if full_health:
        assert player(game)["health"] == 20


def mobs(client, home):
    seed_config(home, profile="hunter", explore=False, radius=28, mob_radius=6,
                targets=["minecraft:diamond_ore"])
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Mob Guard", "--seed", "42"]
    with game_process(client, home, flags) as game:
        setup(game, (
            *crossed_tunnels(151),
            "fill 4 151 2 5 152 2 minecraft:air",
            "summon minecraft:skeleton 4.5 151 2.5 {NoAI:1b,Tags:[\"hidden\"]}",
            "tp @s 0.5 151 0.5 -90 0",
        ))
        approach_and_retreat(game, lambda line: local_command(game, line), 151, "creeper")
        check_alternate_tunnel(game, 151)
        print("PASS: passed a shielded skeleton, backed away from a visible creeper, and mined"
              " the other tunnel while both mobs remained.", flush=True)


def chase(client, home):
    y = 151
    seed_config(home, profile="hunter", explore=False, radius=28, mob_radius=12,
                targets=["minecraft:diamond_ore"])
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Pursuit Guard", "--seed", "42"]
    with game_process(client, home, flags) as game:
        setup(game, (*crossed_tunnels(y), f"tp @s 0.5 {y} 0.5 -90 0"))
        for slot, item in (
            ("hotbar.0", "netherite_pickaxe"),
            ("hotbar.1", "cobblestone 16"),
            ("armor.head", "netherite_helmet"),
            ("armor.chest", "netherite_chestplate"),
            ("armor.legs", "netherite_leggings"),
            ("armor.feet", "netherite_boots"),
            ("weapon.offhand", "totem_of_undying"),
        ):
            local_command(game, f"item replace entity @s {slot} with minecraft:{item}")
        game.run("wait 5")
        approach_and_retreat(game, lambda line: local_command(game, line), y, "cave_spider",
                             ai=True, spawn_x=14.5)

        def barricade():
            player(game)
            return next((x for x in range(1, 12) if
                         has_block(game, (x, y, 0), "cobblestone")
                         and has_block(game, (x, y + 1, 0), "cobblestone")), None)

        wall_x = game.wait_for("seal one column of the pursuit tunnel", barricade, timeout=15)
        check_alternate_tunnel(game, y, full_health=False)
        assert barricade() == wall_x
        proof = local_command(game, "execute if items entity @s weapon.offhand minecraft:totem_of_undying")
        assert "Test passed" in str(proof), proof
        remaining = player(game)["health"]
        print(f"PASS: blocked a pursuing cave spider at x={wall_x} and mined the alternate tunnel"
              f" automatically with {remaining:g} health and the totem intact.", flush=True)


def shelter(client, home):
    seed_config(home, profile="hunter", explore=False, avoid_caves=False, radius=8, run_seconds=2,
                targets=["minecraft:obsidian"])
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Shelter Guard", "--seed", "42"]
    with game_process(client, home, flags) as game:
        setup(game, (
            "fill -3 148 -3 3 156 3 minecraft:bedrock",
            "fill -1 151 -1 1 154 1 minecraft:air",
            "setblock 1 151 0 minecraft:obsidian",
            "tp @s 0.5 151 0.5 -90 0",
        ))
        local_command(game, "item replace entity @s hotbar.1 with minecraft:cobblestone 16")
        game.run("wait 5")
        command(game, "#dihrill forever")
        game.wait_for("timer ends and the miner stops",
                      lambda: "Run timer finished" in status(game), timeout=35)
        game.wait_for("shelter roof is placed",
                      lambda: has_block(game, (0, 153, 0), "cobblestone"), timeout=15)
        assert all(has_block(game, (0, y, 0), "air") for y in (151, 152))
        assert has_block(game, (0, 153, 0), "cobblestone")
        assert has_block(game, (1, 151, 0), "obsidian")
        walls = [(x, y, z) for x, z in ((-1, 0), (1, 0), (0, -1), (0, 1))
                 for y in (151, 152) if (x, y, z) != (1, 151, 0)]
        assert all(has_block(game, pos, "cobblestone") for pos in walls)
        assert player(game)["health"] == 20
        print("PASS: timer expiry enclosed the player's air cells with four walls and a roof.",
              flush=True)


def remote_shelter(game, console, name):
    command(game, "#dihrill stop")
    for line in (
        "fill 5 148 21 11 153 27 minecraft:bedrock",
        "fill 7 150 24 9 151 24 minecraft:air",
        "setblock 9 150 24 minecraft:obsidian",
        f"tp {name} 8.5 150 24.5 -90 0",
        f"item replace entity {name} hotbar.1 with minecraft:cobblestone 16",
    ):
        console(line)
    game.wait_for("arrive at the shelter fixture",
                  lambda: player(game)["block_pos"] == [8, 150, 24], timeout=10)

    game.run("options-set guiScale 1")
    game.run("key key.keyboard.f8 tap")
    game.wait_for("timer menu opens", lambda: game.run("screen")["screen"] == "mod", timeout=10)
    game.run("click-at 405 192")
    time.sleep(0.25)
    game.run("click-at 575 251")
    time.sleep(0.25)
    game.run("type 2s")
    game.run("key key.keyboard.enter tap")
    game.wait_for("timer saved", lambda: json.loads(config_file(game.home).read_text(encoding="utf8"))
                  .get("run_seconds") == 2, timeout=10)
    game.run("key key.keyboard.escape tap")
    game.wait_for("return to gameplay", lambda: game.run("screen")["screen"] == "playing", timeout=10)
    command(game, "#dihrill forever obsidian")
    game.wait_for("timer ends remotely", lambda: "Run timer finished" in status(game), timeout=15)

    walls = ((7, 150, 24), (7, 151, 24), (9, 151, 24))
    game.wait_for("shelter walls become visible on the client",
                  lambda: all(has_block(game, pos, "cobblestone") for pos in walls), timeout=15)
    expected = [(pos, "cobblestone") for pos in walls] + [
        ((8, 150, 24), "air"), ((8, 151, 24), "air"), ((8, 152, 24), "bedrock"),
    ]
    for (x, y, z), block in expected:
        reply = console(f"execute if block {x} {y} {z} minecraft:{block}")
        assert "Test passed" in str(reply), reply
    assert player(game)["health"] == 20
    print("PASS: the loopback server confirmed a sealed shelter and clear player cells after the timer.",
          flush=True)


def remote_mobs(client, run, baseline=False):
    from shop_smoke import TITLE, loopback_shop

    port = free_port()
    name = "ShopQA"
    server_home = run / "server"
    server_home.mkdir(parents=True)
    (server_home / "eula.txt").write_text("eula=true\n", encoding="ascii")
    (server_home / "server.properties").write_text(
        "server-ip=127.0.0.1\nonline-mode=false\nspawn-protection=0\n"
        "white-list=false\nenforce-whitelist=false\ndifficulty=normal\n"
        "view-distance=4\nsimulation-distance=4\n", encoding="ascii")
    with game_process(INSTALLED / "horizon-client.exe", server_home,
                      ["--test-terrain", "--dedicated", "--nogui", "--port", str(port)],
                      "console stop") as server:
        def console(line):
            return server.run(f"console {line}")

        home = run / "remote"
        shop_context = nullcontext((None, port)) if baseline else loopback_shop(port, server)
        with shop_context as (shop, client_port):
            address = f"127.0.0.1:{client_port}"
            options = {}
            if shop is not None:
                options = {
                    "diamond_shop": {"address": address, "title": TITLE, "menu": "minecraft:generic_9x3"},
                    "shop_min_diamonds": sum(shop.stacks),
                    "shop_interval_seconds": 2,
                }
            seed_config(home, profile="hunter", explore=False, avoid_mobs=True,
                        radius=28, mob_radius=6, targets=["minecraft:diamond_ore"], **options)
            with game_process(client, home, CLIENT_FLAGS + ["--server", address, "--name", name]) as game:
                ready(game)
                for line in (
                    "gamerule spawn_mobs false",
                    *crossed_tunnels(150),
                    f"tp {name} 0.5 150 0.5 -90 0",
                    f"item replace entity {name} hotbar.0 with minecraft:netherite_pickaxe",
                    f"item replace entity {name} armor.chest with minecraft:netherite_chestplate",
                    f"item replace entity {name} weapon.offhand with minecraft:totem_of_undying",
                ):
                    console(line)
                game.wait_for("fixture corridor", lambda: has_block(game, (20, 150, 0), "diamond_ore"))
                game.run("wait 10")

                if baseline:
                    command(game, "#dihrill forever")
                    command(game, "#dihrill pause")
                    assert "paused" in status(game).lower()
                    console("summon minecraft:cave_spider 4.5 150 0.5")
                    game.wait_for("cave spider wounds the connected, paused player",
                                  lambda: player(game)["health"] < 20, timeout=20)
                    print("REPRODUCED: v0.9.2 stayed connected while paused and the spider dealt damage.",
                          flush=True)
                    return

                approach_and_retreat(game, console, 150, "cave_spider")
                assert shop is not None
                for index, count in enumerate(shop.stacks[:2]):
                    console(f"item replace entity {name} inventory.{index} with minecraft:diamond {count}")
                game.wait_for("make a sale after retreating", lambda: "applied" in shop.log, timeout=35)
                assert shop.log == ["open", (27, 0, 0), (28, 0, 0), "close", "applied"], shop.log
                check_alternate_tunnel(game, 150)
                for slot, item in (
                    ("hotbar.0", "netherite_pickaxe"),
                    ("armor.chest", "netherite_chestplate"),
                    ("weapon.offhand", "totem_of_undying"),
                ):
                    proof = console(f"execute if items entity {name} {slot} minecraft:{item}")
                    assert "Test passed" in str(proof), proof
                print("PASS: avoided the cave spider, stayed connected, sold diamonds, and resumed mining"
                      " in the other tunnel at full health with gear and totem retained.", flush=True)
                remote_shelter(game, console, name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--case", choices=["all", "cave", "shaft", "mobs", "remote", "shelter", "chase",
                                           "baseline"], default="all")
    options = parser.parse_args()
    options.out.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="dihrill-safety-", dir=options.out.resolve()))
    print(directory, flush=True)
    if options.case == "baseline":
        remote_mobs(options.client.resolve(), directory / "baseline", baseline=True)
    for name, check in (("cave", cave), ("shaft", shaft), ("mobs", mobs), ("remote", remote_mobs),
                        ("shelter", shelter), ("chase", chase)):
        if options.case in ("all", name):
            check(options.client.resolve(), directory / name)
