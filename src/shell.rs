/// A shell the command line can run typed input through — Windows
/// Terminal-style "profile" (see `.claude/rules/litastum-theming.md`'s
/// sibling doc, `.claude/rules/litastum-stack.md`, for the reasoning:
/// only universally-preinstalled shells are built in, no PATH/registry
/// probing for Git Bash/WSL/pwsh yet).
#[derive(Debug, Clone)]
pub struct ShellProfile {
    /// Shown in the `Ctrl+P` picker and the command-line's right edge.
    pub name: String,
    pub program: String,
    /// Arguments before the typed command itself, e.g. `["/C"]` for
    /// `cmd`, or `["-NoLogo", "-Command"]` for PowerShell.
    pub args_prefix: Vec<String>,
}


impl ShellProfile {
    fn new(name: &str, program: &str, args_prefix: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            program: program.to_string(),
            args_prefix: args_prefix.iter().map(|arg| arg.to_string()).collect(),
        }
    }
}


/// The built-in profiles for this platform. Always non-empty; index
/// `0` is the default `App::active_shell` starts on.
pub fn builtin_profiles() -> Vec<ShellProfile> {
    profiles_for_platform()
}


#[cfg(windows)]
fn profiles_for_platform() -> Vec<ShellProfile> {
    vec![
        ShellProfile::new("Command Prompt", "cmd", &["/C"]),
        ShellProfile::new("PowerShell", "powershell", &["-NoLogo", "-Command"]),
    ]
}


#[cfg(not(windows))]
fn profiles_for_platform() -> Vec<ShellProfile> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    vec![ShellProfile::new("Shell", &shell, &["-c"])]
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_is_never_empty() {
        assert!(!builtin_profiles().is_empty());
    }
}
