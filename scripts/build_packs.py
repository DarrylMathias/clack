#!/usr/bin/env python3
"""
Builds all clack starter packs and catalog:
1. IBM Model M (1985) - Bucklespring (GPL-2.0)
2. Alps SKCM Blue - kbsim (MIT)
3. Electric Typewriter - keyboardsounds (GPL-3.0)
4. Cherry MX Blue (PBT) - Mechvibes (MIT)
5. Cherry MX Blue (ABS) - Mechvibes (MIT)
6. Cherry MX Brown (PBT) - Mechvibes (MIT)
7. Cherry MX Brown (ABS) - Mechvibes (MIT)
8. Cherry MX Black (ABS) - Mechvibes (MIT)
9. Kailh Box White - Mechvibes community / wayclick (CC-BY)
10. NovelKeys Cream (Full Travel) - Mechvibes (MIT)
11. Zeal Tealios V2 - keyboardsounds (GPL-3.0)
12. Cherry MX Red (PBT) - Mechvibes (MIT)
13. Everglide Oreo - Mechvibes (MIT)

Converts upstream files into clack native packs (manifest.toml + audio + LICENSE),
archives each pack into .tar.gz, and generates dist/index.json.
"""

import hashlib
import json
import os
import shutil
import tarfile

DIST_DIR = os.path.abspath("dist/packs")
INDEX_PATH = os.path.abspath("dist/index.json")
SOURCES_DIR = os.environ.get("CLACK_PACK_SOURCES", "/tmp/pack_sources")

KEYMAP = {
    "1": 1,    # Esc
    "2": 2,    # 1
    "3": 3,    # 2
    "4": 4,    # 3
    "5": 5,    # 4
    "6": 6,    # 5
    "7": 7,    # 6
    "8": 8,    # 7
    "9": 9,    # 8
    "10": 10,  # 9
    "11": 11,  # 0
    "12": 12,  # -
    "13": 13,  # =
    "14": 14,  # Backspace
    "15": 15,  # Tab
    "16": 16,  # Q
    "17": 17,  # W
    "18": 18,  # E
    "19": 19,  # R
    "20": 20,  # T
    "21": 21,  # Y
    "22": 22,  # U
    "23": 23,  # I
    "24": 24,  # O
    "25": 25,  # P
    "26": 26,  # [
    "27": 27,  # ]
    "28": 28,  # Enter
    "29": 29,  # Left Ctrl
    "30": 30,  # A
    "31": 31,  # S
    "32": 32,  # D
    "33": 33,  # F
    "34": 34,  # G
    "35": 35,  # H
    "36": 36,  # J
    "37": 37,  # K
    "38": 38,  # L
    "39": 39,  # ;
    "40": 40,  # '
    "41": 41,  # `
    "42": 42,  # Left Shift
    "43": 43,  # \
    "44": 44,  # Z
    "45": 45,  # X
    "46": 46,  # C
    "47": 47,  # V
    "48": 48,  # B
    "49": 49,  # N
    "50": 50,  # M
    "51": 51,  # ,
    "52": 52,  # .
    "53": 53,  # /
    "54": 54,  # Right Shift
    "56": 56,  # Left Alt
    "57": 57,  # Space
    "58": 58,  # Caps Lock
    "97": 97,  # Right Ctrl
    "100": 100,# Right Alt
    "125": 125,# Left Super
    "126": 126,# Right Super
}

def create_archive_and_entry(pack_dir, pack_info):
    pack_id = pack_info["id"]
    tar_filename = f"{pack_id}.tar.gz"
    tar_path = os.path.join(DIST_DIR, tar_filename)
    
    print(f"    Archiving {tar_filename}...")
    with tarfile.open(tar_path, "w:gz") as tar:
        for item in sorted(os.listdir(pack_dir)):
            tar.add(os.path.join(pack_dir, item), arcname=item)
            
    size = os.path.getsize(tar_path)
    with open(tar_path, "rb") as f:
        sha256 = hashlib.sha256(f.read()).hexdigest()
        
    print(f"    ✓ {tar_filename}: {size} bytes, sha256={sha256[:12]}...")
    
    return {
        "id": pack_id,
        "name": pack_info["name"],
        "family": pack_info["family"],
        "license": pack_info["license"],
        "starter": pack_info.get("starter", False),
        "size": size,
        "sha256": sha256,
        "url": f"https://github.com/DarrylMathias/clack/releases/latest/download/{tar_filename}",
        "blurb": pack_info["blurb"]
    }

def build_mechvibes_sprite(upstream_folder, pack_info, temp_root):
    pack_id = pack_info["id"]
    pack_dir = os.path.join(temp_root, pack_id)
    os.makedirs(pack_dir, exist_ok=True)
    
    src_folder = os.path.join(SOURCES_DIR, "mechvibes", "src", "audio", upstream_folder)
    with open(os.path.join(src_folder, "config.json"), "r") as f:
        config_data = json.load(f)
        
    sound_file = config_data.get("sound", "sound.ogg")
    shutil.copy(os.path.join(src_folder, sound_file), os.path.join(pack_dir, sound_file))
    
    defines = config_data.get("defines", {})
    lines = [
        "format = 1",
        f'id = "{pack_info["id"]}"',
        f'name = "{pack_info["name"]}"',
        f'family = "{pack_info["family"]}"',
        f'license = "{pack_info["license"]}"',
        f'source = "https://github.com/hainguyents13/mechvibes"',
        f'credits = "Mechvibes contributors"',
        f'sprite = "{sound_file}"',
        "",
        "[press]",
    ]
    
    default_slice = defines.get("57") or next(iter(defines.values())) if defines else [0, 200]
    lines.append(f"default = [{default_slice[0]}, {default_slice[1]}]")
    
    for mv_code, slice_info in defines.items():
        if isinstance(slice_info, list) and len(slice_info) == 2:
            ev_code = KEYMAP.get(str(mv_code))
            if ev_code is None:
                try:
                    ev_code = int(mv_code)
                except ValueError:
                    continue
            lines.append(f'"{ev_code}" = [{slice_info[0]}, {slice_info[1]}]')
            
    with open(os.path.join(pack_dir, "manifest.toml"), "w") as f:
        f.write("\n".join(lines) + "\n")
        
    with open(os.path.join(pack_dir, "LICENSE"), "w") as f:
        f.write("MIT License\n\nCopyright (c) 2020 hainguyen\nhttps://github.com/hainguyents13/mechvibes\n")
        
    return create_archive_and_entry(pack_dir, pack_info)

def build_ibm_model_m(pack_info, temp_root):
    pack_id = pack_info["id"]
    pack_dir = os.path.join(temp_root, pack_id)
    sounds_dir = os.path.join(pack_dir, "sounds")
    os.makedirs(sounds_dir, exist_ok=True)
    
    src_wav_dir = os.path.join(SOURCES_DIR, "bucklespring", "wav")
    press_keys = {}
    release_keys = {}
    alpha_press = []
    alpha_release = []
    
    for f in sorted(os.listdir(src_wav_dir)):
        if not f.endswith(".wav"):
            continue
        stem = f[:-4]
        if '-' not in stem:
            continue
        hex_code, state = stem.split('-', 1)
        try:
            evdev_code = int(hex_code, 16)
        except ValueError:
            continue
            
        shutil.copy(os.path.join(src_wav_dir, f), os.path.join(sounds_dir, f))
        rel_path = f"sounds/{f}"
        
        if state == "0":
            press_keys[str(evdev_code)] = rel_path
            if 0x10 <= evdev_code <= 0x32 and len(alpha_press) < 10:
                alpha_press.append(rel_path)
        elif state == "1":
            release_keys[str(evdev_code)] = rel_path
            if 0x10 <= evdev_code <= 0x32 and len(alpha_release) < 10:
                alpha_release.append(rel_path)
                
    manifest_lines = [
        "format = 1",
        f'id = "{pack_info["id"]}"',
        f'name = "{pack_info["name"]}"',
        f'family = "{pack_info["family"]}"',
        f'license = "{pack_info["license"]}"',
        'source = "https://github.com/zevv/bucklespring"',
        'credits = "Ico Doornekamp (zevv) / Bucklespring"',
        "",
        "[press]",
        f"default = {json.dumps(alpha_press)}",
    ]
    for k, v in sorted(press_keys.items(), key=lambda x: int(x[0])):
        manifest_lines.append(f'"{k}" = "{v}"')
        
    manifest_lines.extend([
        "",
        "[release]",
        f"default = {json.dumps(alpha_release)}",
    ])
    for k, v in sorted(release_keys.items(), key=lambda x: int(x[0])):
        manifest_lines.append(f'"{k}" = "{v}"')
        
    with open(os.path.join(pack_dir, "manifest.toml"), "w") as mf:
        mf.write("\n".join(manifest_lines) + "\n")
        
    lic_src = os.path.join(SOURCES_DIR, "bucklespring", "COPYING")
    if os.path.exists(lic_src):
        shutil.copy(lic_src, os.path.join(pack_dir, "LICENSE"))
    else:
        with open(os.path.join(pack_dir, "LICENSE"), "w") as lf:
            lf.write("GNU GENERAL PUBLIC LICENSE Version 2\nhttps://github.com/zevv/bucklespring\n")
            
    return create_archive_and_entry(pack_dir, pack_info)

def build_alps_skcm_blue(pack_info, temp_root):
    pack_id = pack_info["id"]
    pack_dir = os.path.join(temp_root, pack_id)
    sounds_press = os.path.join(pack_dir, "sounds", "press")
    sounds_release = os.path.join(pack_dir, "sounds", "release")
    os.makedirs(sounds_press, exist_ok=True)
    os.makedirs(sounds_release, exist_ok=True)
    
    src_dir = os.path.join(SOURCES_DIR, "kbsim", "src", "assets", "audio", "bluealps")
    for f in sorted(os.listdir(os.path.join(src_dir, "press"))):
        shutil.copy(os.path.join(src_dir, "press", f), os.path.join(sounds_press, f))
    for f in sorted(os.listdir(os.path.join(src_dir, "release"))):
        shutil.copy(os.path.join(src_dir, "release", f), os.path.join(sounds_release, f))
        
    manifest = """format = 1
id = "alps-skcm-blue"
name = "Alps SKCM Blue"
family = "clicky"
license = "MIT"
source = "https://github.com/tplai/kbsim"
credits = "tplai / kbsim"

[press]
default = [
  "sounds/press/GENERIC_R0.mp3",
  "sounds/press/GENERIC_R1.mp3",
  "sounds/press/GENERIC_R2.mp3",
  "sounds/press/GENERIC_R3.mp3",
  "sounds/press/GENERIC_R4.mp3"
]
"57" = "sounds/press/SPACE.mp3"
"28" = "sounds/press/ENTER.mp3"
"14" = "sounds/press/BACKSPACE.mp3"

[release]
default = [
  "sounds/release/GENERIC.mp3",
  "sounds/release/GENERIC_long.mp3"
]
"57" = "sounds/release/SPACE.mp3"
"28" = "sounds/release/ENTER.mp3"
"14" = "sounds/release/BACKSPACE.mp3"
"""
    with open(os.path.join(pack_dir, "manifest.toml"), "w") as mf:
        mf.write(manifest)
        
    lic_src = os.path.join(SOURCES_DIR, "kbsim", "LICENSE")
    if os.path.exists(lic_src):
        shutil.copy(lic_src, os.path.join(pack_dir, "LICENSE"))
    else:
        with open(os.path.join(pack_dir, "LICENSE"), "w") as lf:
            lf.write("MIT License\n\nCopyright (c) 2020 tplai\nhttps://github.com/tplai/kbsim\n")
            
    return create_archive_and_entry(pack_dir, pack_info)

def build_typewriter(pack_info, temp_root):
    pack_id = pack_info["id"]
    pack_dir = os.path.join(temp_root, pack_id)
    sounds_dir = os.path.join(pack_dir, "sounds")
    os.makedirs(sounds_dir, exist_ok=True)
    
    src_dir = os.path.join(SOURCES_DIR, "keyboardsounds", "keyboardsounds", "profiles", "typewriter")
    for f in sorted(os.listdir(src_dir)):
        if f.endswith(".wav"):
            shutil.copy(os.path.join(src_dir, f), os.path.join(sounds_dir, f))
            
    manifest = """format = 1
id = "typewriter"
name = "Electric Typewriter"
family = "typewriter"
license = "GPL-3.0"
source = "https://github.com/nathan-fiscaletti/keyboardsounds"
credits = "Nathan Fiscaletti / keyboardsounds"

[press]
default = [
  "sounds/key.wav",
  "sounds/key2.wav"
]
"57" = "sounds/space.wav"
"28" = "sounds/enter.wav"
"14" = "sounds/delete.wav"
"""
    with open(os.path.join(pack_dir, "manifest.toml"), "w") as mf:
        mf.write(manifest)
        
    lic_src = os.path.join(SOURCES_DIR, "keyboardsounds", "LICENSE")
    if os.path.exists(lic_src):
        shutil.copy(lic_src, os.path.join(pack_dir, "LICENSE"))
    else:
        with open(os.path.join(pack_dir, "LICENSE"), "w") as lf:
            lf.write("GNU GENERAL PUBLIC LICENSE Version 3\nhttps://github.com/nathan-fiscaletti/keyboardsounds\n")
            
    return create_archive_and_entry(pack_dir, pack_info)

def build_telios_v2(pack_info, temp_root):
    pack_id = pack_info["id"]
    pack_dir = os.path.join(temp_root, pack_id)
    sounds_dir = os.path.join(pack_dir, "sounds")
    os.makedirs(sounds_dir, exist_ok=True)
    
    src_dir = os.path.join(SOURCES_DIR, "keyboardsounds", "keyboardsounds", "profiles", "telios-v2")
    for f in sorted(os.listdir(src_dir)):
        if f.endswith(".wav"):
            shutil.copy(os.path.join(src_dir, f), os.path.join(sounds_dir, f))
            
    manifest = """format = 1
id = "telios-v2"
name = "Zeal Tealios V2"
family = "linear"
license = "GPL-3.0"
source = "https://github.com/nathan-fiscaletti/keyboardsounds"
credits = "Nathan Fiscaletti / keyboardsounds"

[press]
default = [
  "sounds/key1.wav",
  "sounds/key2.wav",
  "sounds/key3.wav",
  "sounds/key4.wav",
  "sounds/key5.wav",
  "sounds/key6.wav"
]
"57" = "sounds/space.wav"
"28" = "sounds/ent.wav"
"14" = "sounds/back.wav"
"42" = "sounds/shift.wav"
"54" = "sounds/shift.wav"
"15" = "sounds/tab.wav"
"""
    with open(os.path.join(pack_dir, "manifest.toml"), "w") as mf:
        mf.write(manifest)
        
    lic_src = os.path.join(SOURCES_DIR, "keyboardsounds", "LICENSE")
    if os.path.exists(lic_src):
        shutil.copy(lic_src, os.path.join(pack_dir, "LICENSE"))
    else:
        with open(os.path.join(pack_dir, "LICENSE"), "w") as lf:
            lf.write("GNU GENERAL PUBLIC LICENSE Version 3\nhttps://github.com/nathan-fiscaletti/keyboardsounds\n")
            
    return create_archive_and_entry(pack_dir, pack_info)

def build_kailh_box_white(pack_info, temp_root):
    pack_id = pack_info["id"]
    pack_dir = os.path.join(temp_root, pack_id)
    sounds_dir = os.path.join(pack_dir, "sounds")
    os.makedirs(sounds_dir, exist_ok=True)
    
    src_dir = os.path.join(SOURCES_DIR, "wayclick_soundpacks", "kailh_box_white")
    for f in sorted(os.listdir(src_dir)):
        if f.endswith(".mp3"):
            shutil.copy(os.path.join(src_dir, f), os.path.join(sounds_dir, f))
            
    clips = [f"sounds/kboxwh - {i:02d}.mp3" for i in range(1, 11)]
    manifest_lines = [
        "format = 1",
        f'id = "{pack_info["id"]}"',
        f'name = "{pack_info["name"]}"',
        f'family = "{pack_info["family"]}"',
        f'license = "{pack_info["license"]}"',
        'source = "https://github.com/dusklinux/wayclick_soundpacks"',
        'credits = "Mechvibes community / wayclick"',
        "",
        "[press]",
        f"default = {json.dumps(clips)}",
        '"28" = "sounds/enter.mp3"',
        '"56" = "sounds/alt.mp3"',
        '"100" = "sounds/alt.mp3"'
    ]
    with open(os.path.join(pack_dir, "manifest.toml"), "w") as mf:
        mf.write("\n".join(manifest_lines) + "\n")
        
    with open(os.path.join(pack_dir, "LICENSE"), "w") as lf:
        lf.write("Creative Commons Attribution / Community Open Soundpack\nOriginally recorded by Mechvibes community contributors.\n")
        
    return create_archive_and_entry(pack_dir, pack_info)

def build_cream_travel(pack_info, temp_root):
    pack_id = pack_info["id"]
    pack_dir = os.path.join(temp_root, pack_id)
    sounds_press = os.path.join(pack_dir, "sounds", "press")
    sounds_release = os.path.join(pack_dir, "sounds", "release")
    os.makedirs(sounds_press, exist_ok=True)
    os.makedirs(sounds_release, exist_ok=True)
    
    src_dir = os.path.join(SOURCES_DIR, "mechvibes", "src", "audio", "cream-travel")
    for f in sorted(os.listdir(os.path.join(src_dir, "press"))):
        shutil.copy(os.path.join(src_dir, "press", f), os.path.join(sounds_press, f))
    for f in sorted(os.listdir(os.path.join(src_dir, "release"))):
        shutil.copy(os.path.join(src_dir, "release", f), os.path.join(sounds_release, f))
        
    manifest = """format = 1
id = "cream-travel"
name = "NovelKeys Cream (Full Travel)"
family = "linear"
license = "MIT"
source = "https://github.com/hainguyents13/mechvibes"
credits = "Mechvibes contributors"

[press]
default = [
  "sounds/press/GENERIC_R0.mp3",
  "sounds/press/GENERIC_R1.mp3",
  "sounds/press/GENERIC_R2.mp3",
  "sounds/press/GENERIC_R3.mp3",
  "sounds/press/GENERIC_R4.mp3"
]
"57" = "sounds/press/SPACE.mp3"
"28" = "sounds/press/ENTER.mp3"
"14" = "sounds/press/BACKSPACE.mp3"

[release]
default = "sounds/release/GENERIC.mp3"
"57" = "sounds/release/SPACE.mp3"
"28" = "sounds/release/ENTER.mp3"
"14" = "sounds/release/BACKSPACE.mp3"
"""
    with open(os.path.join(pack_dir, "manifest.toml"), "w") as mf:
        mf.write(manifest)
        
    with open(os.path.join(pack_dir, "LICENSE"), "w") as lf:
        lf.write("MIT License\n\nCopyright (c) 2020 hainguyen\nhttps://github.com/hainguyents13/mechvibes\n")
        
    return create_archive_and_entry(pack_dir, pack_info)

def ensure_sources():
    os.makedirs(SOURCES_DIR, exist_ok=True)
    repos = [
        ("bucklespring", "https://github.com/zevv/bucklespring.git"),
        ("kbsim", "https://github.com/tplai/kbsim.git"),
        ("keyboardsounds", "https://github.com/nathan-fiscaletti/keyboardsounds.git"),
        ("wayclick_soundpacks", "https://github.com/dusklinux/wayclick_soundpacks.git"),
        ("mechvibes", "https://github.com/hainguyents13/mechvibes.git"),
    ]
    for name, url in repos:
        target = os.path.join(SOURCES_DIR, name)
        if not os.path.exists(target):
            print(f"--> Cloning {name} from {url}...")
            import subprocess
            subprocess.run(["git", "clone", "--depth", "1", url, target], check=True)

def main():
    ensure_sources()
    os.makedirs(DIST_DIR, exist_ok=True)
    os.makedirs("dist", exist_ok=True)
    temp_root = "/tmp/clack_build_staging"
    if os.path.exists(temp_root):
        shutil.rmtree(temp_root)
    os.makedirs(temp_root, exist_ok=True)

    
    catalog_entries = []
    
    # ── 1. IBM Model M ────────────────────────────────────────────────────────
    print("==> Building IBM Model M...")
    catalog_entries.append(build_ibm_model_m({
        "id": "ibm-model-m",
        "name": "IBM Model M (1985)",
        "family": "buckling",
        "license": "GPL-2.0",
        "starter": True,
        "blurb": "The 1985 buckling-spring original. Loud, proud, indestructible."
    }, temp_root))

    # ── 2. Alps SKCM Blue ─────────────────────────────────────────────────────
    print("==> Building Alps SKCM Blue...")
    catalog_entries.append(build_alps_skcm_blue({
        "id": "alps-skcm-blue",
        "name": "Alps SKCM Blue",
        "family": "clicky",
        "license": "MIT",
        "starter": True,
        "blurb": "Vintage complicated blue Alps with tactile click leaf."
    }, temp_root))

    # ── 3. Electric Typewriter ────────────────────────────────────────────────
    print("==> Building Electric Typewriter...")
    catalog_entries.append(build_typewriter({
        "id": "typewriter",
        "name": "Electric Typewriter",
        "family": "typewriter",
        "license": "GPL-3.0",
        "starter": True,
        "blurb": "Vintage typewriter mechanical striker and carriage action."
    }, temp_root))

    # ── 4. Cherry MX Blue (PBT) ───────────────────────────────────────────────
    print("==> Building Cherry MX Blue (PBT)...")
    catalog_entries.append(build_mechvibes_sprite("cherrymx-blue-pbt", {
        "id": "cherrymx-blue-pbt",
        "name": "Cherry MX Blue (PBT)",
        "family": "clicky",
        "license": "MIT",
        "starter": True,
        "blurb": "Crisp clicky switch with dense, deeper PBT keycaps."
    }, temp_root))

    # ── 5. Cherry MX Blue (ABS) ───────────────────────────────────────────────
    print("==> Building Cherry MX Blue (ABS)...")
    catalog_entries.append(build_mechvibes_sprite("cherrymx-blue-abs", {
        "id": "cherrymx-blue-abs",
        "name": "Cherry MX Blue (ABS)",
        "family": "clicky",
        "license": "MIT",
        "starter": True,
        "blurb": "Crisp and classic clicky switch with ABS keycaps."
    }, temp_root))

    # ── 6. Cherry MX Brown (PBT) ──────────────────────────────────────────────
    print("==> Building Cherry MX Brown (PBT)...")
    catalog_entries.append(build_mechvibes_sprite("cherrymx-brown-pbt", {
        "id": "cherrymx-brown-pbt",
        "name": "Cherry MX Brown (PBT)",
        "family": "tactile",
        "license": "MIT",
        "starter": True,
        "blurb": "Subtle tactile bump paired with solid PBT keycaps."
    }, temp_root))

    # ── 7. Cherry MX Brown (ABS) ──────────────────────────────────────────────
    print("==> Building Cherry MX Brown (ABS)...")
    catalog_entries.append(build_mechvibes_sprite("cherrymx-brown-abs", {
        "id": "cherrymx-brown-abs",
        "name": "Cherry MX Brown (ABS)",
        "family": "tactile",
        "license": "MIT",
        "starter": True,
        "blurb": "Balanced tactile bump with muted bottom-out."
    }, temp_root))

    # ── 8. Cherry MX Black (ABS) ──────────────────────────────────────────────
    print("==> Building Cherry MX Black (ABS)...")
    catalog_entries.append(build_mechvibes_sprite("cherrymx-black-abs", {
        "id": "cherrymx-black-abs",
        "name": "Cherry MX Black (ABS)",
        "family": "linear",
        "license": "MIT",
        "starter": True,
        "blurb": "Heavy smooth linear switch."
    }, temp_root))

    # ── 9. Kailh Box White ────────────────────────────────────────────────────
    print("==> Building Kailh Box White...")
    catalog_entries.append(build_kailh_box_white({
        "id": "kailh-box-white",
        "name": "Kailh Box White",
        "family": "clicky",
        "license": "Community / CC-BY",
        "starter": True,
        "blurb": "Modern clickbar switch with sharp click on press and release."
    }, temp_root))

    # ── 10. NovelKeys Cream (Full Travel) ─────────────────────────────────────
    print("==> Building NovelKeys Cream (Full Travel)...")
    catalog_entries.append(build_cream_travel({
        "id": "cream-travel",
        "name": "NovelKeys Cream (Full Travel)",
        "family": "linear",
        "license": "MIT",
        "starter": True,
        "blurb": "Self-lubricating POM linear switch with full travel bottom-out."
    }, temp_root))

    # ── 11. Zeal Tealios V2 ───────────────────────────────────────────────────
    print("==> Building Zeal Tealios V2...")
    catalog_entries.append(build_telios_v2({
        "id": "telios-v2",
        "name": "Zeal Tealios V2",
        "family": "linear",
        "license": "GPL-3.0",
        "starter": True,
        "blurb": "Ultra-smooth premium linear switch."
    }, temp_root))

    # ── 12. Cherry MX Red (PBT) ───────────────────────────────────────────────
    print("==> Building Cherry MX Red (PBT)...")
    catalog_entries.append(build_mechvibes_sprite("cherrymx-red-pbt", {
        "id": "cherrymx-red-pbt",
        "name": "Cherry MX Red (PBT)",
        "family": "linear",
        "license": "MIT",
        "starter": False,
        "blurb": "Light linear travel with deep PBT thock."
    }, temp_root))

    # ── 13. Everglide Oreo ────────────────────────────────────────────────────
    print("==> Building Everglide Oreo...")
    catalog_entries.append(build_mechvibes_sprite("eg-oreo", {
        "id": "eg-oreo",
        "name": "Everglide Oreo",
        "family": "tactile",
        "license": "MIT",
        "starter": False,
        "blurb": "Snappy tactile switch with sharp feedback."
    }, temp_root))

    catalog = {"packs": catalog_entries}
    with open(INDEX_PATH, "w") as f:
        json.dump(catalog, f, indent=2)
    assets_index = os.path.abspath("assets/index.json")
    with open(assets_index, "w") as f:
        json.dump(catalog, f, indent=2)
    print(f"\n✓ Generated {INDEX_PATH} and {assets_index} with {len(catalog_entries)} packs.")


    shutil.rmtree(temp_root, ignore_errors=True)

if __name__ == "__main__":
    main()
