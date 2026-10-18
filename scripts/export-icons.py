"""Package existing PNG exports; no image editing. Requires Pillow for inspection."""
import argparse
import io
import struct
from pathlib import Path
from PIL import Image

BASE = Path(__file__).resolve().parents[1] / 'assets' / 'brand' / 'platforms'
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
MAC = {'icp4': '16x16', 'icp5': '32x32', 'icp6': '32x32@2x',
       'ic07': '128x128', 'ic08': '256x256', 'ic09': '512x512',
       'ic10': '512x512@2x', 'ic11': '16x16@2x', 'ic12': '32x32@2x',
       'ic13': '128x128@2x', 'ic14': '256x256@2x'}

def package():
    payloads = [(BASE / 'windows' / f'planora-{n}.png').read_bytes() for n in SIZES]
    offset = 6 + 16 * len(SIZES)
    entries = []
    for size, blob in zip(SIZES, payloads):
        entries.append(struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(blob), offset))
        offset += len(blob)
    (BASE / 'windows' / 'planora.ico').write_bytes(struct.pack('<HHH', 0, 1, len(SIZES)) + b''.join(entries + payloads))
    chunks = []
    for kind, name in MAC.items():
        blob = (BASE / 'macos' / 'Planora.iconset' / f'icon_{name}.png').read_bytes()
        chunks.append(kind.encode('ascii') + struct.pack('>I', 8 + len(blob)) + blob)
    data = b''.join(chunks)
    (BASE / 'macos' / 'Planora.icns').write_bytes(b'icns' + struct.pack('>I', 8 + len(data)) + data)

def check():
    ico = Image.open(BASE / 'windows' / 'planora.ico')
    assert ico.ico.sizes() == {(n,n) for n in SIZES}
    for n in SIZES:
        frame = ico.ico.getimage((n,n))
        assert frame.size == (n,n) and frame.mode == 'RGBA'
    data = (BASE / 'macos' / 'Planora.icns').read_bytes()
    assert data[:4] == b'icns' and struct.unpack('>I', data[4:8])[0] == len(data)
    position, found = 8, {}
    while position < len(data):
        kind = data[position:position+4].decode('ascii')
        length = struct.unpack('>I', data[position+4:position+8])[0]
        image = Image.open(io.BytesIO(data[position+8:position+length])); image.load()
        source = Image.open(BASE / 'macos' / 'Planora.iconset' / f'icon_{MAC[kind]}.png')
        assert image.size == source.size
        found[kind] = image.size
        position += length
    assert position == len(data) and set(found) == set(MAC)
    mac = Image.open(BASE / 'macos' / 'Planora.icns'); mac.load()
    assert mac.size == (1024,1024)
    for file in BASE.rglob('*.png'):
        image = Image.open(file); image.load()
        assert image.mode == 'RGBA', file
        assert image.getextrema()[3][0] == 0 and image.getextrema()[3][1] == 255, file
    print(f'Validated ICO: {len(SIZES)} frames; ICNS: {len(found)} chunks; platform PNG alpha channels.')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--check', action='store_true')
    if not parser.parse_args().check:
        package()
    check()
