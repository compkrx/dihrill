"""Check continued walking and camera turns during a real Dihrill route."""
import argparse
import json
from pathlib import Path
import tempfile
import time

from extended_smoke import command, config_file, has_block, local_command, seed_config
from smoke import CLIENT_FLAGS, game_process, ready


def visible_vein(client, root):
    legacy = config_file(root).parent.with_name("ore_miner") / "settings.json"
    legacy.parent.mkdir(parents=True)
    legacy.write_text(json.dumps({"targets": ["minecraft:diamond_ore"], "explore": False, "radius": 16}),
                      encoding="utf-8")
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Vein Reach QA", "--seed", "42"]
    with game_process(client, root, flags) as game:
        ready(game)
        for line in (
            "fill -3 100 -3 10 106 3 minecraft:bedrock",
            "fill 0 101 -1 7 102 1 minecraft:air",
            "setblock 3 101 0 minecraft:diamond_ore",
            "setblock 3 102 0 minecraft:diamond_ore",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe",
            "tp @s 0.5 101 0.5 -90 0",
            "gamemode survival @s",
        ):
            local_command(game, line)
        game.run("wait 20")
        assert "1 targets selected" in command(game, "/dihrill status")
        assert json.loads(config_file(root).read_text(encoding="utf-8"))["radius"] == 16
        command(game, "#dihrill diamond 2")

        def mined_from_start():
            x, _, z = game.run("state")["player"]["position"]
            if all(has_block(game, (3, y, 0), "air") for y in (101, 102)):
                return True
            assert abs(x - 0.5) < 0.2 and abs(z - 0.5) < 0.2, (x, z)
            return False

        game.wait_for("both visible faces mined before walking", mined_from_start, timeout=25)
        game.wait_for("both drops picked up", lambda:
                      game.run("inventory")["counts"].get("minecraft:diamond") == 2, timeout=20)
        print("PASS: imported old settings, mined both exposed faces before walking, and collected both drops.", flush=True)


def check(client, root):
    seed_config(root, targets=["minecraft:diamond_ore"], explore=False,
                radius=24, sprint=True, profile="cave", turn_speed=12)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill turns QA", "--seed", "42"]
    with game_process(client, root, flags) as game:
        ready(game)
        for text in (
            "fill -2 100 -2 18 104 10 minecraft:bedrock",
            "fill 0 101 0 13 102 0 minecraft:air",
            "fill 13 101 0 13 102 8 minecraft:air",
            "setblock 13 101 7 minecraft:diamond_ore",
            "setblock 13 102 7 minecraft:diamond_ore",
            "setblock 13 101 8 minecraft:diamond_ore",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe",
            "tp @s 0.5 101 0.5 -90 0",
            "gamemode survival @s",
        ):
            local_command(game, text)
        game.run("wait 20")
        command(game, "#dihrill forever")
        trace = []
        until = time.monotonic() + 35
        ore = ((13,101,7), (13,102,7), (13,101,8))
        while time.monotonic() < until:
            st = game.run("state")
            player = st["player"]
            trace.append(player)
            if len(trace) % 12 == 0 and all(has_block(game, p, "air") for p in ore):
                break
            time.sleep(0.039)
        (root / "motion-trace.json").write_text(json.dumps(trace), encoding="utf8")
        assert all(has_block(game, p, "air") for p in ore), command(game, "#dihrill status")
        straight = [p for p in trace if 2.0 < p["position"][0] < 11.8]
        assert len(straight) > 10, len(straight)
        stopped = sum(abs(p["velocity"][0]) < 0.055 for p in straight)
        assert stopped / len(straight) < 0.12, (stopped, len(straight))
        assert all(abs(p["position"][2] - 0.5) < 0.23 for p in straight)
        sweeps = [
            abs((b["yaw"] - a["yaw"] + 180) % 360 - 180)
            for a, b in zip(trace, trace[1:]) if a["yaw"] != b["yaw"]
        ]
        assert len(sweeps) >= 8 and max(sweeps) <= 30, sweeps
        assert all(p["on_ground"] for p in trace)
        print(f"PASS: {len(straight)} straight tunnel samples, {stopped} near-stops, "
              f"largest sampled camera turn {max(sweeps):.1f} degrees, three ore blocks mined.",
              flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    options = parser.parse_args()
    options.out.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix="dihrill-motion-", dir=options.out.resolve()))
    print(folder, flush=True)
    visible_vein(options.client.resolve(), folder / "vein")
    check(options.client.resolve(), folder / "movement")
