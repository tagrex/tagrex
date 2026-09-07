#!/usr/bin/env python3
# Pack PNG frames into a single .ico (#67).
#
#   pack-ico.py OUT.ico FRAME1.png FRAME2.png ...
#
# The frames are stored PNG-compressed, which every Windows since Vista reads at
# any size — so no BMP/DIB encoding is needed here, only the 6-byte ICONDIR
# header, one 16-byte directory entry per frame, and then the PNG blobs. Frames
# should be square; 256 px is written as the icon-directory's "0" sentinel.
import struct
import sys


def main(out_path, png_paths):
    frames = []
    for path in png_paths:
        with open(path, "rb") as handle:
            data = handle.read()
        # PNG IHDR width/height sit at bytes 16..24: 8-byte signature, then the
        # IHDR chunk's 4-byte length and 4-byte "IHDR" type, then w/h as u32 BE.
        width, height = struct.unpack(">II", data[16:24])
        frames.append((width, height, data))

    header = struct.pack("<HHH", 0, 1, len(frames))  # reserved, type=1 icon, count
    directory = b""
    offset = 6 + 16 * len(frames)
    for width, height, data in frames:
        directory += struct.pack(
            "<BBBBHHII",
            width if width < 256 else 0,  # 0 means 256
            height if height < 256 else 0,
            0,  # palette count (0 for a true-color image)
            0,  # reserved
            1,  # color planes
            32,  # bits per pixel
            len(data),
            offset,
        )
        offset += len(data)

    with open(out_path, "wb") as handle:
        handle.write(header)
        handle.write(directory)
        for _, _, data in frames:
            handle.write(data)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
