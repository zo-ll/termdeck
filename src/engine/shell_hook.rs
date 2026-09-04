use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use portable_pty::CommandBuilder;

const BASH_RC: &str = r#"[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
case $- in *i*) ;; *) return;; esac
if [ -n "${TERMDECK_SHELL_HOOK-}" ] || [ -z "${TERMDECK_SOCK-}" ] || [ -z "${TERMDECK_PANE-}" ]; then return; fi
export TERMDECK_SHELL_HOOK=1
__td_start= __td_cmd=
__td_now() { if [ -n "${EPOCHSECONDS+x}" ]; then REPLY=$EPOCHSECONDS; else REPLY=$SECONDS; fi; }
__td_preexec() { case $BASH_COMMAND in __td_*) return;; esac; [ -n "${__td_prompting-}" ] || [ -n "$__td_start" ] || { __td_now; __td_start=$REPLY; __td_cmd=$BASH_COMMAND; }; }
__td_prompt() {
  local code=$? now secs cmd mode=${TERMDECK_NOTIFY-} threshold=${TERMDECK_NOTIFY_LONG_SECS:-10}
  __td_prompting=1; [ -n "$__td_start" ] || return; __td_now; now=$REPLY; secs=$((now - __td_start)); __td_start=
  case $threshold in ''|*[!0-9]*) threshold=10;; esac
  case $mode in none) return;; error) [ "$code" -ne 0 ] || return;; long) [ "$secs" -ge "$threshold" ] || return;; all|'') [ "$code" -ne 0 ] || [ "$secs" -ge "$threshold" ] || return;; *) return;; esac
  cmd=${__td_cmd//[$'\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037\177']/}; cmd=${cmd:0:512}
  printf '\033]7777;termdeck;finished;code=%s;secs=%s;cmd=%s\a' "$code" "$secs" "$cmd"
}
__td_prompt_end() { __td_prompting=; }
trap '__td_preexec' DEBUG
PROMPT_COMMAND="__td_prompt${PROMPT_COMMAND:+; $PROMPT_COMMAND}; __td_prompt_end"
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
autoload -Uz add-zsh-hook
add-zsh-hook preexec __td_preexec
add-zsh-hook precmd __td_precmd
"#;

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
        if name != "bash" && name != "zsh" {
            return Ok(None);
        }
        let dir = unique_dir()?;
        if name == "bash" {
            let rc = dir.join("bashrc");
            fs::write(&rc, BASH_RC).map_err(|error| error.to_string())?;
            command.arg("--rcfile");
            command.arg(rc);
            command.args(arguments);
        } else {
            fs::write(dir.join(".zshrc"), ZSH_RC).map_err(|error| error.to_string())?;
            if let Some(user_zdotdir) = env::var_os("ZDOTDIR") {
                command.env("TERMDECK_USER_ZDOTDIR", user_zdotdir);
            }
            command.env("ZDOTDIR", &dir);
            command.args(arguments);
        }
        Ok(Some(Self { dir }))
    }
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
    use std::{fs, process::Command};

    use super::{BASH_RC, unique_dir};

    #[test]
    fn bash_snippet_emits_error_and_long_rules_without_control_bytes_in_cmd() {
        let dir = unique_dir().unwrap();
        let rc = dir.join("rc");
        fs::write(&rc, BASH_RC).unwrap();
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
}
