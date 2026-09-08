# Login Bash early-profile shim (#152)

## Decision

For a login Bash pane, Termdeck now creates two *conditional* files in the
generated HOME used to select its `.bash_profile`:

- If the real `$HOME/.hushlogin` exists, create an empty generated
  `.hushlogin`.
- If the real `$HOME/.bash_completion` is a regular file, create a generated
  `.bash_completion` which temporarily sets `HOME` to
  `TERMDECK_HOOK_ORIGINAL_HOME`, sources the real file, then restores the
  generated HOME.

The generated `.bash_profile` remains the #151 profile shim: it restores the
real HOME, runs the normal first-readable user login profile, then installs
the hook before the first prompt. No command is sent through the PTY.

## Why this mechanism

Bash documents that a login invocation reads `/etc/profile` before searching
for `.bash_profile`, `.bash_login`, and `.profile` (in that order). That makes
the existing scoped HOME unavoidable until the generated `.bash_profile`
runs; Bash has no login equivalent of `--rcfile` or zsh's `ZDOTDIR`.

Ubuntu's global Bash setup uses `$HOME/.hushlogin` to suppress its sudo hint.
The empty generated file therefore follows exactly the same existence branch
as the user's real file without copying its contents. The current
`bash-completion` implementation loads its user file late and defaults to
`~/.bash_completion`; the generated forwarding file is found at that normal
point, but runs the user's completion code with the real HOME. That matters
because completion snippets commonly resolve further user files from HOME.

This was preferred to a symlink or simply setting `BASH_COMPLETION_USER_FILE`:
both let the user completion run while HOME is still scoped, and the latter is
specific to bash-completion versions which implement that variable. It was
also preferred to sourcing the user completion after the profile shim, which
would change the global-before-user-profile ordering. The hush-login sentinel
matches Ubuntu's existence check; completion is mirrored only for a regular
real file, matching its `-f` check. When either user file is absent, no
generated counterpart exists.

## Verification and limits

- GNU Bash's startup-file manual confirms the global-before-user order:
  <https://www.gnu.org/software/bash/manual/html_node/Bash-Startup-Files>.
- The installed host is Fedora-like: `/etc/profile` calls `/etc/bashrc` and
  has no Ubuntu sudo hint. Its installed bash-completion source was inspected
  at `/usr/share/bash-completion/bash_completion`; it defaults its user file to
  `~/.bash_completion` and sources it after global completion setup.
- Ubuntu's sudo-hint condition is documented as checking that
  `$HOME/.hushlogin` does not exist:
  <https://askubuntu.com/questions/917299/whats-complaining-about-sudo-when-i-open-a-terminal>.
  The upstream bash-completion configuration documents
  `BASH_COMPLETION_USER_FILE` and its default:
  <https://github.com/scop/bash-completion/blob/main/doc/configuration.md>.
- `bash_login_early_shims_honor_hushlogin_and_user_completion` drives a real
  login Bash through the deterministic Ubuntu-shaped global-profile order. It
  fails without the generated files by printing `SUDO-HINT` and not loading
  the user completion. The existing PTY integration regression
  `login_bash_inherits_real_home_and_paints_one_first_prompt` proves the same
  production startup path still has exactly one first prompt and no fed
  bootstrap command.

The local host cannot directly execute Ubuntu's `/etc/profile` because it is
not an Ubuntu installation. The test fixture deliberately models only the two
observed HOME lookups, while the production test continues to exercise Bash,
the generated profile, and a real PTY.
