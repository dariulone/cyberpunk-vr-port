"""Read-only Quest Pro / Steam Link facial-weight diagnostic (Python stdlib).

Does not change SteamVR settings, write recordings, or animate V. Run with the
headset connected; Steam Link OSC Custom/ALT output defaults to localhost:9015.
Close other consumers of that port first. Protocol reference:
https://github.com/danwillm/VRCFT-SteamLink#about-osc-data-from-steamlink
"""
import argparse
import math
import socket
import struct
import time


def osc_string(data, offset):
    end = data.find(b"\0", offset)
    if end < 0:
        raise ValueError("Unterminated OSC string")
    following = (end + 4) & ~3
    if following > len(data) or any(data[end:following]):
        raise ValueError("Invalid OSC string padding")
    return data[offset:end].decode("utf-8"), following


def osc_messages(data, depth=0):
    """Decode complete messages/bundles with bounded nesting and checked sizes."""
    if depth > 8 or not data or len(data) % 4:
        raise ValueError("Invalid OSC packet size/depth")
    if data.startswith(b"#bundle\0"):
        if len(data) < 16:
            raise ValueError("Truncated OSC bundle")
        result, offset = [], 16  # timetag is not a reliable local freshness clock
        while offset < len(data):
            if offset + 4 > len(data):
                raise ValueError("Truncated bundle length")
            size = struct.unpack_from(">I", data, offset)[0]
            offset += 4
            if not size or size % 4 or size > len(data) - offset:
                raise ValueError("Invalid bundle element size")
            result.extend(osc_messages(data[offset:offset + size], depth + 1))
            offset += size
        return result
    address, offset = osc_string(data, 0)
    tags, offset = osc_string(data, offset)
    if not address.startswith("/") or not tags.startswith(","):
        raise ValueError("Invalid OSC address/type tags")
    values = []
    for tag in tags[1:]:
        if tag in "fid":
            size = 8 if tag == "d" else 4
            if offset + size > len(data):
                raise ValueError("Truncated OSC value")
            values.append(struct.unpack_from(">" + tag, data, offset)[0])
            offset += size
        elif tag == "s":
            value, offset = osc_string(data, offset)
            values.append(value)
        elif tag in "TF":
            values.append(tag == "T")
        else:
            # Steam Link may add unrelated payload types. Ignore that message;
            # a bundle's other independent messages remain useful.
            return []
    if offset != len(data):
        raise ValueError("Trailing OSC message data")
    return [(address, values)]


class FaceSamples:
    # Independent packets can carry different portions of the face. Track each
    # value's receive time so a missing region never looks current indefinitely.
    def __init__(self):
        self.values = {}
        self.accepted = 0

    def accept(self, messages, now):
        for address, values in messages:
            if not address.startswith(("/sl/xrfb/facew/", "/sl/xrfb/facec/")):
                continue
            if len(values) != 1 or type(values[0]) is not float:
                continue
            value = values[0]
            if not math.isfinite(value) or not 0 <= value <= 1:
                continue
            self.values[address] = (value, now)
            self.accepted += 1

    def fresh(self, now):
        return {key: value for key, (value, when) in self.values.items()
                if 0 <= now - when <= 0.5}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=30)
    parser.add_argument("--port", type=int, default=9015)
    args = parser.parse_args()
    if not 1 <= args.seconds <= 300 or not 1024 <= args.port <= 65535:
        parser.error("Use 1..300 seconds and a port in 1024..65535")
    samples, packets, malformed = FaceSamples(), 0, 0
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as receiver:
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            receiver.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        try:
            receiver.bind(("127.0.0.1", args.port))
        except OSError as error:
            parser.exit(2, f"Cannot listen on localhost:{args.port}: {error}\n"
                        "Close VRCFaceTracking/another listener on that port and retry.\n")
        receiver.settimeout(0.2)
        started = next_report = time.monotonic()
        print("Listening locally; no files recorded. Smile, blink, then open your jaw.")
        try:
            while time.monotonic() - started < args.seconds:
                try:
                    data, _ = receiver.recvfrom(65535)
                    packets += 1
                    samples.accept(osc_messages(data), time.monotonic())
                except socket.timeout:
                    pass
                except (ValueError, UnicodeError, struct.error):
                    malformed += 1
                now = time.monotonic()
                if now >= next_report:
                    fresh = samples.fresh(now)
                    labels = ("LipCornerPullerL", "LipCornerPullerR", "EyesClosedL",
                              "EyesClosedR", "JawDrop")
                    status = [f"{name}={fresh['/sl/xrfb/facew/' + name]:.2f}"
                              if '/sl/xrfb/facew/' + name in fresh else f"{name}=waiting"
                              for name in labels]
                    print(f"Fresh face channels: {len(fresh)}; " + " ".join(status))
                    next_report = now + 1
        except KeyboardInterrupt:
            pass
    print(f"Packets: {packets}; accepted facial values: {samples.accepted}; malformed: {malformed}")
    if not samples.accepted:
        print("No validated face weights received. Check headset permissions, Steam Link sharing and port.")


if __name__ == "__main__":
    main()
