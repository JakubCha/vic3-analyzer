//! Binary (normal / ironman) saves without a token map: melt them to plaintext
//! with an external [Rakaly CLI](https://github.com/rakaly/cli), whose release
//! builds embed the Vic3 tokens this project may not redistribute.
//!
//! Lookup order: `VIC3_RAKALY`, `rakaly(.exe)` next to the running executable,
//! then `PATH`.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const EXE: &str = if cfg!(windows) { "rakaly.exe" } else { "rakaly" };

/// Where Rakaly lives, or `None` when it is not installed.
pub fn rakaly_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("VIC3_RAKALY").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(EXE)));
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(EXE));
    beside_exe
        .into_iter()
        .chain(on_path)
        .find(|path| path.is_file())
}

/// Melt binary save bytes to plaintext via `rakaly melt --format vic3`.
///
/// Unknown tokens (Rakaly older than the game patch) are kept as hex keys
/// rather than failing, so a fresh patch degrades to missing fields.
///
/// # Errors
///
/// A user-facing message: Rakaly missing, failed to start, or produced nothing.
pub fn melt(save_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let path = rakaly_path().ok_or(
        "binary save and no token map. Install Rakaly CLI (https://github.com/rakaly/cli/releases) \
         next to vic3-analyzer.exe or set VIC3_RAKALY, or run the game with -debug_mode \
         for plaintext saves",
    )?;
    let mut command = Command::new(&path);
    command
        .args(["melt", "--format", "vic3", "--unknown-key", "stringify", "--to-stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // No console window flashing up from the GUI on every autosave.
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", path.display()))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = save_bytes.to_vec();
    // Feed stdin on a thread: a large save would otherwise deadlock against a full stdout pipe.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|e| format!("rakaly failed: {e}"))?;
    let _ = writer.join();
    // Exit 1 with output = melted, but some tokens were unknown (stringified).
    if output.stdout.is_empty() || !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "rakaly could not melt the save ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}
