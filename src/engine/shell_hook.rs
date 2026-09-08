use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use portable_pty::CommandBuilder;

const BASH_NON_LOGIN_RC: &str = r#"[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
"#;

const BASH_LOGIN_PROFILE: &str = r#"if [ -n "${TERMDECK_HOOK_ORIGINAL_HOME+x}" ]; then
  export HOME="$TERMDECK_HOOK_ORIGINAL_HOME"
else
  unset HOME
fi
if [ -n "${HOME-}" ]; then
  if [ -r "$HOME/.bash_profile" ]; then . "$HOME/.bash_profile"
  elif [ -r "$HOME/.bash_login" ]; then . "$HOME/.bash_login"
  elif [ -r "$HOME/.profile" ]; then . "$HOME/.profile"
  fi
fi
"#;

/// Why HOME is scoped and restored here, and what that cannot reach.
///
/// Login bash has no startup-file override (no `--rcfile` for `-l`, and no
/// ZDOTDIR equivalent), so the only way to source a hook before the first
/// prompt is to own the `.bash_profile` the shell reads. This shim sets HOME
/// to the generated dir, runs the user's real profile chain (first-found
/// rule), then installs the hook — before bash paints its first prompt. That
/// is the #151 fix: one prompt, no fed command.
///
/// KNOWN RESIDUAL (accepted, see issue #152): bash sources `/etc/profile`
/// and `/etc/bash.bashrc` BEFORE `~/.bash_profile`, at a point where HOME is
/// still the generated (scoped) dir. So a login pane may repeat the sudo
/// hint (Ubuntu's `/etc/profile` prints it unless `~/.hushlogin` exists),
/// ignore `~/.hushlogin`, and miss the user's `bash_completion` — all
/// because those system files resolve `~` against the scoped HOME. There is
/// no mechanism that runs our profile earlier (bash has no ZDOTDIR
/// equivalent), and the previous fed-command approach was strictly worse.
/// Revisit if Ubuntu's `/etc/profile` gains a documented extension point.
const BASH_HOOK: &str = r#"case $- in *i*) ;; *) return;; esac
if [ -n "${TERMDECK_SHELL_HOOK-}" ] || [ -z "${TERMDECK_SOCK-}" ] || [ -z "${TERMDECK_PANE-}" ]; then return; fi
export TERMDECK_SHELL_HOOK=1
__td_start= __td_cmd=
__td_now() { if [ -n "${EPOCHSECONDS+x}" ]; then REPLY=$EPOCHSECONDS; else REPLY=$SECONDS; fi; }
__td_debug_trap=$(trap -p DEBUG)
__td_run_debug_trap() {
  [ -n "$__td_debug_trap" ] || return
  local __td_debug_action=${__td_debug_trap#trap -- }
  __td_debug_action=${__td_debug_action% DEBUG}
  eval "eval $__td_debug_action"
}
__td_preexec() {
  case $BASH_COMMAND in __td_*|__systemd_osc_context_*) return;; esac
  [ -n "${__td_prompting-}" ] || [ -n "$__td_start" ] || { __td_now; __td_start=$REPLY; __td_cmd=$BASH_COMMAND; }
  __td_run_debug_trap
}
__td_prompt() {
  local code=$? now secs cmd mode=${TERMDECK_NOTIFY-} threshold=${TERMDECK_NOTIFY_LONG_SECS:-10}
  __td_prompting=1; [ -n "$__td_start" ] || return "$code"; __td_now; now=$REPLY; secs=$((now - __td_start)); __td_start=
  case $threshold in ''|*[!0-9]*) threshold=10;; esac
  case $mode in none) return "$code";; error) [ "$code" -ne 0 ] || return "$code";; long) [ "$secs" -ge "$threshold" ] || return "$code";; all|'') [ "$code" -ne 0 ] || [ "$secs" -ge "$threshold" ] || return "$code";; *) return "$code";; esac
  cmd=${__td_cmd//[$'\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037\177']/}; cmd=${cmd:0:512}
  printf '\033]7777;termdeck;finished;code=%s;secs=%s;cmd=%s\a' "$code" "$secs" "$cmd"
  return "$code"
}
__td_prompt_end() { local code=$?; __td_prompting=; return "$code"; }
__td_ready() { local code=$?; printf '\033]7777;termdeck;ready\a'; return "$code"; }
trap '__td_preexec' DEBUG
case "$(declare -p PROMPT_COMMAND 2>/dev/null)" in
  "declare -a "*) PROMPT_COMMAND=(__td_ready __td_prompt "${PROMPT_COMMAND[@]}" __td_prompt_end);;
  *) PROMPT_COMMAND="__td_ready; __td_prompt${PROMPT_COMMAND:+; $PROMPT_COMMAND}; __td_prompt_end";;
esac
"#;

const ZSH_RC: &str = r#"[ -r "${TERMDECK_USER_ZDOTDIR:-$HOME}/.zshrc" ] && . "${TERMDECK_USER_ZDOTDIR:-$HOME}/.zshrc"
[[ -o interactive ]] || return
zmodload zsh/datetime
if [[ -n ${TERMDECK_SHELL_HOOK-} || -z ${TERMDECK_SOCK-} || -z ${TERMDECK_PANE-} ]]; then return; fi
export TERMDECK_SHELL_HOOK=1
typeset -g __td_start= __td_cmd=
__td_preexec() { __td_start=$EPOCHSECONDS; __td_cmd=$1; }
__td_precmd() {
  local code=$? now=$EPOCHSECONDS secs cmd=$__td_cmd mode=${TERMDECK_NOTIFY-} threshold=${TERMDECK_NOTIFY_LONG_SECS:-10}
  [[ -n $__td_start ]] || return $code; secs=$((now - __td_start)); __td_start=
  case $threshold in ''|*[!0-9]*) threshold=10;; esac
  case $mode in none) return $code;; error) (( code )) || return $code;; long) (( secs >= threshold )) || return $code;; all|'') (( code || secs >= threshold )) || return $code;; *) return $code;; esac
  cmd=${cmd//[$'\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037\177']/}; cmd=${cmd[1,512]}
  printf '\033]7777;termdeck;finished;code=%s;secs=%s;cmd=%s\a' "$code" "$secs" "$cmd"
  return $code
}
__td_ready() { printf '\033]7777;termdeck;ready\a'; }
typeset -ga preexec_functions precmd_functions
preexec_functions+=(__td_preexec)
precmd_functions+=(__td_precmd)
if (( $+widgets[zle-line-init] )); then
  zle -A zle-line-init __td_user_zle_line_init
fi
__td_zle_line_init() {
  if (( $+widgets[__td_user_zle_line_init] )); then
    zle __td_user_zle_line_init "$@"
  fi
  __td_ready
}
zle -N zle-line-init __td_zle_line_init
"#;

/// Ubuntu's system zshrc honours this when it is set in `$ZDOTDIR/.zshenv`
/// (which runs before the global file): skip its own `compinit`, whose
/// interactive insecure-directory prompt would otherwise wait on stdin
/// ahead of the first prompt while the readiness gate holds our input.
const ZSH_ENV: &str = r#"skip_global_compinit=1
"#;

const FISH_RC: &str = r#"if status is-interactive; and not set -q TERMDECK_SHELL_HOOK; and set -q TERMDECK_SOCK; and set -q TERMDECK_PANE
  set -gx TERMDECK_SHELL_HOOK 1
  set -g __td_cmd
  function __td_preexec --on-event fish_preexec
    set -g __td_cmd $argv[1]
  end
  function __td_postexec --on-event fish_postexec
    set -l code $status; set -q __td_cmd CMD_DURATION; or return $code
    set -l secs (math -s0 "$CMD_DURATION / 1000"); set -l mode $TERMDECK_NOTIFY; set -l threshold $TERMDECK_NOTIFY_LONG_SECS
    string match -rq '^[0-9]+$' -- "$threshold"; or set threshold 10
    switch $mode
      case none; return $code
      case error; test $code -ne 0; or return $code
      case long; test $secs -ge $threshold; or return $code
      case all ''; test $code -ne 0; or test $secs -ge $threshold; or return $code
      case '*'; return $code
    end
    set -l cmd (string replace -ra '[\\x00-\\x1f\\x7f]' '' -- "$__td_cmd")
    printf '\\e]7777;termdeck;finished;code=%s;secs=%s;cmd=%s\\a' "$code" "$secs" (string sub -l 512 -- "$cmd")
    return $code
  end
  function __td_ready --on-event fish_prompt
    printf '\\e]7777;termdeck;ready\\a'
    functions -e __td_ready
  end
end
"#;

const READY_MARKER: &[u8] = b"\x1b]7777;termdeck;ready\x07";

/// A short-lived generated startup directory. Its lifetime is the owned PTY.
pub(super) struct ShellHook {
    dir: PathBuf,
}

impl ShellHook {
    pub(super) fn install(
        program: &str,
        arguments: &[String],
        command: &mut CommandBuilder,
    ) -> Result<Option<Self>, String> {
        let Some(name) = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
        else {
            return Ok(None);
        };
        if name != "bash" && name != "zsh" && name != "fish" {
            return Ok(None);
        }
        let dir = unique_dir()?;
        if name == "bash" {
            let rc = dir.join("bashrc");
            let login = bash_is_login(arguments);
            if login {
                fs::write(dir.join(".bash_profile"), bash_login_profile())
                    .map_err(|error| error.to_string())?;
                if let Some(home) = command
                    .get_env("HOME")
                    .map(ToOwned::to_owned)
                    .or_else(|| env::var_os("HOME"))
                {
                    command.env("TERMDECK_HOOK_ORIGINAL_HOME", home);
                } else {
                    command.env_remove("TERMDECK_HOOK_ORIGINAL_HOME");
                }
                command.env("HOME", &dir);
                command.args(arguments);
            } else {
                fs::write(&rc, bash_rc()).map_err(|error| error.to_string())?;
                command.arg("--rcfile");
                command.arg(rc);
                command.args(arguments);
            }
        } else if name == "zsh" {
            fs::write(dir.join(".zshenv"), ZSH_ENV).map_err(|error| error.to_string())?;
            fs::write(dir.join(".zshrc"), ZSH_RC).map_err(|error| error.to_string())?;
            if let Some(user_zdotdir) = env::var_os("ZDOTDIR") {
                command.env("TERMDECK_USER_ZDOTDIR", user_zdotdir);
            }
            command.env("ZDOTDIR", &dir);
            command.args(arguments);
        } else {
            let hook_dir = dir.join("fish/vendor_conf.d");
            fs::create_dir_all(&hook_dir).map_err(|error| error.to_string())?;
            fs::write(hook_dir.join("termdeck.fish"), FISH_RC)
                .map_err(|error| error.to_string())?;
            let mut data_dirs = env::var_os("XDG_DATA_DIRS")
                .map(|dirs| env::split_paths(&dirs).collect())
                .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
            data_dirs.insert(0, dir.clone());
            let data_dirs = env::join_paths(data_dirs).map_err(|error| error.to_string())?;
            command.env("XDG_DATA_DIRS", data_dirs);
            command.args(arguments);
        }
        Ok(Some(Self { dir }))
    }

    pub(super) fn ready_marker(&self) -> &'static [u8] {
        READY_MARKER
    }
}

/// Whether Bash will treat these argv options as a login-shell invocation.
fn bash_is_login(arguments: &[String]) -> bool {
    let mut options = true;
    for argument in arguments {
        if options && argument == "--" {
            options = false;
        } else if options && argument == "--login" {
            return true;
        } else if options && !argument.starts_with("--") {
            if let Some(flags) = argument.strip_prefix('-') {
                if flags.contains('l') {
                    return true;
                }
                // The string after `-c` is a command, not another option.
                options = !flags.contains('c');
            } else {
                options = false;
            }
        }
    }
    false
}

fn bash_rc() -> String {
    format!("{BASH_NON_LOGIN_RC}{BASH_HOOK}")
}

fn bash_login_profile() -> String {
    format!("{BASH_LOGIN_PROFILE}{BASH_HOOK}")
}

impl Drop for ShellHook {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn unique_dir() -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let dir = env::temp_dir().join(format!("termdeck-shell-{}-{nonce}", std::process::id()));
    fs::create_dir(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::Path,
        process::{Command, Stdio},
    };

    use portable_pty::CommandBuilder;

    use super::{FISH_RC, ShellHook, ZSH_ENV, ZSH_RC, bash_is_login, bash_rc, unique_dir};

    fn run_login_bash(home: &Path, input: &[u8]) -> String {
        let mut command = CommandBuilder::new("bash");
        command.env("HOME", home);
        let hook = ShellHook::install("bash", &["-l".to_owned()], &mut command)
            .unwrap()
            .unwrap();
        assert_eq!(command.get_env("HOME"), Some(hook.dir.as_os_str()));
        assert_eq!(
            command.get_env("TERMDECK_HOOK_ORIGINAL_HOME"),
            Some(home.as_os_str())
        );
        let mut child = Command::new("bash")
            .args(["-i", "-l"])
            .env("HOME", &hook.dir)
            .env("TERMDECK_HOOK_ORIGINAL_HOME", home)
            .env("TERMDECK_SOCK", "test")
            .env("TERMDECK_PANE", "test")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    /// #136 r4: Ubuntu's system zshrc runs an interactive `compinit`
    /// (insecure-directory prompt) ahead of the first prompt while the
    /// readiness gate holds our input — a deadlock. The hook opts out via
    /// the flag that file documents, set in `$ZDOTDIR/.zshenv` which runs
    /// before the global file. This pins the opt-out and the recursion-free
    /// widget form (the old `zle -A` + same-name function redefinition
    /// could not invoke the saved widget and dropped the user's own).
    #[test]
    fn zsh_hook_skips_global_compinit_and_preserves_widgets_by_alias() {
        let mut command = CommandBuilder::new("zsh");
        let hook = ShellHook::install("zsh", &[], &mut command)
            .unwrap()
            .unwrap();
        let zshenv = fs::read_to_string(hook.dir.join(".zshenv")).unwrap();
        assert_eq!(zshenv, ZSH_ENV);
        assert!(
            zshenv.contains("skip_global_compinit=1"),
            "the Ubuntu global-compinit opt-out must be set: {zshenv:?}"
        );
        assert!(
            ZSH_RC.contains("zle -N zle-line-init __td_zle_line_init"),
            "the widget must keep its own function name so the saved alias stays callable"
        );
        assert!(
            !ZSH_RC.contains("zle-line-init()"),
            "redefining the same-name function shadows what `zle -A` saved"
        );
    }

    #[test]
    fn bash_login_detection_preserves_argv_and_stops_after_c() {
        assert!(bash_is_login(&["-ilc".to_owned(), "echo hook".to_owned()]));
        assert!(bash_is_login(&[
            "--login".to_owned(),
            "--".to_owned(),
            "-l".to_owned()
        ]));
        assert!(!bash_is_login(&["-c".to_owned(), "echo -l".to_owned()]));
        assert!(!bash_is_login(&["-i".to_owned()]));
    }

    #[test]
    fn fish_hook_uses_a_small_vendor_data_shim() {
        assert!(FISH_RC.lines().count() <= 30);
        let mut command = CommandBuilder::new("fish");
        let hook = ShellHook::install("fish", &[], &mut command)
            .unwrap()
            .unwrap();
        assert!(hook.dir.join("fish/vendor_conf.d/termdeck.fish").is_file());
        assert!(command.get_env("XDG_DATA_DIRS").is_some_and(|paths| {
            paths
                .to_string_lossy()
                .starts_with(&*hook.dir.to_string_lossy())
        }));
    }

    #[test]
    fn bash_snippet_emits_error_and_long_rules_without_control_bytes_in_cmd() {
        let dir = unique_dir().unwrap();
        let rc = dir.join("rc");
        fs::write(&rc, bash_rc()).unwrap();
        let output = Command::new("bash")
            .args([
                "--noprofile",
                "--norc",
                "-i",
                "-c",
                "source \"$1\"; __td_cmd=$'bad\\033title'; __td_start=$EPOCHSECONDS; false; __td_prompt; __td_cmd=slow; __td_start=$((EPOCHSECONDS - 10)); true; __td_prompt",
                "bash",
                rc.to_str().unwrap(),
            ])
            .env("HOME", &dir)
            .env("TERMDECK_SOCK", "test")
            .env("TERMDECK_PANE", "test")
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("\x1b]7777;termdeck;finished;code=1;"),
            "{stdout:?}"
        );
        assert!(stdout.contains("cmd=badtitle\x07"), "{stdout:?}");
        assert!(stdout.contains("code=0;secs=10;cmd=slow\x07"), "{stdout:?}");
    }

    /// #147: `__td_prompt` is prepended to PROMPT_COMMAND, so it runs BEFORE
    /// every element the user already had. It captures `$?` on its first line
    /// and must hand that same status back on every return path — otherwise
    /// each later element sees the status of whatever ran last inside the hook
    /// (the `case`, or the `__td_start=` assignment), i.e. success after a
    /// failed command. `TERMDECK_NOTIFY=none` was no escape: its early return
    /// happens after `local code=$?` has already consumed the status.
    fn user_prompt_statuses(prompt_command: &str, notify: Option<&str>) -> Vec<String> {
        let home = unique_dir().unwrap();
        fs::write(
            home.join(".bashrc"),
            format!(
                "__td_user_prompt() {{ printf 'USER_STATUS=%s\\n' \"$?\"; }}\n{prompt_command}\n"
            ),
        )
        .unwrap();
        let rc = home.join("rc");
        // The same rc the engine writes for a non-login bash pane.
        fs::write(&rc, bash_rc()).unwrap();
        let mut command = Command::new("bash");
        command
            .args(["--rcfile", rc.to_str().unwrap(), "-i"])
            .env("HOME", &home)
            .env("TERMDECK_SOCK", "test")
            .env("TERMDECK_PANE", "test")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match notify {
            Some(mode) => command.env("TERMDECK_NOTIFY", mode),
            None => command.env_remove("TERMDECK_NOTIFY"),
        };
        let mut child = command.spawn().unwrap();
        // `false` fails with 1; `(exit 42)` fails with 42 from a subshell, so
        // the DEBUG trap records no start and the hook takes its earliest
        // return — the path that must still report 42.
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"false\n(exit 42)\nexit 0\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let statuses = stdout
            .split("USER_STATUS=")
            .skip(1)
            .map(|rest| {
                rest.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        fs::remove_dir_all(home).unwrap();
        statuses
    }

    #[test]
    fn bash_prompt_hook_hands_the_real_exit_status_to_later_prompt_commands() {
        // Scalar and array PROMPT_COMMAND: the hook prepends itself to both.
        for prompt_command in [
            "PROMPT_COMMAND=\"__td_user_prompt\"",
            "PROMPT_COMMAND=(__td_user_prompt)",
        ] {
            // Every notification mode, `none` included: opting out of
            // notifications must not cost the user their exit status.
            for notify in [None, Some("none"), Some("error"), Some("all")] {
                let statuses = user_prompt_statuses(prompt_command, notify);
                assert_eq!(
                    statuses,
                    ["0", "1", "42"],
                    "{prompt_command} with TERMDECK_NOTIFY={notify:?}"
                );
            }
        }
    }

    #[test]
    fn bash_login_hook_preserves_profiles_and_array_prompt_commands() {
        let mut command = CommandBuilder::new("bash");
        let home = unique_dir().unwrap();
        command.env("HOME", &home);
        let hook = ShellHook::install("bash", &["-l".to_owned()], &mut command)
            .unwrap()
            .unwrap();
        fs::write(
            home.join("profile.d"),
            "PROFILE_D_COUNT=$((PROFILE_D_COUNT + 1))\nunset PROMPT_COMMAND\ndeclare -a PROMPT_COMMAND=()\n__systemd_osc_context_precmdline() { printf PROFILE_PROMPT\\n; }\nPROMPT_COMMAND+=(__systemd_osc_context_precmdline)\n",
        )
        .unwrap();
        fs::write(
            home.join(".bashrc"),
            "printf 'BASHRC-MARKER\\n'\nif ! shopt -q login_shell; then . \"$HOME/profile.d\"; fi\n",
        )
        .unwrap();
        fs::write(
            home.join(".bash_profile"),
            "printf 'PROFILE-MARKER\\n'\nPROFILE_D_COUNT=0\n. \"$HOME/profile.d\"\n. \"$HOME/.bashrc\"\ntrap 'printf DEBUG=kept\\n' DEBUG\n",
        )
        .unwrap();
        fs::write(home.join(".bash_logout"), "printf LOGOUT=kept\\n").unwrap();
        let argv = command
            .get_argv()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(argv[0], "bash");
        assert_eq!(argv, ["bash", "-l"]);
        assert_eq!(command.get_env("HOME"), Some(hook.dir.as_os_str()));
        assert_eq!(
            command.get_env("TERMDECK_HOOK_ORIGINAL_HOME"),
            Some(home.as_os_str())
        );
        let mut child = Command::new("bash")
            // The real production argv is `bash -l`; `-i` only makes this
            // pipe-backed regression test interactive like a PTY.
            .args(["-i", "-l"])
            .env("HOME", &hook.dir)
            .env("TERMDECK_HOOK_ORIGINAL_HOME", &home)
            .env("TERMDECK_SOCK", "test")
            .env("TERMDECK_PANE", "test")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(b"printf 'HOOK=%s PROFILE_D=%s HOME=%s LOGIN=%s\\n' \"$TERMDECK_SHELL_HOOK\" \"$PROFILE_D_COUNT\" \"$HOME\" \"$(shopt -q login_shell && echo yes)\"\nfalse\nTERMDECK_NOTIFY_LONG_SECS=0\nsleep 0.01\nexit\n").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("HOOK=1 PROFILE_D=1"), "{stdout:?}");
        assert!(stdout.contains(&format!("HOME={} LOGIN=yes", home.display())));
        assert_eq!(stdout.matches("PROFILE-MARKER").count(), 1, "{stdout:?}");
        assert_eq!(stdout.matches("BASHRC-MARKER").count(), 1, "{stdout:?}");
        assert!(
            stdout.find("PROFILE-MARKER") < stdout.find("BASHRC-MARKER"),
            "{stdout:?}"
        );
        assert!(stdout.contains("DEBUG=kept"), "{stdout:?}");
        assert!(stdout.contains("PROFILE_PROMPT"), "{stdout:?}");
        assert!(stdout.contains("cmd=false\x07"), "{stdout:?}");
        assert!(stdout.contains("cmd=sleep 0.01\x07"), "{stdout:?}");
        assert!(!stdout.contains("cmd=__systemd_osc_context_"), "{stdout:?}");
        assert!(stdout.contains("LOGOUT=kept"), "{stdout:?}");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn bash_login_profile_uses_normal_user_file_fallback_order() {
        let home = unique_dir().unwrap();
        fs::write(home.join(".bash_profile"), "printf 'TD-BASH-PROFILE\\n'").unwrap();
        fs::write(home.join(".bash_login"), "printf 'TD-BASH-LOGIN\\n'").unwrap();
        fs::write(home.join(".profile"), "printf 'TD-DOT-PROFILE\\n'").unwrap();

        let stdout = run_login_bash(&home, b"exit\n");
        assert_eq!(stdout.matches("TD-BASH-PROFILE").count(), 1, "{stdout:?}");
        assert!(!stdout.contains("TD-BASH-LOGIN"), "{stdout:?}");
        assert!(!stdout.contains("TD-DOT-PROFILE"), "{stdout:?}");

        fs::remove_file(home.join(".bash_profile")).unwrap();
        let stdout = run_login_bash(&home, b"exit\n");
        assert_eq!(stdout.matches("TD-BASH-LOGIN").count(), 1, "{stdout:?}");
        assert!(!stdout.contains("TD-DOT-PROFILE"), "{stdout:?}");

        fs::remove_file(home.join(".bash_login")).unwrap();
        let stdout = run_login_bash(&home, b"exit\n");
        assert_eq!(stdout.matches("TD-DOT-PROFILE").count(), 1, "{stdout:?}");
        fs::remove_dir_all(home).unwrap();
    }
}
