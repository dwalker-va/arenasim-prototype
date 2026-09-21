//! `wand_rod.gltf` is the one weapon model AUTHORED in this repo rather than
//! downloaded, so it is the one that can be malformed in a way nothing else
//! catches: a bad accessor or a truncated buffer shows up only as a silently
//! missing weapon in the running client, and no other suite in the tree loads
//! a glTF at all.
//!
//! This validates the file structurally — the same things Bevy's `GltfLoader`
//! reads, checked against the decoded buffer — plus the geometry conventions
//! `weapon_mount` assumes.
//!
//! **What it does NOT check is how the rod LOOKS.** Driving Bevy's real
//! loader was tried and abandoned: the asset pipeline needs more plugin
//! infrastructure than a headless test app provides, and the control (the
//! shipped `dagger.gltf`) failed to load in exactly the same way — so such a
//! test would have measured the harness rather than the asset. Appearance is
//! a human check.

use std::path::PathBuf;

use serde_json::Value;

fn wand_gltf() -> Value {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "assets",
        "models",
        "weapons",
        "wand_rod.gltf",
    ]
    .iter()
    .collect();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is not readable: {e}", path.display()));
    serde_json::from_str(&text).expect("wand_rod.gltf is valid JSON")
}

/// The buffer, decoded from its embedded data URI. The file ships no sidecar
/// `.bin` and no texture, so this is the whole of its binary payload.
fn decoded_buffer(gltf: &Value) -> Vec<u8> {
    let buffers = gltf["buffers"].as_array().expect("buffers");
    assert_eq!(buffers.len(), 1, "one embedded buffer, no sidecar .bin");
    let uri = buffers[0]["uri"].as_str().expect("buffer uri");
    let b64 = uri
        .strip_prefix("data:application/octet-stream;base64,")
        .expect("the buffer is an embedded base64 data URI");
    let bytes = base64_decode(b64);
    assert_eq!(
        bytes.len() as u64,
        buffers[0]["byteLength"].as_u64().expect("byteLength"),
        "the declared byteLength does not match the decoded payload"
    );
    bytes
}

/// Minimal standard-alphabet base64, so the test needs no new dependency.
fn base64_decode(s: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = ALPHABET
            .iter()
            .position(|&a| a == c)
            .unwrap_or_else(|| panic!("non-base64 byte {c:?} in the buffer URI"))
            as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

#[test]
fn the_wand_rod_is_a_structurally_valid_gltf() {
    let gltf = wand_gltf();
    let bytes = decoded_buffer(&gltf);

    assert_eq!(gltf["asset"]["version"], "2.0");
    assert_eq!(gltf["scenes"].as_array().unwrap().len(), 1);
    assert_eq!(gltf["nodes"].as_array().unwrap().len(), 1);

    // Two primitives, two materials: the haft and the emissive gem. Losing
    // one is how the rod would quietly become a bare stick.
    let meshes = gltf["meshes"].as_array().expect("meshes");
    assert_eq!(meshes.len(), 1);
    let primitives = meshes[0]["primitives"].as_array().expect("primitives");
    assert_eq!(primitives.len(), 2, "haft + gem");
    assert_eq!(gltf["materials"].as_array().unwrap().len(), 2);
    assert!(
        gltf["materials"][1]["emissiveFactor"].is_array(),
        "the gem's material must carry an emissiveFactor"
    );

    // Every primitive carries the attributes the renderer needs: without
    // NORMAL, a primitive renders flat-black under a lit material.
    for p in primitives {
        assert!(p["attributes"]["POSITION"].is_number(), "POSITION");
        assert!(p["attributes"]["NORMAL"].is_number(), "NORMAL");
        assert!(p["indices"].is_number(), "indexed geometry");
    }

    // Every bufferView and accessor lies inside the decoded buffer. This is
    // the class of defect the generator can actually produce — an off-by-one
    // in an offset or a length — and the loader's symptom for it is an
    // invisible weapon.
    let views = gltf["bufferViews"].as_array().expect("bufferViews");
    for (i, v) in views.iter().enumerate() {
        let off = v["byteOffset"].as_u64().unwrap_or(0);
        let len = v["byteLength"].as_u64().expect("byteLength");
        assert!(
            off + len <= bytes.len() as u64,
            "bufferView {i} runs past the end of the buffer"
        );
    }
    let accessors = gltf["accessors"].as_array().expect("accessors");
    for (i, a) in accessors.iter().enumerate() {
        let view = &views[a["bufferView"].as_u64().expect("bufferView") as usize];
        let component = match a["componentType"].as_u64().expect("componentType") {
            5123 => 2, // u16
            5126 => 4, // f32
            other => panic!("accessor {i} uses unexpected componentType {other}"),
        };
        let components = match a["type"].as_str().expect("type") {
            "SCALAR" => 1,
            "VEC3" => 3,
            other => panic!("accessor {i} uses unexpected type {other}"),
        };
        let span = a["count"].as_u64().expect("count") * component * components;
        let off = a["byteOffset"].as_u64().unwrap_or(0);
        assert!(
            off + span <= view["byteLength"].as_u64().unwrap(),
            "accessor {i} runs past its bufferView"
        );
    }

    // Every index addresses a real vertex. An out-of-range index is the other
    // way the generator could produce a file that parses and then renders
    // garbage.
    let vertex_count = accessors[0]["count"].as_u64().expect("POSITION count");
    for a in accessors.iter().filter(|a| a["type"] == "SCALAR") {
        let view = &views[a["bufferView"].as_u64().unwrap() as usize];
        let base = view["byteOffset"].as_u64().unwrap_or(0) + a["byteOffset"].as_u64().unwrap_or(0);
        for k in 0..a["count"].as_u64().unwrap() {
            let at = (base + k * 2) as usize;
            let index = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as u64;
            assert!(
                index < vertex_count,
                "index {index} addresses vertex {index} of {vertex_count}"
            );
        }
    }
}

#[test]
fn the_wand_rod_matches_the_mount_convention() {
    // Read off the POSITION accessor's declared bounds, which are what a
    // viewer and the loader both use for the model's extent.
    let gltf = wand_gltf();
    let pos = &gltf["accessors"][0];
    assert_eq!(pos["type"], "VEC3");
    let min: Vec<f64> = pos["min"]
        .as_array()
        .expect("POSITION accessors must declare min/max")
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let max: Vec<f64> = pos["max"]
        .as_array()
        .expect("max")
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let size: Vec<f64> = (0..3).map(|i| max[i] - min[i]).collect();

    // Every weapon here is authored with its haft along local +Y and its grip
    // at y 0; `weapon_mount` composes aim and scale on top of that.
    assert!(
        min[1].abs() < 1e-5,
        "the grip sits at local y {}, not 0",
        min[1]
    );
    assert!(
        size[1] > 0.3 && size[1] < 0.6,
        "the rod is {} long — a wand, not a staff",
        size[1]
    );
    assert!(
        size[0] < size[1] * 0.5 && size[2] < size[1] * 0.5,
        "the rod is {size:?} — too thick to read as a wand"
    );

    // Low-poly, like the rest of the CC0 set it stands beside.
    let tris: u64 = gltf["accessors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["type"] == "SCALAR")
        .map(|a| a["count"].as_u64().unwrap())
        .sum::<u64>()
        / 3;
    assert!(tris > 0 && tris <= 64, "{tris} triangles");
}
