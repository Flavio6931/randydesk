//! Script de compilación para incrustar el icono multi-resolución `Portal R` en `RandyDesk.exe`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let Ok(out_dir) = env::var("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let ico_path = out_dir.join("randydesk.ico");
    let rc_path = out_dir.join("randydesk.rc");
    let res_path = out_dir.join("randydesk.res");

    let ico_bytes = build_multi_size_ico(&[16, 32, 48, 256]);
    if fs::write(&ico_path, ico_bytes).is_err() {
        return;
    }

    let rc_content = "1 ICON \"randydesk.ico\"\r\n";
    if fs::write(&rc_path, rc_content).is_err() {
        return;
    }

    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=No se encontró rc.exe del Windows SDK; omitiendo icono .exe.");
        return;
    };

    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .current_dir(&out_dir)
        .status();

    if matches!(status, Ok(s) if s.success()) && res_path.exists() {
        println!("cargo:rustc-link-arg-bin=RandyDesk={}", res_path.display());
    } else {
        println!("cargo:warning=rc.exe falló compilando randydesk.rc");
    }
}

fn find_rc_exe() -> Option<PathBuf> {
    if let Ok(out) = Command::new("where.exe").arg("rc.exe").output() {
        if out.status.success() {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let p = PathBuf::from(line.trim());
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let entries = fs::read_dir(kits_bin).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path().join("x64").join("rc.exe"))
        .filter(|p| p.exists())
        .collect();
    candidates.sort();
    candidates.pop()
}

fn build_multi_size_ico(sizes: &[u32]) -> Vec<u8> {
    let mut images = Vec::with_capacity(sizes.len());
    for &sz in sizes {
        let rgba = render_portal_r_rgba(sz);
        images.push((sz, build_ico_dib_entry(sz, &rgba)));
    }

    let mut out = Vec::new();
    // ICONDIR header (6 bytes)
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut data_offset = 6 + images.len() * 16;
    for (sz, dib) in &images {
        let dim_byte = if *sz >= 256 { 0u8 } else { *sz as u8 };
        out.push(dim_byte);
        out.push(dim_byte);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data_offset as u32).to_le_bytes());
        data_offset += dib.len();
    }

    for (_, dib) in images {
        out.extend_from_slice(&dib);
    }
    out
}

fn build_ico_dib_entry(size: u32, rgba: &[u8]) -> Vec<u8> {
    let and_stride = ((size as usize).div_ceil(32)) * 4;
    let xor_bytes = (size as usize) * (size as usize) * 4;
    let and_bytes = and_stride * (size as usize);

    let mut buf = Vec::with_capacity(40 + xor_bytes + and_bytes);
    // BITMAPINFOHEADER (40 bytes)
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(size as i32).to_le_bytes());
    buf.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&32u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&((xor_bytes + and_bytes) as u32).to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Bottom-up BGRA rows
    for y in (0..size as usize).rev() {
        for x in 0..size as usize {
            let idx = (y * size as usize + x) * 4;
            buf.push(rgba[idx + 2]);
            buf.push(rgba[idx + 1]);
            buf.push(rgba[idx]);
            buf.push(rgba[idx + 3]);
        }
    }
    buf.resize(buf.len() + and_bytes, 0);
    buf
}

fn lerp_u8(a: u8, b: u8, t: f32) -> f32 {
    let tc = t.clamp(0.0, 1.0);
    (a as f32) + ((b as f32) - (a as f32)) * tc
}

fn render_portal_r_rgba(size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut acc_a = 0.0f32;

            for sy in 0..4 {
                for sx in 0..4 {
                    let x = ((px as f32) + (sx as f32 + 0.5) * 0.25) * 100.0 / (size as f32);
                    let y = ((py as f32) + (sy as f32 + 0.5) * 0.25) * 100.0 / (size as f32);
                    let (r, g, b, a) = sample_portal_r(x, y);
                    acc_r += r * a;
                    acc_g += g * a;
                    acc_b += b * a;
                    acc_a += a;
                }
            }

            let idx = ((py * size + px) * 4) as usize;
            if acc_a > 0.0 {
                out[idx] = (acc_r / acc_a).round().clamp(0.0, 255.0) as u8;
                out[idx + 1] = (acc_g / acc_a).round().clamp(0.0, 255.0) as u8;
                out[idx + 2] = (acc_b / acc_a).round().clamp(0.0, 255.0) as u8;
                out[idx + 3] = ((acc_a / 16.0) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

fn sample_portal_r(x: f32, y: f32) -> (f32, f32, f32, f32) {
    let rad = 24.0f32;
    let b = 50.0 - rad;
    let qx = (x - 50.0).abs() - b;
    let qy = (y - 50.0).abs() - b;
    let sd = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad;
    if sd > 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    // Live emerald node (top-right inside squircle)
    if (x - 78.5).hypot(y - 22.5) <= 5.5 {
        return (0x10 as f32, 0xB9 as f32, 0x81 as f32, 1.0);
    }

    // White R spine + top bowl minus inner cutout
    let in_spine = (26.0..=39.0).contains(&x) && (22.0..=78.0).contains(&y);
    let in_bowl_outer = ((26.0..=56.0).contains(&x) && (22.0..=60.0).contains(&y))
        || ((x - 56.0).hypot(y - 41.0) <= 19.0);
    let in_bowl_inner = ((39.0..=55.0).contains(&x) && (33.0..=49.0).contains(&y))
        || ((x - 55.0).hypot(y - 41.0) <= 8.0);
    if (in_spine || in_bowl_outer) && !in_bowl_inner {
        let t = (y - 22.0) / 56.0;
        return (
            lerp_u8(255, 0xD8, t),
            lerp_u8(255, 0xE8, t),
            255.0,
            1.0,
        );
    }

    // Diagonal Remote Beam Leg
    if (55.0..=78.0).contains(&y) {
        let u = (y - 55.0) / 23.0;
        let xl = 47.0 + 16.0 * u;
        let xr = 61.0 + 17.0 * u;
        if x >= xl && x <= xr {
            return (
                lerp_u8(0x3B, 0x06, u),
                lerp_u8(0x82, 0xB6, u),
                lerp_u8(0xF6, 0xD4, u),
                1.0,
            );
        }
    }

    // Border rim
    if sd > -2.0 {
        return (0x2D as f32, 0x46 as f32, 0x78 as f32, 1.0);
    }

    // Deep obsidian-cobalt background + subtle inner glow
    let bg_t = (x + y) / 200.0;
    let mut r = lerp_u8(0x10, 0x07, bg_t);
    let mut g = lerp_u8(0x1A, 0x0B, bg_t);
    let mut b_col = lerp_u8(0x33, 0x14, bg_t);
    let glow_d = (x - 50.0).hypot(y - 48.0);
    if glow_d < 44.0 {
        let k = (1.0 - glow_d / 44.0).powi(2) * 0.24;
        r = r * (1.0 - k) + (0x2F as f32) * k;
        g = g * (1.0 - k) + (0x81 as f32) * k;
        b_col = b_col * (1.0 - k) + (0xF7 as f32) * k;
    }
    (r, g, b_col, 1.0)
}
