use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn main() -> Result<(), String> {
    ensure_cmd_exists("yt-dlp")?;
    ensure_cmd_exists("ffmpeg")?;

    println!("youtube: ");
    let youtube = read_line()?.trim().to_string();
    if youtube.is_empty() {
        return Err("youtube link is empty".into());
    }

    println!("time (optional, example: 0:00 - 0:50). Press Enter for full: ");
    let time_raw = read_line()?.trim().to_string();
    let download_sections = if time_raw.is_empty() {
        None
    } else {
        parse_time_range_to_yt_dlp_sections(&time_raw)
    };

    println!("name (optional, default: clip): ");
    let name_raw = read_line()?.trim().to_string();
    let base = if name_raw.is_empty() {
        "clip".to_string()
    } else {
        sanitize_filename(&name_raw)
    };

    let out_dir = PathBuf::from("output");
    fs::create_dir_all(&out_dir).map_err(|e| format!("Failed to create output dir: {e}"))?;

    let mp4_path = out_dir.join(format!("{base}.mp4"));
    let mp3_path = out_dir.join(format!("{base}.mp3"));
    let srt_path = out_dir.join(format!("{base}.en.srt"));
    let txt_path = out_dir.join(format!("{base}.en.txt"));

    // 1) MP4 (QuickTime-friendly)
    run_yt_dlp_mp4(&youtube, download_sections.as_deref(), &mp4_path)?;

    // 2) MP3
    run_yt_dlp_mp3(&youtube, download_sections.as_deref(), &mp3_path)?;

    // 3) EN transcript (SRT)
    run_yt_dlp_english_srt(&youtube, &out_dir, &base)?;

    // Rename produced SRT to stable name
    let produced_srt = find_first_srt_in_dir(&out_dir)
        .ok_or_else(|| "Could not find any .srt produced by yt-dlp in output dir".to_string())?;
    fs::rename(&produced_srt, &srt_path).map_err(|e| format!("Failed to rename SRT: {e}"))?;

    // Create clean TXT from SRT
    srt_to_clean_txt(&srt_path, &txt_path)?;

    println!("\nDone ✅");
    println!("MP4: {}", mp4_path.display());
    println!("MP3: {}", mp3_path.display());
    println!("SRT: {}", srt_path.display());
    println!("TXT: {}", txt_path.display());

    Ok(())
}

fn read_line() -> Result<String, String> {
    let mut s = String::new();
    io::stdout().flush().ok();
    io::stdin()
        .read_line(&mut s)
        .map_err(|e| format!("Failed to read input: {e}"))?;
    Ok(s)
}

fn run_yt_dlp_mp4(url: &str, sections: Option<&str>, out_file: &Path) -> Result<(), String> {
    let mut cmd = Command::new("yt-dlp");
    if let Some(s) = sections {
        cmd.arg("--download-sections").arg(s);
    }
    cmd.args([
        "-f",
        "bv*[vcodec^=avc1][ext=mp4]+ba[ext=m4a]/b[ext=mp4]",
        "--merge-output-format",
        "mp4",
        "-o",
    ])
    .arg(out_file.to_string_lossy().to_string())
    .arg(url);

    run_cmd(&mut cmd, "yt-dlp mp4")
}

fn run_yt_dlp_mp3(url: &str, sections: Option<&str>, out_file: &Path) -> Result<(), String> {
    let mut cmd = Command::new("yt-dlp");
    if let Some(s) = sections {
        cmd.arg("--download-sections").arg(s);
    }
    cmd.args([
        "--extract-audio",
        "--audio-format",
        "mp3",
        "--audio-quality",
        "0",
        "-o",
    ])
    .arg(out_file.to_string_lossy().to_string())
    .arg(url);

    run_cmd(&mut cmd, "yt-dlp mp3")
}

fn run_yt_dlp_english_srt(url: &str, out_dir: &Path, base: &str) -> Result<(), String> {
    // Output template in the same folder
    let out_tmpl = out_dir.join(format!("{base}.%(ext)s"));

    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "--skip-download",
        "--write-auto-subs",
        "--sub-langs",
        "en.*",
        "--convert-subs",
        "srt",
        "-o",
    ])
    .arg(out_tmpl.to_string_lossy().to_string())
    .arg(url);

    run_cmd(&mut cmd, "yt-dlp transcript (en srt)")
}

fn srt_to_clean_txt(srt_path: &Path, txt_path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(srt_path)
        .map_err(|e| format!("Failed to read SRT {}: {e}", srt_path.display()))?;

    let mut out_lines = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if t.contains("-->") {
            continue;
        }
        out_lines.push(t.to_string());
    }

    fs::write(txt_path, out_lines.join("\n"))
        .map_err(|e| format!("Failed to write TXT {}: {e}", txt_path.display()))?;
    Ok(())
}

fn find_first_srt_in_dir(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("srt"))
            == Some(true)
        {
            return Some(p);
        }
    }
    None
}

fn parse_time_range_to_yt_dlp_sections(input: &str) -> Option<String> {
    // Accept: "0:00-0:50" or "0:00 - 0:50"
    let normalized = input.replace(' ', "");
    let parts: Vec<&str> = normalized.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parts[0];
    let end = parts[1];
    if start.is_empty() || end.is_empty() {
        return None;
    }
    Some(format!("*{start}-{end}"))
}

fn sanitize_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        let ok = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.';
        out.push(if ok { ch } else { '_' });
    }
    if out.is_empty() { "clip".into() } else { out }
}

fn ensure_cmd_exists(cmd: &str) -> Result<(), String> {
    let status = Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {cmd} >/dev/null 2>&1"))
        .status()
        .map_err(|e| format!("Failed to check command '{cmd}': {e}"))?;

    if !status.success() {
        return Err(format!(
            "Missing dependency: '{cmd}'. Install it (brew install {cmd})"
        ));
    }
    Ok(())
}

fn run_cmd(cmd: &mut Command, label: &str) -> Result<(), String> {
    cmd.stdin(Stdio::null());
    let status = cmd
        .status()
        .map_err(|e| format!("Failed to run {label}: {e}"))?;
    if !status.success() {
        return Err(format!("{label} failed with exit code: {status}"));
    }
    Ok(())
}
