#!/usr/bin/env python3
"""Direct, isolated libass 0.17.5 oracle. Never invoked by cargo test.
Generate only new compatibility samples with --generate; do not touch historic
FFmpeg frames or self goldens. No fontconfig/system font discovery or video
color conversion. Call render() for ad-hoc source probes.
"""
import argparse
import ctypes as C
import ctypes.util
import hashlib
import json
import pathlib
import subprocess
import struct

ROOT = pathlib.Path(__file__).resolve().parents[2]
LIB = C.CDLL(ctypes.util.find_library('ass'))
P = C.c_void_p

def function(name, result, *args):
    f = getattr(LIB, name)
    f.restype, f.argtypes = result, list(args)
    return f

version = function('ass_library_version', C.c_int)()
if version != 0x01705000:
    raise RuntimeError(f'Requires libass 0.17.5 (0x01705000); got {version:#x}')
init = function('ass_library_init', P)
done = function('ass_library_done', None, P)
rinit = function('ass_renderer_init', P, P)
rdone = function('ass_renderer_done', None, P)
size = function('ass_set_frame_size', None, P, C.c_int, C.c_int)
storage = function('ass_set_storage_size', None, P, C.c_int, C.c_int)
fonts = function('ass_set_fonts', None, P, C.c_char_p, C.c_char_p, C.c_int, C.c_char_p, C.c_int)
addfont = function('ass_add_font', None, P, C.c_char_p, C.c_char_p, C.c_int)
read = function('ass_read_memory', P, P, C.c_char_p, C.c_size_t, C.c_char_p)
free = function('ass_free_track', None, P)
hinting = function('ass_set_hinting', None, P, C.c_int)
shaping = function('ass_set_shaper', None, P, C.c_int)
pixel_aspect = function('ass_set_pixel_aspect', None, P, C.c_double)
feature = function('ass_track_set_feature', C.c_int, P, C.c_int, C.c_int)

class Image(C.Structure):
    pass
Image._fields_ = [('w', C.c_int), ('h', C.c_int), ('stride', C.c_int),
                  ('bitmap', C.POINTER(C.c_ubyte)), ('color', C.c_uint32),
                  ('x', C.c_int), ('y', C.c_int), ('next', C.POINTER(Image)), ('type', C.c_int)]
frame = function('ass_render_frame', C.POINTER(Image), P, P, C.c_longlong, C.POINTER(C.c_int))

def render(ass, width, height, times):
    """Each invocation is a fresh renderer; times can probe playback retention."""
    lib = init()
    data = (ROOT / 'fonts/DejaVuSans.ttf').read_bytes()
    addfont(lib, b'DejaVuSans.ttf', data, len(data))
    renderer = rinit(lib)
    size(renderer, width, height)
    storage(renderer, width, height)
    hinting(renderer, 0)
    shaping(renderer, 1)
    pixel_aspect(renderer, 1.0)
    fonts(renderer, str(ROOT / 'fonts/DejaVuSans.ttf').encode(), b'DejaVu Sans', 0, None, 1)
    source = ass.encode('utf8')
    track = read(lib, source, len(source), b'UTF-8')
    if not track:
        raise RuntimeError('libass could not parse fixture')
    # ASS_FEATURE_WRAP_UNICODE is explicitly disabled, regardless of build.
    if feature(track, 3, 0) != 0:
        raise RuntimeError('cannot configure Unicode wrapping')
    outputs = []
    try:
        for time in times:
            out = bytearray(bytes([0, 0, 0, 255]) * (width * height))
            cur = frame(renderer, track, time, C.byref(C.c_int()))
            while cur:
                im = cur.contents
                r, g, b, opacity = im.color >> 24, (im.color >> 16) & 255, (im.color >> 8) & 255, 255 - (im.color & 255)
                for y in range(im.h):
                    py = im.y + y
                    if not 0 <= py < height:
                        continue
                    for x in range(im.w):
                        px = im.x + x
                        if not 0 <= px < width:
                            continue
                        a = im.bitmap[y * im.stride + x] * opacity / 255
                        i = (py * width + px) * 4
                        for c, value in enumerate((r, g, b)):
                            out[i + c] = int((value * a + out[i + c] * (255 - a)) / 255 + 0.5)
                cur = im.next
            outputs.append(bytes(out))
    finally:
        free(track)
        rdone(renderer)
        done(lib)
    return outputs

def runtime_dependencies():
    ft = C.CDLL(ctypes.util.find_library('freetype'))
    ft.FT_Init_FreeType.argtypes = [C.POINTER(P)]
    ft.FT_Library_Version.argtypes = [P, C.POINTER(C.c_int), C.POINTER(C.c_int), C.POINTER(C.c_int)]
    ft.FT_Done_FreeType.argtypes = [P]
    library = P()
    if ft.FT_Init_FreeType(C.byref(library)) != 0:
        raise RuntimeError('FreeType init failed')
    major, minor, patch = C.c_int(), C.c_int(), C.c_int()
    ft.FT_Library_Version(library, C.byref(major), C.byref(minor), C.byref(patch))
    ft.FT_Done_FreeType(library)
    hb = C.CDLL(ctypes.util.find_library('harfbuzz'))
    hb.hb_version_string.restype = C.c_char_p
    fb = C.CDLL(ctypes.util.find_library('fribidi'))
    return {'freetype': f'{major.value}.{minor.value}.{patch.value}',
            'harfbuzz': hb.hb_version_string().decode(),
            'fribidi': C.c_char_p.in_dll(fb, 'fribidi_version_info').value.decode().splitlines()[0]}

def generate():
    directory = pathlib.Path(__file__).resolve().parent
    samples = []
    for row in (directory / 'cases.tsv').read_text().splitlines():
        if not row or row.startswith('#'):
            continue
        name, script, w, h, ms = row.split('\t')
        image = render((directory / f'{script}.ass').read_text(), int(w), int(h), [int(ms)])[0]
        # RGBA run-length encoding: little-endian u32 pixel count + one
        # RGBA pixel. Keep large mostly-black frames small in source control.
        encoded = bytearray()
        pixel, count = image[:4], 0
        for offset in range(0, len(image), 4):
            current = image[offset:offset + 4]
            if current != pixel:
                encoded.extend(struct.pack('<I', count) + pixel)
                pixel, count = current, 0
            count += 1
        encoded.extend(struct.pack('<I', count) + pixel)
        (directory / f'{name}.rle').write_bytes(encoded)
        samples.append({'name': name, 'sha256': hashlib.sha256(image).hexdigest()})
    def pkg(name):
        return subprocess.check_output(['pkg-config', '--modversion', name], text=True).strip()
    provenance = {
        'generator': 'tests/compatibility/libass.py --generate', 'libass': '0.17.5',
        'api_version': hex(version), 'runtime_dependencies': runtime_dependencies(), 'freetype_pkg_config': pkg('freetype2'), 'harfbuzz': pkg('harfbuzz'),
        'fribidi_pkg_config': pkg('fribidi'), 'fribidi_build_header': '1.0.16 (libass Shaper log)',
        'libass_source': '4a05d8127f525943ebf45fdc6497c9e665947f0d', 'libunibreak_pkg_config': pkg('libunibreak'), 'font_provider': 'ASS_FONTPROVIDER_NONE',
        'fonts': {'fonts/DejaVuSans.ttf': hashlib.sha256((ROOT / 'fonts/DejaVuSans.ttf').read_bytes()).hexdigest()},
        'configuration': {'hinting': 'NONE', 'shaping': 'COMPLEX', 'storage_size': 'frame dimensions',
                          'wrap_unicode': False, 'pixel_aspect': 1, 'margins': [0, 0, 0, 0],
                          'samples': 'fresh renderer at each timestamp', 'composition': 'raw ASS_Image over black; no FFmpeg'},
        'scripts': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(directory.glob('*.ass'))},
        'samples': samples,
    }
    (directory / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')

def probe_retention():
    path = pathlib.Path(__file__).resolve().parent / 'collision-staggered.ass'
    source = path.read_text()
    header, events = source.split('[Events]')
    lines = events.splitlines()
    dialogues = [line for line in lines if line.startswith('Dialogue:')]
    # Earlier event first, then later event. End the earlier one mid-overlap.
    earlier = dialogues[1].replace('0:00:05.00', '0:00:01.50')
    later = dialogues[0]
    script = header + '[Events]\n' + lines[1] + '\n' + earlier + '\n' + later + '\n'
    playback = render(script, 384, 216, [1000, 2000])[-1]
    seek = render(script, 384, 216, [2000])[0]
    differing = sum(playback[i:i+4] != seek[i:i+4] for i in range(0, len(seek), 4))
    print(f'libass playback vs fresh seek: {differing} differing pixels (retained collision slot)')
    if not differing:
        raise RuntimeError('expected libass retention difference was not reproduced')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--generate', action='store_true')
    parser.add_argument('--probe-retention', action='store_true')
    args = parser.parse_args()
    if args.generate:
        generate()
    if args.probe_retention:
        probe_retention()
