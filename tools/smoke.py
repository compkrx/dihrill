"""Exercise Dihrill in a scratch Horizon instance using the real game controls."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time


INSTALLED = (
    Path(os.environ["APPDATA"])
    / "horizon-launcher/versions/nightly-2026-11-13-9f42064"
)
RESOURCES = Path(os.environ.get("DIHRILL_TEST_RESOURCES", INSTALLED / ".local/horizon-client/26.3"))


class Game:
    def __init__(self, executable, home, log, arguments):
        self.endpoint = home / "control.json"
        self.home = home
        env = os.environ | {
            "HORIZON_HOME": str(home / "app"),
            "HORIZON_LAUNCHER_HOME": str(home / "launcher"),
            "XDG_DATA_HOME": str(home / "data"),
            "XDG_CONFIG_HOME": str(home / "config"),
            "HORIZON_LAUNCHER_NO_SELF_UPDATE": "1",
            "HORIZON_NO_AUDIO": "1",
            "HORIZON_NO_DISCORD": "1",
            "HORIZON_TELEMETRY_URL": "off",
            "RUST_LOG": "warn",
        }
        self.process = subprocess.Popen(
            [
                str(executable), "--game-dir", str(home), "--resources", str(RESOURCES),
                "--control", "--control-socket", str(self.endpoint), *arguments,
            ],
            cwd=home,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )

    def run(self, line, timeout=10):
        if self.process.poll() is not None:
            raise RuntimeError(f"Horizon exited with code {self.process.returncode}")
        endpoint = json.loads(self.endpoint.read_text(encoding="utf-8"))
        with socket.create_connection(("127.0.0.1", endpoint["port"]), timeout=timeout) as pipe:
            with pipe.makefile("rwb") as stream:
                stream.write(f"auth {endpoint['token']}\n{line}\n".encode("utf-8"))
                stream.flush()
                assert json.loads(stream.readline())["ok"]
                reply = json.loads(stream.readline())
        if "error" in reply:
            raise RuntimeError(f"{line}: {reply['error']}")
        return reply

    def wait_for(self, label, test, timeout=90):
        deadline = time.monotonic() + timeout
        failure = None
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"{label}: Horizon exited with code {self.process.returncode}")
            try:
                result = test()
                if result:
                    return result
            except (OSError, ValueError, RuntimeError) as error:
                failure = error
            time.sleep(0.3)
        raise RuntimeError(f"{label} did not become ready: {failure}")

    def finish(self, line="quit"):
        if self.process.poll() is not None:
            return
        try:
            self.run(line)
            self.process.wait(timeout=25)
        except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired):
            self.process.terminate()
            self.process.wait(timeout=10)


@contextmanager
def game_process(executable, home, arguments, quit_command="quit"):
    home.mkdir(parents=True, exist_ok=True)
    with (home / f"runtime-{time.time_ns()}.log").open("w", encoding="utf-8") as log:
        game = Game(executable, home, log, arguments)
        try:
            ready_command = "server-status" if quit_command == "console stop" else "hello"
            game.wait_for("control", lambda: game.run(ready_command, timeout=2))
            yield game
        finally:
            game.finish(quit_command)


CLIENT_FLAGS = [
    "--offscreen", "--no-pause", "--width", "960", "--height", "600",
    "--render-distance", "4",
]


def ready(game):
    game.wait_for("spawn", lambda: game.run("state")["player"])
    game.run("wait 20", timeout=30)



def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def prove(game, cmd, who, check_inventory):
    ready(game)
    commands = (
        # One corridor with a stone/deepslate plug. Three unsafe targets
        # sit above, beside or behind lava, water and gravel respectively.
        "fill -3 84 -3 13 91 3 minecraft:bedrock",
        "fill -2 86 0 10 87 0 minecraft:air",
        "setblock 3 86 0 minecraft:stone",
        "setblock 3 87 0 minecraft:deepslate",
        "setblock 7 86 0 minecraft:diamond_ore",
        "setblock 8 87 0 minecraft:deepslate_diamond_ore",
        "setblock 10 86 0 minecraft:diamond_ore",
        "setblock 9 87 0 minecraft:bedrock",
        "setblock 10 87 0 minecraft:water",
        "setblock 5 86 2 minecraft:diamond_ore",
        "setblock 5 87 2 minecraft:gravel",
        "setblock 4 87 -1 minecraft:diamond_ore",
        "setblock 4 88 -1 minecraft:lava",
        "setblock 8 86 1 minecraft:coal_ore",
        f"tp {who} 0.5 86 0.5 -90 0",
        f"gamemode survival {who}",
        f"clear {who}",
        f"item replace entity {who} hotbar.0 with minecraft:diamond_pickaxe",
        f"item replace entity {who} inventory.0 with minecraft:cobblestone 64",
        f"item replace entity {who} inventory.1 with minecraft:cobbled_deepslate 64",
        f"item replace entity {who} hotbar.8 with minecraft:cobbled_deepslate 23",
    )
    for c in commands:
        cmd(c)
    for n in range(2,27):
        cmd(f"item replace entity {who} inventory.{n} with minecraft:apple 1")
    for n in range(1,8):
        cmd(f"item replace entity {who} hotbar.{n} with minecraft:apple 1")
    game.run("wait 20", timeout=30)
    assert all(slot["item"] for slot in game.run("inventory")["main"])
    # Logical UI coordinates for the 960 x 600 test viewport at GUI scale 1.
    def click(x, y):
        game.run(f"click-at {x} {y}")
        # The native frame hook consumes the click on its next frame. A
        # control-socket wait would claim the game agent and pause Dihrill.
        time.sleep(0.2)

    game.run("options-set guiScale 1")
    game.run("open-inventory")
    game.run("wait 3")
    click(335, 224)
    game.wait_for("menu from inventory", lambda: game.run("screen")["screen"] == "mod",
                  timeout=5)
    settings_file = game.home / "horizon-mods/.horizon-mod-data/dihrill/settings.json"

    def saved():
        return json.loads(settings_file.read_text(encoding="utf-8"))

    click(456, 214)  # All blocks
    game.run("type gravel")
    game.run("wait 3")
    click(335, 257)
    assert not settings_file.exists(), "Gravel appeared as a selectable target"
    click(652, 235)
    game.run("type minecraft:glass")
    game.run("wait 3")
    click(335, 257)
    assert saved()["targets"] == ["minecraft:glass"], saved()
    assert saved()["junk"] == [], "Junk disposal must be opt-in"
    click(335, 257)
    assert saved()["targets"] == [], saved()
    click(652, 235)
    click(325, 214)  # Ores
    game.run("type diamond")
    game.run("wait 3")
    click(335, 257)
    click(335, 275)
    assert saved()["targets"] == ["minecraft:deepslate_diamond_ore", "minecraft:diamond_ore"], saved()
    click(590, 214)  # Priority order
    click(633, 257)  # Move first row down
    assert saved()["targets"] == ["minecraft:diamond_ore", "minecraft:deepslate_diamond_ore"], saved()
    game.run(f"screenshot {game.home.parent / (game.home.name + '-picker.png')}", timeout=20)

    click(555, 192)  # Inventory rules
    click(326, 228)  # Cobblestone
    click(462, 228)  # Cobbled deepslate
    assert set(saved()["junk"]) == {"minecraft:cobblestone", "minecraft:cobbled_deepslate"}, saved()

    click(400, 192)  # Mining settings
    click(455, 270)  # Block count
    if game.run("screen")["screen"] != "mod":
        raise AssertionError("Settings left the miner panel")
    game.run("type 2")
    game.run("key enter tap")
    game.run("wait 3")
    assert saved()["goal"] == {"blocks": 2}, saved()
    click(316, 408)  # Start
    game.wait_for("menu closes on Start", lambda: game.run("screen")["screen"] == "playing")
    game.run("key key.keyboard.f8 tap")
    game.wait_for("F8 opens running miner", lambda: game.run("screen")["screen"] == "mod", timeout=10)
    click(419, 408)  # Pause
    click(505, 408)  # Resume
    game.wait_for("resume closes the menu", lambda: game.run("screen")["screen"] == "playing", timeout=10)
    game.wait_for("diamond pickups", lambda: game.run("inventory")["counts"].get("minecraft:diamond",0) >= 2,
                  timeout=45)
    for x,y,z,name in (
        (3,86,0,"air"), (3,87,0,"air"), (7,86,0,"air"), (8,87,0,"air"),
        (10,86,0,"diamond_ore"), (5,86,2,"diamond_ore"), (5,87,2,"gravel"),
        (4,87,-1,"diamond_ore"), (8,86,1,"coal_ore"),
    ):
        reply=game.run(f"block {x} {y} {z}")
        assert f"minecraft:{name}" in json.dumps(reply), (name,reply)
    inv = game.run("inventory")
    assert inv["counts"]["minecraft:diamond"] >= 2
    assert inv["counts"]["minecraft:apple"] == 32
    junk = ("minecraft:cobblestone", "minecraft:cobbled_deepslate")
    assert sum(inv["counts"].get(k, 0) for k in junk) <= 2, inv["counts"]
    thrown = [entity for entity in game.run("entities 24 item")["entities"]
              if entity["item"]["item"] in junk and entity["pos"][0] < 0.0]
    assert sum(entity["item"]["count"] for entity in thrown) >= 151, thrown
    check_inventory()
    game.run("key key.keyboard.f8 tap")
    game.wait_for("F8 opens the picker", lambda: game.run("screen")["screen"] == "mod", timeout=10)
    game.run("key key.keyboard.escape tap")
    game.wait_for("Escape closes the picker", lambda: game.run("screen")["screen"] == "playing", timeout=10)
    print("PASS: menu priority, quantity, pause/resume, whole-inventory junk cleanup, tunnel mining, and lava/water/gravel avoidance.", flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client",type=Path)
    parser.add_argument("--out",type=Path,required=True)
    parser.add_argument("--case",choices=("local","remote","all"),default="all")
    opts=parser.parse_args()
    exe=opts.client.resolve()
    opts.out.mkdir(parents=True,exist_ok=True)
    folder=Path(tempfile.mkdtemp(prefix="ore-miner-proof-",dir=opts.out.resolve()))
    print(folder,flush=True)
    if opts.case != "remote":
        with game_process(exe,folder/"local",CLIENT_FLAGS+["--test-terrain","--world","Ore Miner QA","--seed","42"]) as g:
            prove(g,lambda c:g.run(f"command {c}"),"@s",lambda:None)
    if opts.case != "local":
        port=free_port()
        h=folder/"server"
        h.mkdir()
        (h/"eula.txt").write_text("eula=true\n",encoding="ascii")
        (h/"server.properties").write_text(
            "server-ip=127.0.0.1\nonline-mode=false\nspawn-protection=0\n"
            "white-list=false\nenforce-whitelist=false\nview-distance=4\n"
            "simulation-distance=3\npause-when-empty-seconds=0\n",encoding="ascii")
        with game_process(INSTALLED/"horizon-client.exe",h,
            ["--test-terrain","--dedicated","--nogui","--port",str(port)],"console stop") as s:
            with game_process(exe,folder/"remote",CLIENT_FLAGS+
                ["--server",f"127.0.0.1:{port}","--name","OreMinerQA"]) as g:
                def verify():
                    inv=json.dumps(s.run("console data get entity OreMinerQA Inventory"))
                    assert "minecraft:diamond" in inv,inv
                    print("Stock dedicated server confirmed the diamond in its inventory.",flush=True)
                prove(g,lambda c:s.run(f"console {c}"),"OreMinerQA",verify)


if __name__ == "__main__":
    main()


