"""Exercise both directions of vertical digging through Horizon's normal controls."""
import argparse
from pathlib import Path
import tempfile
import time

from extended_smoke import has_block, seed_config, status
from smoke import CLIENT_FLAGS, INSTALLED, free_port, game_process, module_settings, ready, ui_click


def start_shaft(game, direction, length):
    game.run("key key.keyboard.f8 tap")
    game.wait_for("Dihrill menu opens", lambda: game.run("screen")["screen"] == "mod")
    module_settings(game, "vertical")
    ui_click(game, 478, 268)
    game.run(f"type {length}")
    game.run("key key.keyboard.enter tap")
    time.sleep(0.25)
    x = 390 if direction == "down" else 560
    game.run(f"click-at {x} 292")
    game.wait_for("vertical digging starts", lambda: game.run("screen")["screen"] == "playing")


def setup(game, send, player):
    ready(game)
    game.run("options-set guiScale 1")
    for line in (
        "fill -4 138 -4 20 164 4 minecraft:bedrock",
        "fill 0 145 0 0 149 0 minecraft:deepslate",
        "setblock 0 148 0 minecraft:deepslate_diamond_ore",
        "fill 0 150 0 0 151 0 minecraft:air",
        "fill 4 149 0 4 155 0 minecraft:deepslate",
        "fill 4 150 0 4 151 0 minecraft:air",
        "setblock 4 153 0 minecraft:deepslate_diamond_ore",
        "fill 8 145 0 8 149 0 minecraft:deepslate",
        "fill 8 150 0 8 151 0 minecraft:air",
        "setblock 8 148 0 minecraft:lava",
        "fill 10 145 0 10 149 0 minecraft:deepslate",
        "fill 10 150 0 10 151 0 minecraft:air",
        "setblock 10 148 0 minecraft:air",
        "fill 12 149 0 12 154 0 minecraft:deepslate",
        "fill 12 150 0 12 151 0 minecraft:air",
        "setblock 12 153 0 minecraft:gravel",
        "fill 17 149 0 17 154 0 minecraft:deepslate",
        "fill 17 150 0 17 151 0 minecraft:air",
        "setblock 18 152 0 minecraft:water",
        f"clear {player}",
        f"item replace entity {player} hotbar.0 with minecraft:diamond_pickaxe",
        f"item replace entity {player} hotbar.1 with minecraft:cobblestone 8",
        f"gamemode survival {player}",
        "gamerule spawn_mobs false",
    ):
        send(line)


def check(game, send, player, hazards):
    setup(game, send, player)

    def teleport(x):
        send(f"tp {player} {x}.5 150 0.5 -90 0")
        game.wait_for("centered position",
                      lambda: abs(game.run("state")["player"]["position"][0] - x - 0.5) < 0.02)
        game.run("wait 4")

    def wait_for_status(label):
        try:
            game.wait_for(label, lambda: label in status(game), timeout=30)
        except RuntimeError as error:
            raise RuntimeError(status(game)) from error

    teleport(0)
    start_shaft(game, "down", 2)
    wait_for_status("Vertical digging finished")
    assert abs(game.run("state")["player"]["position"][1] - 148) < 0.05
    assert has_block(game, (0, 147, 0), "deepslate")
    assert game.run("inventory")["counts"].get("minecraft:diamond") == 1

    teleport(4)
    start_shaft(game, "up", 3)
    wait_for_status("Vertical digging finished")
    assert abs(game.run("state")["player"]["position"][1] - 153) < 0.05
    assert all(has_block(game, (4, y, 0), "cobblestone") for y in range(150, 153))
    assert game.run("inventory")["counts"].get("minecraft:diamond") == 2
    print("PASS: two levels down, three levels up, both diamond drops collected, and exact shaft lengths.", flush=True)

    if hazards:
        for x, direction in ((8, "down"), (10, "down"), (12, "up"), (17, "up")):
            teleport(x)
            start_shaft(game, direction, 1)
            wait_for_status("Vertical digging paused for unsafe ground")
            assert has_block(game, (x, 149 if direction == "down" else 152, 0), "deepslate")
            game.run("chat #dihrill stop")
        assert game.run("state")["player"]["health"] == 20
        print("PASS: left shaft blocks intact above lava and a drop, below gravel, and beside water.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--case", choices=["local", "remote", "all"], default="all")
    options = parser.parse_args()
    options.out.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(prefix="dihrill-vertical-", dir=options.out.resolve()))
    print(run, flush=True)
    client = options.client.resolve()
    settings = dict(targets=["minecraft:deepslate_diamond_ore"], avoid_caves=True)
    if options.case != "remote":
        home = run / "local"
        seed_config(home, **settings)
        flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Vertical QA", "--seed", "42"]
        with game_process(client, home, flags) as game:
            check(game, lambda line: game.run(f"command {line}"), "@s", hazards=True)
    if options.case != "local":
        port = free_port()
        server_home = run / "server"
        server_home.mkdir()
        (server_home / "eula.txt").write_text("eula=true\n", encoding="ascii")
        (server_home / "server.properties").write_text(
            "server-ip=127.0.0.1\nonline-mode=false\nspawn-protection=0\n"
            "white-list=false\nview-distance=4\nsteady-tick-rate=20\n", encoding="ascii")
        with game_process(INSTALLED / "horizon-client.exe", server_home,
                          ["--test-terrain", "--dedicated", "--nogui", "--port", str(port)],
                          "console stop") as server:
            home = run / "remote"
            seed_config(home, **settings)
            player = "ShaftQA"
            flags = CLIENT_FLAGS + ["--server", f"127.0.0.1:{port}", "--name", player]
            with game_process(client, home, flags) as game:
                check(game, lambda line: server.run(f"console {line}"), player, hazards=False)
                assert "minecraft:diamond" in str(server.run(f"console data get entity {player} Inventory"))
                print("PASS: stock server accepted real mining, pillar placement, and both pickups.", flush=True)


if __name__ == "__main__":
    main()
