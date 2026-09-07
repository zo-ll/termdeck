use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use portable_pty::CommandBuilder;

const BASH_NON_LOGIN_RC: &str = r#"[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
"#;

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
  __td_prompting=1; [ -n "$__td_start" ] || return; __td_now; now=$REPLY; secs=$((now - __td_start)); __td_start=
  case $threshold in ''|*[!0-9]*) threshold=10;; esac
  case $mode in none) return;; error) [ "$code" -ne 0 ] || return;; long) [ "$secs" -ge "$threshold" ] || return;; all|'') [ "$code" -ne 0 ] || [ "$secs" -ge "$threshold" ] || return;; *) return;; esac
  cmd=${__td_cmd//[$'\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037\177']/}; cmd=${cmd:0:512}
  printf '\033]7777;termdeck;finished;code=%s;secs=%s;cmd=%s\a' "$code" "$secs" "$cmd"
}
__td_prompt_end() { __td_prompting=; }
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
  [[ -n $__td_start ]] || return; secs=$((now - __td_start)); __td_start=
  case $threshold in ''|*[!0-9]*) threshold=10;; esac
  case $mode in none) return;; error) (( code )) || return;; long) (( secs >= threshold )) || return;; all|'') (( code || secs >= threshold )) || return;; *) return;; esac
  cmd=${cmd//[$'\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037\177']/}; cmd=${cmd[1,512]}
  printf '\033]7777;termdeck;finished;code=%s;secs=%s;cmd=%s\a' "$code" "$secs" "$cmd"
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
    set -l code $status; set -q __td_cmd CMD_DURATION; or return
    set -l secs (math -s0 "$CMD_DURATION / 1000"); set -l mode $TERMDECK_NOTIFY; set -l threshold $TERMDECK_NOTIFY_LONG_SECS
    string match -rq '^[0-9]+$' -- "$threshold"; or set threshold 10
    switch $mode
      case none; return
      case error; test $code -ne 0; or return
      case long; test $secs -ge $threshold; or return
      case all ''; test $code -ne 0; or test $secs -ge $threshold; or return
      case '*'; return
    end
    set -l cmd (string replace -ra '[\\x00-\\x1f\\x7f]' '' -- "$__td_cmd")
    printf '\\e]7777;termdeck;finished;code=%s;secs=%s;cmd=%s\\a' "$code" "$secs" (string sub -l 512 -- "$cmd")
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
    bootstrap: Option<Vec<u8>>,
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
        let mut bootstrap = None;
        if name == "bash" {
            let rc = dir.join("bashrc");
            let login = bash_is_login(arguments);
            let contents = if login { BASH_HOOK } else { &bash_rc() };
            fs::write(&rc, contents).map_err(|error| error.to_string())?;
            if login {
                // `--rcfile` is ignored by a login shell. Keep Bash's real
                // `-l` startup (including `login_shell` and profile guards),
                // then feed this source command to its PTY for the first
                // prompt. Terminal input is buffered until startup is done.
                command.args(arguments);
                bootstrap = Some(source_command(&rc));
            } else {
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
        Ok(Some(Self { dir, bootstrap }))
    }

    pub(super) fn bootstrap(&self) -> Option<&[u8]> {
        self.bootstrap.as_deref()
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

fn source_command(path: &Path) -> Vec<u8> {
    let quoted = path.to_string_lossy().replace('\'', "'\\''");
    // `stty echo` first: echo is off only for this one injected line, and
    // restoring it before the source keeps a failing `.` from leaving the
    // pane unable to show what the user types (#149).
    format!("stty echo; . '{quoted}'\n").into_bytes()
}

/// Turns the PTY's terminal echo off, for the login-bash bootstrap only
/// (#149). That bootstrap is fed to the shell as terminal input, so both
/// the line discipline and readline would otherwise print the source line
/// at the first prompt. The bootstrap turns echo back on itself, so this
/// must be called before the shell is spawned and never for a shell that
/// has no bootstrap to hide.
#[cfg(unix)]
pub(super) fn silence_bootstrap_echo(fd: std::os::unix::io::RawFd) -> Result<(), String> {
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `tcgetattr` on an owned PTY fd, writing one `termios`.
    if unsafe { libc::tcgetattr(fd, attributes.as_mut_ptr()) } == -1 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: `tcgetattr` returned success, so the value is initialised.
    let mut attributes = unsafe { attributes.assume_init() };
    attributes.c_lflag &= !libc::ECHO;
    // SAFETY: `tcsetattr` on the same fd with the value read just above.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &attributes) } == -1 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
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
        io::{Read, Write},
        process::{Command, Stdio},
    };

    use portable_pty::{CommandBuilder, PtySize, native_pty_system};

    #[cfg(unix)]
    use super::silence_bootstrap_echo;
    use super::{FISH_RC, ShellHook, ZSH_ENV, ZSH_RC, bash_is_login, bash_rc, unique_dir};

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

    #[test]
    fn bash_login_hook_preserves_profiles_and_array_prompt_commands() {
        let mut command = CommandBuilder::new("bash");
        let hook = ShellHook::install("bash", &["-l".to_owned()], &mut command)
            .unwrap()
            .unwrap();
        fs::write(
            hook.dir.join("profile.d"),
            "PROFILE_D_COUNT=$((PROFILE_D_COUNT + 1))\nunset PROMPT_COMMAND\ndeclare -a PROMPT_COMMAND=()\n__systemd_osc_context_precmdline() { printf PROFILE_PROMPT\\n; }\nPROMPT_COMMAND+=(__systemd_osc_context_precmdline)\n",
        )
        .unwrap();
        fs::write(
            hook.dir.join(".bashrc"),
            "if ! shopt -q login_shell; then . \"$HOME/profile.d\"; fi\n",
        )
        .unwrap();
        fs::write(
            hook.dir.join(".bash_profile"),
            "PROFILE_D_COUNT=0\n. \"$HOME/profile.d\"\n. \"$HOME/.bashrc\"\ntrap 'printf DEBUG=kept\\n' DEBUG\n",
        )
        .unwrap();
        fs::write(hook.dir.join(".bash_logout"), "printf LOGOUT=kept\\n").unwrap();
        let argv = command
            .get_argv()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(argv[0], "bash");
        assert_eq!(argv, ["bash", "-l"]);
        let mut child = Command::new("bash")
            // The real production argv is `bash -l`; `-i` only makes this
            // pipe-backed regression test interactive like a PTY.
            .args(["-i", "-l"])
            .env("HOME", &hook.dir)
            .env("TERMDECK_SOCK", "test")
            .env("TERMDECK_PANE", "test")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(hook.bootstrap().unwrap()).unwrap();
        stdin.write_all(b"printf 'HOOK=%s PROFILE_D=%s\\n' \"$TERMDECK_SHELL_HOOK\" \"$PROFILE_D_COUNT\"\nfalse\nTERMDECK_NOTIFY_LONG_SECS=0\nsleep 0.01\nexit\n").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("HOOK=1 PROFILE_D=1"), "{stdout:?}");
        assert!(stdout.contains("DEBUG=kept"), "{stdout:?}");
        assert!(stdout.contains("PROFILE_PROMPT"), "{stdout:?}");
        assert!(stdout.contains("cmd=false\x07"), "{stdout:?}");
        assert!(stdout.contains("cmd=sleep 0.01\x07"), "{stdout:?}");
        assert!(!stdout.contains("cmd=__systemd_osc_context_"), "{stdout:?}");
        assert!(stdout.contains("LOGOUT=kept"), "{stdout:?}");
    }

    /// #149: the login bootstrap reaches the shell as terminal input, so it
    /// must not be echoed back at the first prompt — and echo has to be on
    /// again for everything the user types afterwards.
    #[cfg(unix)]
    #[test]
    fn bash_login_bootstrap_is_not_echoed_at_the_first_prompt() {
        let mut command = CommandBuilder::new("bash");
        // `--noprofile` keeps this to the bootstrap alone: the login profile
        // fidelity is the neighbouring test's subject, and running the real
        // one here would only add a system's worth of startup to the suite.
        let arguments = ["--noprofile".to_owned(), "-l".to_owned()];
        let hook = ShellHook::install("bash", &arguments, &mut command)
            .unwrap()
            .unwrap();
        command.env("HOME", &hook.dir);
        command.env("TERMDECK_SOCK", "test");
        command.env("TERMDECK_PANE", "test");
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();
        silence_bootstrap_echo(pair.master.as_raw_fd().unwrap()).unwrap();
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        writer.write_all(hook.bootstrap().unwrap()).unwrap();
        writer
            .write_all(b"printf 'HOOK=%s\\n' \"$TERMDECK_SHELL_HOOK\"\nexit\n")
            .unwrap();
        let collector = std::thread::spawn(move || {
            let mut output = Vec::new();
            // The master reports the closed slave as an error, not as EOF.
            let _ = reader.read_to_end(&mut output);
            output
        });
        child.wait().unwrap();
        let output = String::from_utf8_lossy(&collector.join().unwrap()).into_owned();
        assert!(!output.contains("termdeck-shell"), "{output:?}");
        assert!(output.contains("HOOK=1"), "{output:?}");
        assert!(output.contains("printf 'HOOK="), "{output:?}");
    }
}
