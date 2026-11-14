"""Exercise the StrataDrill shop UI and sale loop against a localhost shop fixture."""

import argparse
from contextlib import contextmanager
import json
from pathlib import Path
import socket
import struct
import tempfile
import threading
import time
import zlib

from extended_smoke import config_file, has_block, seed_config
from smoke import CLIENT_FLAGS, INSTALLED, RESOURCES, free_port, game_process, ready


TITLE = "QA Diamond Shop"
WINDOW = 41
CONTAINER_ROWS = 27
REGISTRIES = json.loads((RESOURCES / "registries.json").read_text(encoding="utf8"))
DIAMOND = REGISTRIES["minecraft:item"]["entries"]["minecraft:diamond"]["protocol_id"]
PICKAXE = REGISTRIES["minecraft:item"]["entries"]["minecraft:diamond_pickaxe"]["protocol_id"]
MENU = REGISTRIES["minecraft:menu"]["entries"]["minecraft:generic_9x3"]["protocol_id"]


def vi(value):
    result = bytearray()
    while value > 127:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)


def read_vi(stream):
    value = 0
    for offset in range(0, 35, 7):
        data = stream.read(1)
        if not data:
            raise EOFError
        byte = data[0]
        value |= (byte & 127) << offset
        if byte < 128:
            return value
    raise ValueError("Oversized VarInt")


def stack(item, count):
    return vi(count) + (vi(item) + b"\0\0" if count else b"")


class Shop:
    def __init__(self, listener, server_port, server):
        self.listener = listener
        self.server_port = server_port
        self.server = server
        self.compression = None
        self.phase = "login"
        self.lock = threading.Lock()
        self.closing = threading.Event()
        self.log = []
        self.errors = []
        self.stacks = [12, 5, 0]
        self.clicks = []
        self.visit_times = []
        self.window_open = False

    def receive(self, stream):
        length = read_vi(stream)
        body = stream.read(length)
        if len(body) != length:
            raise EOFError
        if self.compression is not None:
            from io import BytesIO
            packed = BytesIO(body)
            raw_length = read_vi(packed)
            body = packed.read()
            if raw_length:
                body = zlib.decompress(body)
                assert len(body) == raw_length
        return body

    def send(self, endpoint, body, framing=True):
        if framing and self.compression is not None:
            body = (
                vi(len(body)) + zlib.compress(body)
                if len(body) >= self.compression else b"\0" + body
            )
        with self.lock:
            endpoint.sendall(vi(len(body)) + body)

    def open_shop(self, client):
        self.log.append("open")
        self.visit_times.append(time.monotonic())
        self.clicks.clear()
        self.window_open = True
        title = TITLE.encode()
        self.send(client, vi(60) + vi(WINDOW) + vi(MENU)
                  + b"\x08" + struct.pack(">H", len(title)) + title)
        contents = (
            b"\0" * CONTAINER_ROWS
            + b"".join(stack(DIAMOND, count) for count in self.stacks)
            + b"\0" * 24 + stack(PICKAXE, 1) + b"\0" * 8
        )
        self.send(client, vi(18) + vi(WINDOW) + vi(1) + vi(63) + contents + b"\0")

    def click(self, data):
        window = read_vi(data)
        if window != WINDOW:
            return False
        read_vi(data)
        slot, button = struct.unpack(">hB", data.read(3))
        operation = read_vi(data)
        self.log.append((slot, button, operation))
        assert (button, operation) == (0, 0), self.log
        index = slot - CONTAINER_ROWS
        assert 0 <= index < len(self.stacks) and index not in self.clicks, self.log
        assert self.stacks[index] > 0, self.log
        self.clicks.append(index)
        # First sale keeps eight diamonds. The other clicks empty their slots.
        self.stacks[index] = 8 if len(self.visit_times) == 2 and index == 0 else 0
        # Hold back all sale updates until close. This verifies the client does
        # not need confirmation to send the second click or leave the shop.
        return True

    def apply_sales(self):
        for index in self.clicks:
            remaining = self.stacks[index]
            item = f"minecraft:diamond {remaining}" if remaining else "minecraft:air"
            self.server.run(f"console item replace entity ShopQA inventory.{index} with {item}")

    def relay(self, incoming, outgoing, client_side):
        from io import BytesIO
        try:
            with incoming.makefile("rb") as stream:
                while not self.closing.is_set():
                    body = self.receive(stream)
                    data = BytesIO(body)
                    kind = read_vi(data)
                    if self.phase == "login" and not client_side and kind == 3:
                        threshold = read_vi(data)
                        self.send(outgoing, body, framing=False)
                        self.compression = threshold
                        continue
                    if client_side and self.phase == "login" and kind == 3:
                        self.phase = "config"
                    elif client_side and self.phase == "config" and kind == 3:
                        self.phase = "play"
                    elif client_side and self.phase == "play":
                        if kind == 7 and data.read(read_vi(data)) == b"shop":
                            self.open_shop(incoming)
                            continue
                        if kind == 18 and self.window_open and self.click(data):
                            continue
                        if kind == 19 and self.window_open and read_vi(data) == WINDOW:
                            self.log.append("close")
                            self.window_open = False
                            if self.clicks:
                                self.apply_sales()
                                self.log.append("applied")
                            continue
                    self.send(outgoing, body)
        except (EOFError, OSError) as error:
            if not self.closing.is_set() and not isinstance(error, EOFError):
                self.errors.append(repr(error))
        except Exception as error:
            self.errors.append(repr(error))
        finally:
            for connection in (incoming, outgoing):
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def run(self):
        try:
            client, _ = self.listener.accept()
        except OSError:
            return
        with client, socket.create_connection(("127.0.0.1", self.server_port)) as server:
            reader = threading.Thread(target=self.relay, args=(server, client, False), daemon=True)
            reader.start()
            self.relay(client, server, True)
            reader.join(3)


@contextmanager
def loopback_shop(port, server):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        shop = Shop(listener, port, server)
        thread = threading.Thread(target=shop.run, daemon=True)
        thread.start()
        try:
            yield shop, listener.getsockname()[1]
        finally:
            shop.closing.set()
            listener.close()
            thread.join(4)
            assert not shop.errors, shop.errors


def check(client, root):
    host = root / "server"
    host.mkdir()
    (host / "eula.txt").write_text("eula=true\n", encoding="ascii")
    (host / "server.properties").write_text(
        "server-ip=127.0.0.1\nonline-mode=false\nspawn-protection=0\n"
        "white-list=false\nenforce-whitelist=false\n"
        "view-distance=4\nsimulation-distance=3\npause-when-empty-seconds=0\n",
        encoding="ascii",
    )
    port = free_port()
    installed = INSTALLED / "horizon-client.exe"
    with game_process(installed, host, ["--test-terrain", "--dedicated", "--nogui",
                                       "--port", str(port)], "console stop") as server:
        with loopback_shop(port, server) as (shop, proxy_port):
            address = f"127.0.0.1:{proxy_port}"
            home = root / "client"
            seed_config(home, targets=["minecraft:iron_ore"], explore=False, radius=32)
            with game_process(client, home, CLIENT_FLAGS + ["--server", address, "--name", "ShopQA"]) as game:
                ready(game)
                for text in (
                    "fill -3 126 -5 29 134 5 minecraft:bedrock",
                    "fill 0 130 0 26 131 0 minecraft:air",
                    "fill 4 130 0 19 131 0 minecraft:deepslate",
                    "setblock 23 130 0 minecraft:iron_ore",
                    "setblock 24 130 0 minecraft:iron_ore",
                    "setblock 24 131 0 minecraft:iron_ore",
                    "tp ShopQA 0.5 130 0.5 -90 0",
                    "gamemode survival ShopQA",
                    "clear ShopQA",
                    "item replace entity ShopQA hotbar.0 with minecraft:diamond_pickaxe",
                    "item replace entity ShopQA inventory.0 with minecraft:diamond 12",
                    "item replace entity ShopQA inventory.1 with minecraft:diamond 5",
                ):
                    server.run("console " + text)
                game.run("wait 20")
                game.run("options-set guiScale 1")
                game.run("key key.keyboard.f8 tap")
                game.wait_for("settings", lambda: game.run("screen")["screen"] == "mod")
                def click(x, y):
                    game.run(f"click-at {x} {y}")
                    time.sleep(0.25)

                click(555, 192)
                click(395, 352)
                for x, value in ((395, 20), (570, 4)):
                    click(x, 266)
                    game.run(f"type {value}")
                    game.run("key enter tap")
                    time.sleep(0.25)
                settings = json.loads(config_file(home).read_text(encoding="utf8"))
                assert settings["shop_min_diamonds"] == 20, settings
                assert settings["shop_interval_seconds"] == 4, settings
                click(395, 291)
                def configured():
                    config = json.loads(config_file(home).read_text(encoding="utf8"))
                    return config.get("diamond_shop", {}).get("title") == TITLE
                game.wait_for("learned shop", configured, timeout=12)
                assert shop.log == ["open"], shop.log
                assert game.run("inventory")["counts"].get("minecraft:diamond") == 17
                game.run("key key.keyboard.escape tap")
                game.wait_for("setup closes", lambda: game.run("screen")["screen"] == "playing")
                game.run("chat #mine forever")
                game.wait_for("walking below the diamond threshold",
                              lambda: game.run("state")["player"]["position"][0] >= 1.5, timeout=15)
                assert shop.log == ["open", "close"], shop.log
                shop.stacks[:] = [30, 21, 19]
                for i, count in enumerate(shop.stacks):
                    server.run(f"console item replace entity ShopQA inventory.{i} with minecraft:diamond {count}")
                game.wait_for("two separate sale visits", lambda: shop.log.count("applied") == 2, timeout=25)
                assert shop.log == [
                    "open", "close", "open", (27, 0, 0), (28, 0, 0), "close", "applied",
                    "open", (29, 0, 0), (27, 0, 0), "close", "applied",
                ], shop.log
                interval = shop.visit_times[2] - shop.visit_times[1]
                assert interval >= 3.85, interval
                game.wait_for("all diamond sales applied",
                              lambda: not game.run("inventory")["counts"].get("minecraft:diamond"), timeout=10)
                game.wait_for("mining resumes after sale", lambda:
                              all(has_block(game, p, "air") for p in
                                  ((23,130,0), (24,130,0), (24,131,0))), timeout=100)
                print(f"PASS: 17 diamonds skipped at the 20-diamond threshold; two partial stacks "
                      f"sold per visit; another visit after {interval:.2f}s sold the remainder; mining resumed.",
                      flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    options = parser.parse_args()
    options.out.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix="stratadrill-shop-", dir=options.out.resolve()))
    print(folder, flush=True)
    check(options.client.resolve(), folder)
