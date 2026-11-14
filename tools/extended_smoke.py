"""Run the Dihrill command, vein, exploration, and session integration checks."""

import argparse
import json
from pathlib import Path
import tempfile
import time

from smoke import CLIENT_FLAGS, INSTALLED, free_port, game_process, ready


def config_file(game_dir):
    return game_dir / "horizon-mods/.horizon-mod-data/dihrill/settings.json"


def seed_config(game_dir, **overrides):
    target = config_file(game_dir)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(overrides), encoding="utf-8")


def command(game, text):
    reply = game.run("chat " + text)
    messages = [line for line in reply["chat"] if "[Dihrill]" in line]
    assert messages, (text, reply)
    return messages[0]


def status(game):
    return command(game, "#dihrill status")


def block(game, position):
    x, y, z = position
    reply = game.run(f"block {x} {y} {z}")
    return reply.get("block", json.dumps(reply))


def has_block(game, position, name):
    value = block(game,position)
    return f"minecraft:{name}" in str(value)


def local_command(game, text):
    return game.run("command " + text)


def setup_vein(game):
    for text in (
        "fill -3 97 -5 17 104 5 minecraft:bedrock",
        "fill 0 100 -3 14 101 3 minecraft:air",
        "setblock 3 100 0 minecraft:diamond_ore",
        "setblock 4 100 0 minecraft:diamond_ore",
        "setblock 5 100 0 minecraft:diamond_ore",
        "setblock 5 100 2 minecraft:diamond_ore",
        "setblock 2 101 -2 minecraft:iron_ore",
        "setblock 7 100 -2 minecraft:coal_ore",
        "tp @s 0.5 100 0.5 -90 0",
        "gamemode survival @s",
        "clear @s",
        "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe[minecraft:damage=1555]",
        "item replace entity @s hotbar.1 with minecraft:iron_pickaxe",
        "gamerule spawn_mobs false",
    ):
        local_command(game,text)
    game.run("wait 20", timeout=30)


def local_vein_and_commands(executable, run):
    home = run / "vein"
    seed_config(home, explore=False, radius=16, diagonals=False, durability=8, avoid_caves=False)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Vein QA", "--seed", "42"]
    with game_process(executable,home,flags) as game:
        ready(game)
        setup_vein(game)
        assert "targets selected" in command(game, "/dihrill status")
        assert "Unknown block" in command(game,"#dihrill no_such_ore_627")
        assert "quantity" in command(game,"#dihrill stone 0").lower()
        assert "Dihrill cannot safely mine" in command(game,"#dihrill gravel")
        game.run("key key.keyboard.t tap")
        game.run("type #dihrill diamond_or")
        game.run("key key.keyboard.tab tap")
        assert game.run("screen")["chat_input"] == "#dihrill diamond_ore"
        game.run("key key.keyboard.escape tap")
        command(game,"#dihrill settings")
        game.wait_for("settings opens", lambda: game.run("screen")["screen"] == "mod", timeout=8)
        game.run("key key.keyboard.escape tap")
        game.wait_for("settings closes", lambda: game.run("screen")["screen"] == "playing", timeout=8)
        command(game,"#dihrill #minecraft:diamond_ores iron_ore 3")
        command(game,"#dihrill pause")
        assert "paus" in status(game).lower(), status(game)
        time.sleep(1)
        assert has_block(game,[3,100,0],"diamond_ore")
        command(game,"#dihrill resume")
        game.wait_for("vein quantity", lambda: "3 target blocks mined" in status(game),
                      timeout=50)
        for x in (3,4,5):
            assert has_block(game,[x,100,0],"air"), block(game,[x,100,0])
        assert has_block(game,[5,100,2],"diamond_ore")
        assert has_block(game,[2,101,-2],"iron_ore")
        inventory = game.run("inventory")
        assert inventory["hotbar"][0].get("damage") == 1555, inventory["hotbar"][0]
        assert inventory["hotbar"][1].get("damage",0) >= 3, inventory["hotbar"][1]
        print("PASS: hash commands, tag expansion, Tab completion, settings, pause/resume, vein priority, durability and exact block quota",flush=True)
        command(game,"#dihrill stop")
        local_command(game, "setblock 7 100 -1 minecraft:stone")
        local_command(game, "setblock 8 100 -1 minecraft:stone")
        local_command(game, "setblock 9 100 -1 minecraft:stone")
        command(game,"#dihrill stone 2")
        game.wait_for("stone quota",lambda: "2 target blocks mined" in status(game),timeout=30)
        broken = sum(has_block(game,[x,100,-1],"air") for x in (7,8,9))
        assert broken == 2, broken
        command(game,"#dihrill stop")
    with game_process(executable,home,flags) as game:
        ready(game)
        assert "1 targets selected" in status(game)
        persisted = json.loads(config_file(home).read_text(encoding="utf-8"))
        assert persisted["targets"] == ["minecraft:stone"], persisted
        assert persisted["goal"] == {"blocks": 2}, persisted
        assert persisted["junk"] == [], persisted
        print("PASS: configured targets and quantity persist after reopening; jobs start idle",flush=True)


def local_exploration(executable,run):
    home = run / "tunnel"
    seed_config(home, profile="tunnel", targets=["minecraft:diamond_ore"], radius=8,
                explore=True, explore_limit=20, turn_speed=18)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Tunnel QA", "--seed", "42"]
    with game_process(executable,home,flags) as game:
        ready(game)
        for text in (
            "fill -3 117 -4 23 125 4 minecraft:bedrock",
            "fill 0 120 0 3 121 0 minecraft:air",
            "fill 4 120 0 18 121 0 minecraft:stone",
            "setblock 13 120 0 minecraft:diamond_ore",
            "setblock 17 121 0 minecraft:lava",
            "setblock 0 120 -1 minecraft:gravel",
            "tp @s 0.5 120 0.5 -90 0",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:netherite_pickaxe",
        ):
            local_command(game,text)
        game.run("wait 20",timeout=30)
        command(game,"#dihrill forever")
        game.wait_for("ore beyond initial scan radius", lambda: has_block(game,[13,120,0],"air"),
                      timeout=75)
        command(game,"#dihrill stop")
        assert has_block(game,[4,120,0],"air")
        assert has_block(game,[16,121,0],"stone")
        assert has_block(game,[0,120,-1],"gravel")
        print("PASS: forward exploration extends a tunnel beyond the initial search radius",flush=True)


def local_continuous_exploration(executable, run):
    home = run / "continuous"
    seed_config(home, profile="tunnel", targets=["minecraft:diamond_ore"], radius=8,
                explore=True, explore_limit=8, continuous_explore=True, turn_speed=18)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Continuous QA", "--seed", "42"]
    with game_process(executable, home, flags) as game:
        ready(game)
        for line in (
            "fill -3 117 -3 25 125 3 minecraft:bedrock",
            "fill 0 120 0 2 121 0 minecraft:air",
            "fill 3 120 0 20 121 0 minecraft:stone",
            "setblock 20 120 0 minecraft:diamond_ore",
            "tp @s 0.5 120 0.5 -90 0",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:netherite_pickaxe[minecraft:enchantments={efficiency:5}]",
        ):
            local_command(game, line)
        game.run("wait 20", timeout=30)
        command(game, "#dihrill forever")
        game.wait_for("ore past the configured exploration limit",
                      lambda: game.run("inventory")["counts"].get("minecraft:diamond") == 1, timeout=90)
        assert has_block(game, (20, 120, 0), "air")
        assert has_block(game, (14, 121, 0), "air")
        command(game, "#dihrill stop")
        print("PASS: continuous exploration mined beyond the usual distance limit", flush=True)


def local_run_timer(executable, run):
    home = run / "timer"
    seed_config(home, profile="tunnel", targets=["minecraft:diamond_ore"], radius=8,
                explore=True, continuous_explore=True)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Timer QA", "--seed", "42"]
    with game_process(executable, home, flags) as game:
        ready(game)
        for line in (
            "fill -3 117 -3 48 125 3 minecraft:bedrock",
            "fill 0 120 0 2 121 0 minecraft:air",
            "fill 3 120 0 45 121 0 minecraft:stone",
            "tp @s 0.5 120 0.5 -90 0",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:iron_pickaxe",
        ):
            local_command(game, line)
        game.run("wait 20", timeout=30)
        game.run("options-set guiScale 1")
        game.run("key key.keyboard.f8 tap")
        game.wait_for("timer menu opens", lambda: game.run("screen")["screen"] == "mod")
        game.run("click-at 405 192")
        time.sleep(0.25)

        def open_timer():
            game.run("click-at 575 251")
            time.sleep(0.25)

        def duration_is(seconds):
            return json.loads(config_file(home).read_text(encoding="utf8")).get("run_seconds") == seconds

        def set_timer(value, seconds):
            open_timer()
            game.run(f"type {value}")
            game.run("key key.keyboard.enter tap")
            game.wait_for(f"duration {value} saved", lambda: duration_is(seconds))
            time.sleep(0.15)

        set_timer("23", 1380)
        set_timer("2:15:30", 8130)
        set_timer("3h5m17s", 11117)
        open_timer()
        game.run("type 4:65")
        game.run("key key.keyboard.enter tap")
        time.sleep(0.25)
        assert duration_is(11117)
        game.run("key key.keyboard.escape tap")
        time.sleep(0.25)
        open_timer()
        game.run("click-at 480 299")
        game.wait_for("timer turns off", lambda: duration_is(0))
        set_timer("31s", 31)
        game.run("key key.keyboard.escape tap")
        command(game, "#dihrill forever")
        game.wait_for("timer starts", lambda: "left" in status(game), timeout=3)
        game.run("key key.keyboard.f9 tap")
        game.wait_for("hotkey pauses", lambda: "Paused" in status(game), timeout=3)
        game.run("key key.keyboard.f9 tap")
        game.wait_for("hotkey resumes", lambda: "Paused" not in status(game), timeout=3)
        game.run("key key.keyboard.f8 tap")
        game.wait_for("waits while screen is open", lambda:
                      "Waiting for gameplay" in status(game), timeout=3)
        set_timer("0:06", 6)
        before = status(game)
        assert "Waiting for gameplay" in before and "6s left" in before, before
        time.sleep(2)
        assert status(game) == before
        game.run("key key.keyboard.escape tap")
        start = time.monotonic()
        game.wait_for("run timer expires", lambda: "Run timer finished" in status(game), timeout=10)
        assert 3.0 <= time.monotonic() - start <= 8.0
        assert has_block(game, (3, 120, 0), "air") or has_block(game, (3, 121, 0), "air")
        assert has_block(game, (20, 120, 0), "stone")
        print("PASS: custom timer formats, invalid input, disable, live edit, F9 pause/resume, and expiry", flush=True)


def local_cave(executable,run):
    home = run / "cave"
    seed_config(home, profile="cave", targets=["minecraft:diamond_ore"],
                radius=12, explore=False, max_fall=1, avoid_caves=False)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Cave QA", "--seed", "42"]
    with game_process(executable,home,flags) as game:
        ready(game)
        for text in (
            "fill -3 156 -5 15 165 6 minecraft:bedrock",
            "fill 0 160 0 3 162 0 minecraft:air",
            "fill 4 161 0 8 163 0 minecraft:air",
            "setblock 8 161 0 minecraft:diamond_ore",
            "setblock 3 160 1 minecraft:stone",
            "setblock 3 161 1 minecraft:stone",
            "setblock 3 160 2 minecraft:diamond_ore",
            "fill 2 157 -3 2 161 -1 minecraft:air",
            "setblock 2 157 -3 minecraft:diamond_ore",
            "tp @s 0.5 160 0.5 -90 0",
            "gamerule spawn_mobs false",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:iron_pickaxe",
        ):
            local_command(game,text)
        game.run("wait 20",timeout=30)
        command(game,"#dihrill forever")
        game.wait_for("cave route climbs a one-block step",lambda:
                      game.run("inventory")["counts"].get("minecraft:diamond",0) >= 1,
                      timeout=45)
        game.wait_for("cave search completes",lambda:
                      "stopped" in status(game).lower(),timeout=25)
        assert has_block(game,[8,161,0],"air")
        assert has_block(game,[3,160,1],"stone")
        assert has_block(game,[3,160,2],"diamond_ore")
        assert has_block(game,[2,157,-3],"diamond_ore")
        print("PASS: Cave Miner climbed a step and left the blocked and deep-drop targets alone",flush=True)


def local_tools(executable, run):
    for preference in ("silk", "fortune"):
        home = run / preference
        seed_config(home, tool=preference, explore=False, radius=8)
        flags = CLIENT_FLAGS + ["--test-terrain","--world","Dihrill Tool QA","--seed","42"]
        with game_process(executable,home,flags) as game:
            ready(game)
            for text in (
                "fill -3 157 -3 11 165 3 minecraft:bedrock",
                "fill 0 160 0 8 161 0 minecraft:air",
                "setblock 4 160 0 minecraft:diamond_ore",
                "tp @s 0.5 160 0.5 -90 0",
                "gamerule spawn_mobs false",
                "gamemode survival @s",
                "clear @s",
                "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe[minecraft:enchantments={efficiency:5}]",
                "item replace entity @s hotbar.1 with minecraft:iron_pickaxe[minecraft:enchantments={silk_touch:1}]",
                "item replace entity @s hotbar.2 with minecraft:iron_pickaxe[minecraft:enchantments={fortune:3}]",
            ):
                local_command(game,text)
            game.run("wait 20",timeout=30)
            command(game,"#dihrill diamond 1")
            game.wait_for("enchanted mining completes",lambda:
                          "Mining goal reached" in status(game),timeout=35)
            inv=game.run("inventory")
            slot=1 if preference=="silk" else 2
            resource="minecraft:diamond_ore" if preference=="silk" else "minecraft:diamond"
            assert inv["hotbar"][slot].get("damage")==1, inv["hotbar"][slot]
            assert inv["hotbar"][0].get("damage",0)==0, inv["hotbar"][0]
            assert inv["counts"].get(resource,0)>=1, inv["counts"]
    print("PASS: Silk Touch and Fortune preferences choose the appropriate enchanted hotbar tool",flush=True)


def local_wading(executable, run):
    # A contained spring feeds a one-block-high side channel. The shallow end
    # crosses the walking lane with a solid floor and dry headroom.
    for enabled in (False,True):
        home = run / ("wading-on" if enabled else "wading-off")
        seed_config(home, profile="cave", targets=["minecraft:diamond_ore"],
                    radius=12, explore=False, shallow_water=enabled)
        flags = CLIENT_FLAGS + ["--test-terrain","--world","Dihrill Wading QA","--seed","42"]
        with game_process(executable,home,flags) as game:
            ready(game)
            for text in (
                "fill -3 167 -4 15 175 11 minecraft:bedrock",
                "fill 0 170 0 10 171 0 minecraft:air",
                "fill 4 170 1 4 170 6 minecraft:air",
                "setblock 4 170 6 minecraft:water",
                "setblock 8 170 0 minecraft:diamond_ore",
                "tp @s 0.5 170 0.5 -90 0",
                "gamerule spawn_mobs false",
                "gamemode survival @s",
                "clear @s",
                "item replace entity @s hotbar.0 with minecraft:iron_pickaxe",
            ):
                local_command(game,text)
            game.wait_for("shallow flow reaches the route", lambda:
                          game.run("block 4 170 0").get("state") == "minecraft:water[level=6]",
                          timeout=20)
            command(game,"#dihrill forever")
            game.wait_for("wading run ends",lambda: "stopped" in status(game).lower(),timeout=45)
            assert has_block(game,[8,170,0],"air" if enabled else "diamond_ore")
            assert game.run("inventory")["counts"].get("minecraft:diamond",0) == int(enabled)
    print("PASS: shallow flow blocks the default route and can be crossed only with wading enabled",flush=True)


def local_branch(executable,run):
    home = run / "branch"
    seed_config(home, profile="branch", targets=["minecraft:diamond_ore"],
                radius=8, explore=True, explore_limit=28, branch_spacing=6, turn_speed=18)
    flags = CLIENT_FLAGS + ["--test-terrain", "--world", "Dihrill Branch QA", "--seed", "42"]
    with game_process(executable,home,flags) as game:
        ready(game)
        for text in (
            "fill -3 176 -4 18 185 11 minecraft:bedrock",
            "fill 0 180 0 16 181 0 minecraft:stone",
            "fill 6 180 1 6 181 8 minecraft:stone",
            "fill 12 180 -3 12 181 -1 minecraft:stone",
            "setblock 0 180 0 minecraft:air",
            "setblock 0 181 0 minecraft:air",
            "tp @s 0.5 180 0.5 -90 0",
            "gamerule spawn_mobs false",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:diamond_pickaxe[minecraft:enchantments={efficiency:4}]",
        ):
            local_command(game,text)
        game.run("wait 20",timeout=30)
        command(game,"#dihrill forever")
        game.wait_for("branch at the first interval",lambda:
                      has_block(game,[6,180,4],"air"),timeout=65)
        game.wait_for("returns to the main tunnel",lambda:
                      has_block(game,[10,180,0],"air"),timeout=65)
        command(game,"#dihrill stop")
        print("PASS: Branch Miner dug a lateral branch and returned to extend the main tunnel",flush=True)


def local_storage(executable, run):
    home = run / "storage"
    seed_config(home, explore=False, radius=16, storage=[1,140,2], avoid_caves=False,
                targets=["minecraft:diamond_ore"])
    flags = CLIENT_FLAGS + ["--test-terrain","--world","Dihrill Storage QA","--seed","42"]
    with game_process(executable,home,flags) as game:
        ready(game)
        for text in (
            "fill -3 137 -4 14 144 4 minecraft:bedrock",
            "fill 0 140 -2 11 141 3 minecraft:air",
            "setblock 1 140 2 minecraft:chest",
            "setblock 6 140 0 minecraft:diamond_ore",
            "tp @s 0.5 140 0.5 -90 0",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:iron_pickaxe",
            "item replace entity @s inventory.0 with minecraft:diamond 32",
        ):
            local_command(game,text)
        for slot in range(1,9):
            local_command(game,f"item replace entity @s hotbar.{slot} with minecraft:apple 1")
        for slot in range(1,27):
            local_command(game,f"item replace entity @s inventory.{slot} with minecraft:apple 1")
        game.run("wait 20",timeout=30)
        command(game,"#dihrill forever")
        game.wait_for("mines after using storage",lambda: has_block(game,[6,140,0],"air"),
                      timeout=50)
        check = local_command(game, "data get block 1 140 2 Items")
        assert "minecraft:diamond" in json.dumps(check), check
        inv = game.run("inventory")
        assert inv["counts"].get("minecraft:apple") == 34, inv["counts"]
        assert inv["counts"].get("minecraft:iron_pickaxe") == 1, inv["counts"]
        print("PASS: full inventory deposits selected resources in a configured chest and resumes mining",flush=True)


def local_item_and_area(executable,run):
    home = run / "area"
    seed_config(home, profile="area", explore=False, radius=12, avoid_caves=False,
                area=[[1,150,-1],[7,151,1]], targets=["minecraft:diamond_ore"])
    flags = CLIENT_FLAGS + ["--test-terrain","--world","Dihrill Area QA","--seed","42"]
    with game_process(executable,home,flags) as game:
        ready(game)
        for text in (
            "fill -3 147 -4 14 154 4 minecraft:bedrock",
            "fill 0 150 -2 11 151 3 minecraft:air",
            "setblock 3 150 0 minecraft:diamond_ore",
            "setblock 2 150 2 minecraft:diamond_ore",
            "setblock 8 150 0 minecraft:diamond_ore",
            "tp @s 0.5 150 0.5 -90 0",
            "gamemode survival @s",
            "clear @s",
            "item replace entity @s hotbar.0 with minecraft:iron_pickaxe",
        ):
            local_command(game,text)
        game.run("wait 20",timeout=30)
        command(game,"#dihrill forever")
        game.wait_for("Area Miner collects the inside block", lambda:
                      game.run("inventory")["counts"].get("minecraft:diamond",0) >= 1,
                      timeout=40)
        game.wait_for("Area Miner exhausts the configured area",lambda:
                      "stopped" in status(game).lower(), timeout=30)
        assert has_block(game,[2,150,2],"diamond_ore")
        assert has_block(game,[8,150,0],"diamond_ore")
        command(game,"#dihrill items diamond 1 diamond")
        game.wait_for("Quantity Miner collects one new item",lambda:
                      "stopped" in status(game).lower(),timeout=40)
        assert game.run("inventory")["counts"].get("minecraft:diamond") == 2
        assert sum(has_block(game,pos,"diamond_ore") for pos in ([2,150,2],[8,150,0])) == 1
        print("PASS: Area Miner respects its cuboid and an item goal counts new pickups",flush=True)


def remote_disconnect(executable,run):
    port = free_port()
    address = f"127.0.0.1:{port}"
    server_dir = run/"disconnect-server"
    server_dir.mkdir()
    (server_dir/"eula.txt").write_text("eula=true\n",encoding="ascii")
    (server_dir/"server.properties").write_text(
        "server-ip=127.0.0.1\nonline-mode=false\nspawn-protection=3\n"
        "white-list=false\nenforce-whitelist=false\nview-distance=4\nsimulation-distance=3\npause-when-empty-seconds=0\n",encoding="ascii")
    (server_dir/"ops.json").write_text(json.dumps([{
        "uuid": "1b604196-5e28-47ef-9f3a-a4bf4a1b6d65",
        "name": "FixtureOperator",
        "level": 4,
        "bypassesPlayerLimit": False,
    }]), encoding="utf-8")
    with game_process(INSTALLED/"horizon-client.exe",server_dir,
                      ["--test-terrain","--dedicated","--nogui","--port",str(port)],"console stop") as server:
        home = run/"disconnect-client"
        seed_config(home, explore=False, radius=16, avoid_caves=False)
        flags = CLIENT_FLAGS + ["--server",address,"--name","DihrillQA"]
        with game_process(executable,home,flags) as game:
            ready(game)
            for text in (
                "fill -3 127 -4 14 135 4 minecraft:bedrock",
                "fill 0 130 -2 11 131 3 minecraft:air",
                "setblock 2 130 0 minecraft:diamond_ore",
                "setblock 8 130 0 minecraft:diamond_ore",
                "setblock 10 130 0 minecraft:diamond_ore",
                "setworldspawn 0 130 0",
                "gamerule spawn_mobs false",
                "tp DihrillQA 0.5 130 0.5 -90 0",
                "item replace entity DihrillQA hotbar.0 with minecraft:iron_pickaxe",
            ):
                server.run("console " + text)
            game.run("wait 20",timeout=30)
            command(game,"#dihrill diamond")
            game.run("disconnect")
            game.run(f"connect {address} DihrillQA")
            ready(game)
            game.run("wait 40",timeout=30)
            assert has_block(game,[8,130,0],"diamond_ore")
            assert "No paused job" in command(game,"#dihrill resume")
            assert has_block(game,[8,130,0],"diamond_ore")
            print("PASS: disconnect cancels the running job across reconnect",flush=True)
            command(game,"#dihrill items diamond 1 diamond")
            game.wait_for("server-confirmed item goal",lambda:
                          "Mining goal reached" in status(game),timeout=55)
            assert has_block(game,[2,130,0],"diamond_ore")
            assert sum(has_block(game,[x,130,0],"air") for x in (8,10)) == 1
            inv=server.run("console data get entity DihrillQA Inventory")
            assert "minecraft:diamond" in json.dumps(inv), inv
            assert "1 target blocks mined" in status(game)
            print("PASS: spawn protection rejected the near ore and only the accepted break and pickup counted",flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out",type=Path,required=True)
    checks = {
        "vein": local_vein_and_commands,
        "tools": local_tools,
        "explore": local_exploration,
        "continuous": local_continuous_exploration,
        "timer": local_run_timer,
        "cave": local_cave,
        "water": local_wading,
        "branch": local_branch,
        "storage": local_storage,
        "area": local_item_and_area,
        "disconnect": remote_disconnect,
    }
    parser.add_argument("--case", choices=("all",*checks), default="all")
    options = parser.parse_args()
    options.out.mkdir(parents=True,exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="dihrill-extended-",dir=options.out.resolve()))
    print(directory,flush=True)
    for case in checks if options.case == "all" else (options.case,):
        checks[case](options.client.resolve(),directory)


if __name__ == "__main__":
    main()
