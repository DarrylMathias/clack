#!/usr/bin/env python3
"""
Downloads open-source starter sound packs from upstream Mechvibes,
converts them into clack native packs (manifest.toml + sound.ogg + LICENSE),
compresses them into .tar.gz archives, and writes index.json.
"""

import hashlib
import json
import os
import shutil
import tarfile
import urllib.request

PACKS = [
    {
        "id": "cherrymx-blue-abs",
        "name": "Cherry MX Blue (ABS)",
        "family": "clicky",
        "upstream": "cherrymx-blue-abs",
        "license": "MIT",
        "starter": True,
        "blurb": "Crisp and classic clicky switch with ABS keycaps"
    },
    {
        "id": "cherrymx-brown-abs",
        "name": "Cherry MX Brown (ABS)",
        "family": "tactile",
        "upstream": "cherrymx-brown-abs",
        "license": "MIT",
        "starter": True,
        "blurb": "Balanced tactile bump with muted bottom-out"
    },
    {
        "id": "cherrymx-black-abs",
        "name": "Cherry MX Black (ABS)",
        "family": "linear",
        "upstream": "cherrymx-black-abs",
        "license": "MIT",
        "starter": True,
        "blurb": "Heavy smooth linear switch"
    },
    {
        "id": "cherrymx-red-pbt",
        "name": "Cherry MX Red (PBT)",
        "family": "linear",
        "upstream": "cherrymx-red-pbt",
        "license": "MIT",
        "starter": True,
        "blurb": "Light linear travel with deep PBT thock"
    },
    {
        "id": "eg-oreo",
        "name": "Everglide Oreo",
        "family": "tactile",
        "upstream": "eg-oreo",
        "license": "MIT",
        "starter": True,
        "blurb": "Snappy tactile switch with sharp feedback"
    }
]

BASE_URL = "https://raw.githubusercontent.com/hainguyents13/mechvibes/master/src/audio"
DIST_DIR = os.path.abspath("dist/packs")
INDEX_PATH = os.path.abspath("dist/index.json")

# Mapping of Mechvibes JS keycodes to Linux evdev keycodes
# Mechvibes uses browser keycodes (e.g. 65 = A, 13 = Enter)
KEYMAP = {
    "1": 1,    # Esc -> KEY_ESC
    "2": 2,    # 1 -> KEY_1
    "3": 3,    # 2 -> KEY_2
    "4": 4,    # 3 -> KEY_3
    "5": 5,    # 4 -> KEY_4
    "6": 6,    # 5 -> KEY_5
    "7": 7,    # 6 -> KEY_6
    "8": 8,    # 7 -> KEY_7
    "9": 9,    # 8 -> KEY_8
    "10": 10,  # 9 -> KEY_9
    "11": 11,  # 0 -> KEY_0
    "12": 12,  # - -> KEY_MINUS
    "13": 13,  # = -> KEY_EQUAL
    "14": 14,  # Backspace -> KEY_BACKSPACE
    "15": 15,  # Tab -> KEY_TAB
    "16": 16,  # Q -> KEY_Q
    "17": 17,  # W -> KEY_W
    "18": 18,  # E -> KEY_E
    "19": 19,  # R -> KEY_R
    "20": 20,  # T -> KEY_T
    "21": 21,  # Y -> KEY_Y
    "22": 22,  # U -> KEY_U
    "23": 23,  # I -> KEY_I
    "24": 24,  # O -> KEY_O
    "25": 25,  # P -> KEY_P
    "26": 26,  # [ -> KEY_LEFTBRACE
    "27": 27,  # ] -> KEY_RIGHTBRACE
    "28": 28,  # Enter -> KEY_ENTER
    "29": 29,  # Left Ctrl -> KEY_LEFTCTRL
    "30": 30,  # A -> KEY_A
    "31": 31,  # S -> KEY_S
    "32": 32,  # D -> KEY_D
    "33": 33,  # F -> KEY_F
    "34": 34,  # G -> KEY_G
    "35": 35,  # H -> KEY_H
    "36": 36,  # J -> KEY_J
    "37": 37,  # K -> KEY_K
    "38": 38,  # L -> KEY_L
    "39": 39,  # ; -> KEY_SEMICOLON
    "40": 40,  # ' -> KEY_APOSTROPHE
    "41": 41,  # ` -> KEY_GRAVE
    "42": 42,  # Left Shift -> KEY_LEFTSHIFT
    "43": 43,  # \ -> KEY_BACKSLASH
    "44": 44,  # Z -> KEY_Z
    "45": 45,  # X -> KEY_X
    "46": 46,  # C -> KEY_C
    "47": 47,  # V -> KEY_V
    "48": 48,  # B -> KEY_B
    "49": 49,  # N -> KEY_N
    "50": 50,  # M -> KEY_M
    "51": 51,  # , -> KEY_COMMA
    "52": 52,  # . -> KEY_DOT
    "53": 53,  # / -> KEY_SLASH
    "54": 54,  # Right Shift -> KEY_RIGHTSHIFT
    "56": 56,  # Left Alt -> KEY_LEFTALT
    "57": 57,  # Space -> KEY_SPACE
    "58": 58,  # Caps Lock -> KEY_CAPSLOCK
    "97": 97,  # Right Ctrl -> KEY_RIGHTCTRL
    "100": 100,# Right Alt -> KEY_RIGHTALT
    "125": 125,# Left Super -> KEY_LEFTMETA
    "126": 126,# Right Super -> KEY_RIGHTMETA
}

def convert_mechvibes_config(config_json, pack_info):
    defines = config_json.get("defines", {})
    sound_file = config_json.get("sound", "sound.ogg")
    
    # Manifest TOML construction
    lines = [
        "format = 1",
        f'id = "{pack_info["id"]}"',
        f'name = "{pack_info["name"]}"',
        f'family = "{pack_info["family"]}"',
        f'license = "{pack_info["license"]}"',
        f'sprite = "{sound_file}"',
        "",
        "[press]",
    ]
    
    # Pick a default fallback slice (Space or key '57' or first define)
    default_slice = defines.get("57") or next(iter(defines.values())) if defines else [0, 200]
    lines.append(f"default = [{default_slice[0]}, {default_slice[1]}]")
    
    for mv_code, slice_info in defines.items():
        if isinstance(slice_info, list) and len(slice_info) == 2:
            # Map code if known, otherwise parse int
            ev_code = KEYMAP.get(str(mv_code), None)
            if ev_code is None:
                try:
                    ev_code = int(mv_code)
                except ValueError:
                    continue
            lines.append(f'"{ev_code}" = [{slice_info[0]}, {slice_info[1]}]')
            
    return "\n".join(lines) + "\n"

def main():
    os.makedirs(DIST_DIR, exist_ok=True)
    os.makedirs("dist", exist_ok=True)
    
    catalog_entries = []
    
    for p in PACKS:
        pack_id = p["id"]
        upstream = p["upstream"]
        print(f"==> Processing pack: {pack_id}...")
        
        raw_config_url = f"{BASE_URL}/{upstream}/config.json"
        raw_sound_url = f"{BASE_URL}/{upstream}/sound.ogg"
        
        temp_pack_dir = os.path.join(DIST_DIR, f"tmp_{pack_id}")
        if os.path.exists(temp_pack_dir):
            shutil.rmtree(temp_pack_dir)
        os.makedirs(temp_pack_dir, exist_ok=True)
        
        # Download config.json
        print(f"    Downloading config.json...")
        with urllib.request.urlopen(raw_config_url) as resp:
            config_data = json.loads(resp.read().decode("utf-8"))

        sound_filename = config_data.get("sound", "sound.ogg")
        raw_sound_url = f"{BASE_URL}/{upstream}/{sound_filename}"
            
        # Download sound file
        sound_path = os.path.join(temp_pack_dir, sound_filename)
        print(f"    Downloading {sound_filename}...")
        with urllib.request.urlopen(raw_sound_url) as resp, open(sound_path, "wb") as f:
            shutil.copyfileobj(resp, f)
            
        # Write manifest.toml
        manifest_str = convert_mechvibes_config(config_data, p)
        with open(os.path.join(temp_pack_dir, "manifest.toml"), "w") as f:
            f.write(manifest_str)
            
        # Write LICENSE
        with open(os.path.join(temp_pack_dir, "LICENSE"), "w") as f:
            f.write("MIT License\n\nCopyright (c) 2020 hainguyen\nhttps://github.com/hainguyents13/mechvibes\n")
            
        # Create tar.gz
        tar_filename = f"{pack_id}.tar.gz"
        tar_path = os.path.join(DIST_DIR, tar_filename)
        print(f"    Creating {tar_filename}...")
        with tarfile.open(tar_path, "w:gz") as tar:
            for item in os.listdir(temp_pack_dir):
                tar.add(os.path.join(temp_pack_dir, item), arcname=item)
                
        shutil.rmtree(temp_pack_dir)
        
        # Calculate size and sha256
        size = os.path.getsize(tar_path)
        with open(tar_path, "rb") as f:
            sha256 = hashlib.sha256(f.read()).hexdigest()
            
        print(f"    ✓ {tar_filename}: {size} bytes, sha256={sha256[:12]}...")
        
        catalog_entries.append({
            "id": pack_id,
            "name": p["name"],
            "family": p["family"],
            "license": p["license"],
            "starter": p["starter"],
            "size": size,
            "sha256": sha256,
            "url": f"https://github.com/DarrylMathias/clack/releases/latest/download/{tar_filename}",
            "blurb": p["blurb"]
        })
        
    catalog = {"packs": catalog_entries}
    with open(INDEX_PATH, "w") as f:
        json.dump(catalog, f, indent=2)
    print(f"\n✓ Generated {INDEX_PATH} with {len(catalog_entries)} packs.")

if __name__ == "__main__":
    main()
