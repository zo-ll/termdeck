use super::*;

pub(super) struct OuterTerminal;

impl OuterTerminal {
    pub(super) fn enter() -> io::Result<Self> {
        let mut previous = std::mem::MaybeUninit::<libc::termios>::zeroed();
        // SAFETY: stdin is a valid descriptor and `previous` is writable.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, previous.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded above.
        let previous = unsafe { previous.assume_init() };
        let mut raw = previous;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        // SAFETY: stdin is a valid descriptor and `raw` is a valid termios value.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        *saved_termios()
            .lock()
            .expect("terminal state lock poisoned") = Some(previous);
        let mut stdout = io::stdout();
        if let Err(error) =
            stdout.write_all(b"\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h")
        {
            restore_outer_terminal();
            return Err(error);
        }
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for OuterTerminal {
    fn drop(&mut self) {
        restore_outer_terminal();
    }
}

fn saved_termios() -> &'static Mutex<Option<libc::termios>> {
    SAVED_TERMIOS.get_or_init(|| Mutex::new(None))
}

fn saved_panic_hook() -> &'static Mutex<Option<PanicHook>> {
    SAVED_PANIC_HOOK.get_or_init(|| Mutex::new(None))
}

fn restore_outer_terminal() {
    let _ =
        io::stdout().write_all(b"\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?25h\x1b[?1049l");
    let _ = io::stdout().flush();
    if let Some(previous) = saved_termios()
        .lock()
        .ok()
        .and_then(|mut saved| saved.take())
    {
        // SAFETY: `previous` came from tcgetattr for this process's stdin.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &previous) };
    }
}

pub(super) struct PanicGuard;

impl PanicGuard {
    pub(super) fn install() -> Self {
        let previous = panic::take_hook();
        *saved_panic_hook().lock().expect("panic hook lock poisoned") = Some(previous);
        panic::set_hook(Box::new(|info| {
            restore_outer_terminal();
            if let Ok(hooks) = saved_panic_hook().lock()
                && let Some(previous) = hooks.as_ref()
            {
                previous(info);
            }
        }));
        Self
    }
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        let _ = panic::take_hook();
        if let Some(previous) = saved_panic_hook()
            .lock()
            .ok()
            .and_then(|mut hook| hook.take())
        {
            panic::set_hook(previous);
        }
    }
}

extern "C" fn caught_signal(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::SeqCst);
}

pub(super) struct SignalGuard {
    #[cfg(unix)]
    previous_int: libc::sigaction,
    #[cfg(unix)]
    previous_term: libc::sigaction,
}

impl SignalGuard {
    pub(super) fn install() -> io::Result<Self> {
        // SAFETY: the handler only performs an atomic store, which is signal-safe.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = caught_signal as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous_int = std::mem::zeroed();
            let mut previous_term = std::mem::zeroed();
            if libc::sigaction(libc::SIGINT, &action, &mut previous_int) != 0
                || libc::sigaction(libc::SIGTERM, &action, &mut previous_term) != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                previous_int,
                previous_term,
            })
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        // SAFETY: these are the handlers captured by `install`.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous_int, std::ptr::null_mut());
            libc::sigaction(libc::SIGTERM, &self.previous_term, std::ptr::null_mut());
        }
    }
}
