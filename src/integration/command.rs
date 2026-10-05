use std::path::Path;

#[cfg(any(windows, test))]
use base64::Engine;

pub(crate) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(crate) fn hook_command(hook_path: &Path, action: Option<&str>) -> String {
    let path = hook_path.display().to_string();
    #[cfg(windows)]
    {
        let mut command = format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File {}",
            windows_command_quote(&path)
        );
        if let Some(action) = action {
            command.push(' ');
            command.push_str(action);
        }
        command
    }

    #[cfg(not(windows))]
    {
        let mut command = format!("bash {}", shell_single_quote(&path));
        if let Some(action) = action {
            command.push(' ');
            command.push_str(action);
        }
        command
    }
}

/// Spellings of `hook_command` that reach the hook through the home directory
/// instead of an absolute path. A config shared between machines, such as a
/// dotfiles checkout, can only hold one of these.
pub(crate) fn home_relative_hook_commands(
    hook_path: &Path,
    home: Option<&Path>,
    action: &str,
) -> Vec<String> {
    // The Windows command runs PowerShell, which has no such spelling.
    if cfg!(windows) {
        return Vec::new();
    }
    let Some(relative) = home.and_then(|home| hook_path.strip_prefix(home).ok()) else {
        return Vec::new();
    };
    let relative = relative.display();
    vec![
        format!("bash \"$HOME/{relative}\" {action}"),
        format!("bash \"${{HOME}}/{relative}\" {action}"),
        format!("bash $HOME/{relative} {action}"),
        format!("bash ${{HOME}}/{relative} {action}"),
        format!("bash ~/{relative} {action}"),
    ]
}

#[cfg(any(windows, test))]
pub(crate) fn powershell_encoded_hook_command(hook_path: &Path, action: &str) -> String {
    let path = hook_path.display().to_string().replace('\'', "''");
    let script = format!("& '{path}' {action}");
    let encoded_script = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let encoded = base64::engine::general_purpose::STANDARD.encode(encoded_script);
    format!("powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {encoded}")
}

pub(crate) fn legacy_bash_hook_command(hook_path: &Path, action: Option<&str>) -> String {
    let mut command = format!(
        "bash {}",
        shell_single_quote(&hook_path.display().to_string())
    );
    if let Some(action) = action {
        command.push(' ');
        command.push_str(action);
    }
    command
}

#[cfg(windows)]
fn windows_command_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}
