//! What a run can't reach (design-going-forward.md, goal 1): the files and
//! directories that hold credentials, and the variables that carry them.
//! bash doesn't see them inside the boundary, and read refuses them, so a
//! push or a publish fails however the command is written.

use std::path::{Path, PathBuf};

/// Under the home directory: SSH and GPG keys, git credentials, the `gh`
/// token, cloud and registry keys, password stores.
const IN_HOME: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".git-credentials",
    ".config/git/credentials",
    ".netrc",
    ".config/gh",
    ".config/glab-cli",
    ".config/hub",
    ".aws",
    ".azure",
    ".config/gcloud",
    ".config/doctl",
    ".kube",
    ".docker",
    ".npmrc",
    ".pypirc",
    ".cargo/credentials",
    ".cargo/credentials.toml",
    ".gem/credentials",
    ".terraform.d",
    ".password-store",
    ".config/op",
    ".local/share/keyrings",
    "Library/Keychains",
];

/// The paths to hide: the credential files under `home`, and the user's
/// runtime directory (`XDG_RUNTIME_DIR`), where the SSH and GPG agents, the
/// keyring and the session bus listen.
pub fn hidden_paths(home: &Path, runtime_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = IN_HOME.iter().map(|p| home.join(p)).collect();
    v.extend(runtime_dir.map(Path::to_owned));
    v
}

/// Variables bash never sees: keys and tokens, the sockets of agents that
/// hold them, and startup files that would run before every command.
pub fn hidden_var(name: &str) -> bool {
    const ENDS: [&str; 5] = ["_KEY", "_TOKEN", "_SECRET", "_PASSWORD", "_CREDENTIALS"];
    const NAMES: [&str; 9] = [
        "BASH_ENV",
        "ENV",
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "SSH_ASKPASS",
        "GIT_ASKPASS",
        "GPG_AGENT_INFO",
        "DBUS_SESSION_BUS_ADDRESS",
        "KUBECONFIG",
    ];
    ENDS.iter().any(|e| name.ends_with(e)) || NAMES.contains(&name)
}

/// Set for bash on top of the hidden variables: git asks no credential
/// helper (an empty value clears the list from the config files), so a
/// token kept in a keychain is not handed out either.
pub const GIT_NO_HELPER: [(&str, &str); 3] = [
    ("GIT_CONFIG_COUNT", "1"),
    ("GIT_CONFIG_KEY_0", "credential.helper"),
    ("GIT_CONFIG_VALUE_0", ""),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_paths_and_variables() {
        let h = hidden_paths(Path::new("/h"), Some(Path::new("/run/user/1")));
        for p in [
            "/h/.ssh",
            "/h/.config/gh",
            "/h/.git-credentials",
            "/run/user/1",
        ] {
            assert!(h.contains(&PathBuf::from(p)), "{p}");
        }
        for v in [
            "OPENAI_API_KEY",
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "NPM_TOKEN",
            "SSH_AUTH_SOCK",
            "BASH_ENV",
        ] {
            assert!(hidden_var(v), "{v}");
        }
        for v in ["PATH", "HOME", "TMPDIR", "CARGO_HOME", "KEYBOARD"] {
            assert!(!hidden_var(v), "{v}");
        }
    }
}
