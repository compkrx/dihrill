"""Exercise unlimited rescans, the walking finder, module controls and keybinds."""

import argparse
import json
from pathlib import Path
import tempfile
import time

from extended_smoke import command, config_file, has_block, local_command, seed_config, status
from safety_smoke import setup
from smoke import CLIENT_FLAGS, game_process, module_settings, ui_click


def check(client, home):
    seed_config(home, targets=["minecraft:diamond_ore"], explore=False, avoid_caves=False)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Utility QA", "--seed", "42"]
    with game_process(client, home, flags) as game:
        setup(game, (
            "fill -2 149 -3 15 155 3 minecraft:bedrock",
            "fill 0 151 0 13 152 0 minecraft:air",
            "fill 4 151 0 4 152 0 minecraft:bedrock",
            "setblock 7 151 0 minecraft:diamond_ore",
            "tp @s 0.5 151 0.5 -90 0",
        ))
        local_command(game, "item replace entity @s hotbar.1 with minecraft:cobblestone 24")
        command(game, "#dihrill forever")
        game.wait_for("route search waits with an inaccessible target",
                      lambda: "Waiting to rescan" in status(game), timeout=25)
        time.sleep(3)
        assert has_block(game, (1,151,0), "air")
        assert has_block(game, (1,152,0), "air")
        local_command(game, "fill 4 151 0 4 152 0 minecraft:air")
        game.wait_for("unlimited miner resumes when the route opens",
                      lambda: "1 target blocks mined" in status(game) and has_block(game, (7,151,0), "air"),
                      timeout=32)
        command(game, "#dihrill stop")
        print("PASS: an unavailable route waited and resumed with the same job. No shelter blocks were placed.",
              flush=True)

        local_command(game, "setblock 10 151 0 minecraft:diamond_ore")
        local_command(game, "item replace entity @s hotbar.0 with minecraft:air")
        local_command(game, "tp @s 0.5 151 0.5 -90 0")
        game.run("options-set guiScale 1")
        game.run("key key.keyboard.f8 tap")
        game.wait_for("module dashboard", lambda: game.run("screen")["screen"] == "mod")
        ui_click(game, 318, 235, right=True)
        game.run(f"screenshot {home / 'modules.png'}", timeout=20)
        ui_click(game, 318, 235, right=True)
        ui_click(game, 318, 235)
        game.wait_for("finder arrives without mining",
                      lambda: "Found diamond_ore at 10, 151, 0" in status(game), timeout=32)
        assert has_block(game, (10,151,0), "diamond_ore")
        assert game.run("state")["player"]["position"][0] > 7.0
        print("PASS: the menu started a walk-only finder. It reached the ore without a tool or any block changes.",
              flush=True)

        game.run("key key.keyboard.f8 tap")
        game.wait_for("module dashboard reopens", lambda: game.run("screen")["screen"] == "mod")
        module_settings(game, "hud")
        ui_click(game, 606, 252)
        ui_click(game, 605, 216)
        hud = json.loads(config_file(home).read_text(encoding="utf-8"))["hud"]
        assert hud["position"] == "top_right" and hud["destination"], hud

        module_settings(game, "bindings")
        ui_click(game, 512, 216)
        game.run("key key.keyboard.f7 tap")
        time.sleep(0.25)
        ui_click(game, 512, 261)
        game.run("key key.keyboard.f6 tap")
        time.sleep(0.25)
        game.run("key key.keyboard.escape tap")
        game.wait_for("settings close", lambda: game.run("screen")["screen"] == "playing")
        game.run("key key.keyboard.f8 tap")
        time.sleep(0.25)
        assert game.run("screen")["screen"] == "playing"
        game.run("key key.keyboard.f6 tap")
        game.wait_for("custom HUD key toggles the persisted setting", lambda:
                      not json.loads(config_file(home).read_text(encoding="utf-8"))["hud"]["visible"])
        game.run("key key.keyboard.f7 tap")
        game.wait_for("custom menu key opens Dihrill", lambda: game.run("screen")["screen"] == "mod")
        print("PASS: right-click settings, HUD controls and custom menu/HUD keys worked in the client.",
              flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(prefix="dihrill-utility-", dir=args.out.resolve()))
    print(run, flush=True)
    check(args.client.resolve(), run)
