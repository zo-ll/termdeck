//! Human-friendly ctl.v1 client.  It is intentionally a thin one-call socket
//! wrapper, so agents and shell users exercise exactly the session protocol.

use std::{env, path::PathBuf, process};

use termdeck::{
    ctl::{self, Request, Response, SCHEMA},
    session::input::encode_keys,
};

fn main() {
    process::exit(run(env::args().skip(1).collect()));
}

fn run(arguments: Vec<String>) -> i32 {
    match help_for(&arguments) {
        Ok(Some(help)) => {
            println!("{help}");
            return 0;
        }
        Err(message) => return usage_error(&message),
        Ok(None) => {}
    }
    let (json, socket, request) = match parse(arguments) {
        Ok(value) => value,
        Err(message) => return usage_error(&message),
    };
    match ctl::call(&socket, &request) {
        Ok(response) => print_response(response, json),
        Err(message) => {
            eprintln!("termctl: {message}");
            1
        }
    }
}

fn usage_error(message: &str) -> i32 {
    eprintln!(
        "termctl: {message}\n\n{}\nTry `termctl --help` for more information.",
        usage()
    );
    2
}

fn parse(arguments: Vec<String>) -> Result<(bool, PathBuf, Request), String> {
    let mut json = false;
    let mut socket = None;
    let mut requested_lines = None;
    let mut force = false;
    let mut on = None;
    let mut input = None;
    let mut positional = Vec::new();
    let mut literal = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if !literal && argument == "--" {
            literal = true;
            continue;
        }
        if literal {
            positional.push(argument);
            continue;
        }
        match argument.as_str() {
            "--json" => json = true,
            "--socket" => {
                socket = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--socket requires a path".to_owned())?,
                ));
            }
            "--lines" => {
                if requested_lines.is_some() {
                    return Err("--lines can only be specified once".to_owned());
                }
                requested_lines = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--lines requires a number".to_owned())?
                        .parse::<usize>()
                        .map_err(|_| "peek lines must be a number".to_owned())?,
                );
            }
            "--force" => force = true,
            "--on" => {
                if on.replace(true).is_some() {
                    return Err("zoom mode can only be specified once".to_owned());
                }
            }
            "--off" => {
                if on.replace(false).is_some() {
                    return Err("zoom mode can only be specified once".to_owned());
                }
            }
            "--text" | "--paste" | "--keys" => {
                if input.is_some() {
                    return Err("input kind can only be specified once".to_owned());
                }
                input = Some((
                    argument,
                    arguments
                        .next()
                        .ok_or_else(|| "input kind requires text".to_owned())?,
                ));
            }
            argument if argument.starts_with('-') => {
                return Err(format!("unknown option: {argument}"));
            }
            _ => positional.push(argument),
        }
    }
    let Some(verb) = positional.first() else {
        return Err("missing verb".to_owned());
    };
    let (id, lines, msg, path) = match verb.as_str() {
        "status" | "list" | "version" if positional.len() == 1 => (None, None, None, None),
        "peek" if positional.len() == 2 => {
            (Some(positional[1].clone()), requested_lines, None, None)
        }
        "peek" if positional.len() == 3 && requested_lines.is_none() => (
            Some(positional[1].clone()),
            Some(
                positional[2]
                    .parse::<usize>()
                    .map_err(|_| "peek lines must be a number".to_owned())?,
            ),
            None,
            None,
        ),
        "notify" if positional.len() >= 2 => (None, None, Some(positional[1..].join(" ")), None),
        "open" if positional.len() == 2 => (None, None, None, Some(positional[1].clone())),
        "close" | "promote" | "input" if positional.len() == 2 => {
            (Some(positional[1].clone()), None, None, None)
        }
        "input"
            if positional.len() > 2 && input.as_ref().is_some_and(|(kind, _)| kind == "--keys") =>
        {
            (Some(positional[1].clone()), None, None, None)
        }
        "zoom" if positional.len() == 1 => (None, None, None, None),
        "status" | "list" | "peek" | "notify" | "version" | "open" | "close" | "promote"
        | "zoom" | "input" => return Err(format!("invalid arguments for {verb}")),
        _ => return Err(format!("unknown verb: {verb}")),
    };
    if (force && !matches!(verb.as_str(), "close" | "input"))
        || (on.is_some() && verb != "zoom")
        || (input.is_some() && verb != "input")
    {
        return Err(format!("invalid options for {verb}"));
    }
    if verb == "input" && input.is_none() {
        return Err("input requires exactly one of --text, --paste, or --keys".to_owned());
    }
    let (text, paste, keys) = match input {
        Some((kind, value)) if kind == "--text" => (Some(value), None, None),
        Some((kind, value)) if kind == "--paste" => (None, Some(value), None),
        Some((_, value)) => {
            let mut values = vec![value];
            values.extend(positional.iter().skip(2).cloned());
            let bytes = encode_keys(&values)?;
            let keys = String::from_utf8(bytes)
                .map_err(|_| "encoded keys are not valid UTF-8".to_owned())?;
            (None, None, Some(keys))
        }
        None => (None, None, None),
    };
    Ok((
        json,
        socket
            .map(Ok)
            .unwrap_or_else(ctl::socket_from_environment)?,
        Request {
            schema: SCHEMA.to_owned(),
            verb: verb.clone(),
            id,
            lines,
            msg,
            path,
            force,
            on,
            text,
            paste,
            keys,
        },
    ))
}

fn print_response(response: Response, json: bool) -> i32 {
    if json {
        match serde_json::to_string(&response) {
            Ok(response) => println!("{response}"),
            Err(error) => {
                eprintln!("termctl: {error}");
                return 1;
            }
        }
    } else if response.ok {
        print_plain(response.data.as_ref().unwrap_or(&serde_json::Value::Null));
    } else if let Some(error) = &response.error {
        eprintln!("termctl: {}", error.message);
    }
    response.error.map_or(0, |error| i32::from(error.code))
}

fn print_plain(data: &serde_json::Value) {
    if let Some(lines) = data.get("lines").and_then(serde_json::Value::as_array) {
        for line in lines {
            println!("{}", line.as_str().unwrap_or_default());
        }
    } else if let Some(items) = data.as_array() {
        for item in items {
            println!(
                "{}\t{}\t{}",
                item["id"].as_str().unwrap_or_default(),
                item["state"].as_str().unwrap_or_default(),
                item["path"].as_str().unwrap_or_default(),
            );
        }
    } else {
        println!("{data}");
    }
}

const fn usage() -> &'static str {
    "usage: termctl [--json] [--socket PATH] COMMAND [OPTIONS]"
}

const fn notify_help() -> &'static str {
    "TERMCTL-NOTIFY\n\nUSAGE\n  termctl [--json] [--socket PATH] notify MESSAGE\n\nSends an explicit notification from a Termdeck pane.\n\nOPTIONS\n  --json         Print the ctl.v1 response as JSON.\n  --socket PATH  Connect to PATH instead of TERMDECK_SOCK.\n  -h, --help     Show this help page.\n\nFor automatic shell completion notifications and their environment settings, see README.md#shell-notifications."
}

fn help_for(arguments: &[String]) -> Result<Option<&'static str>, String> {
    if arguments.iter().any(|argument| argument == "--") {
        return Ok(None);
    }
    if arguments == ["help"] {
        return Ok(Some(help()));
    }
    if let [command, topic] = arguments
        && command == "help"
    {
        return command_help(topic)
            .map(Some)
            .ok_or_else(|| format!("unknown verb: {topic}"));
    }
    let Some(last) = arguments.last() else {
        return Ok(None);
    };
    if !matches!(last.as_str(), "--help" | "-h") {
        return Ok(None);
    }
    let mut command = None;
    let mut takes_value = false;
    for argument in &arguments[..arguments.len() - 1] {
        if takes_value {
            takes_value = false;
        } else if matches!(
            argument.as_str(),
            "--socket" | "--lines" | "--text" | "--paste" | "--keys"
        ) {
            takes_value = true;
        } else if !argument.starts_with('-') && command.is_none() {
            command = Some(argument.as_str());
        }
    }
    if takes_value {
        return Ok(None);
    }
    let Some(topic) = command else {
        return Ok(Some(help()));
    };
    command_help(topic)
        .map(Some)
        .ok_or_else(|| format!("unknown verb: {topic}"))
}

const fn help() -> &'static str {
    "TERMCTL\n\nUSAGE\n  termctl [OPTIONS] COMMAND [COMMAND OPTIONS]\n  termctl -- COMMAND [ARGUMENTS]\n\nA one-call ctl.v1 client for a running Termdeck session.\n\nCOMMANDS\n  status                 Show session status.\n  list                   List terminals.\n  peek ID [--lines N|N]  Read a terminal screen or history.\n  notify MESSAGE         Send an explicit notification.\n  open PATH              Open a directory as a terminal.\n  close ID [--force]     Close a terminal.\n  promote ID             Promote a terminal to master.\n  zoom [--on|--off]      Toggle or set zoom mode; status reads it.\n  input ID KIND VALUE    Send --text, --paste, or --keys input.\n  version                Show the ctl.v1 schema version.\n  help [COMMAND]         Show general or command-specific help.\n\nGLOBAL OPTIONS\n  --json         Print the ctl.v1 response as JSON.\n  --socket PATH  Connect to PATH instead of TERMDECK_SOCK.\n  --             End option parsing; remaining arguments are positional.\n  -h, --help     Show this help page.\n\nENVIRONMENT\n  TERMDECK_SOCK               Socket path for the running Termdeck session.\n  TERMDECK_PANE               Current pane identity for notifications.\n  TERMDECK_NOTIFY             Enables automatic shell completion notifications.\n  TERMDECK_NOTIFY_LONG_SECS   Long-command threshold in seconds.\n  TERMDECK_ALLOW_INPUT        Permits termctl input requests from this pane.\n\nEXAMPLES\n  termctl status\n  termctl list --json\n  termctl peek backend --lines 40\n  termctl notify 'build completed'\n  termctl input backend --paste 'git status'\n  termctl input backend --keys 'C-c Enter Up' 'text'\n  termctl help input"
}

fn command_help(command: &str) -> Option<&'static str> {
    match command {
        "status" => Some(
            "TERMCTL-STATUS\n\nUSAGE\n  termctl [OPTIONS] status\n\nShows the session name, terminals, active pane, size, and view state.",
        ),
        "list" => Some(
            "TERMCTL-LIST\n\nUSAGE\n  termctl [OPTIONS] list\n\nLists terminals and their identities, paths, states, and master status.",
        ),
        "peek" => Some(
            "TERMCTL-PEEK\n\nUSAGE\n  termctl [OPTIONS] peek ID [--lines N|N]\n\nReads up to N active-screen or retained-history lines; N defaults to 30.",
        ),
        "notify" => Some(notify_help()),
        "open" => Some(
            "TERMCTL-OPEN\n\nUSAGE\n  termctl [OPTIONS] open PATH\n\nOpens PATH as a new terminal in the current session.",
        ),
        "close" => Some(
            "TERMCTL-CLOSE\n\nUSAGE\n  termctl [OPTIONS] close ID [--force]\n\nCloses terminal ID; --force bypasses its ordinary close confirmation.",
        ),
        "promote" => Some(
            "TERMCTL-PROMOTE\n\nUSAGE\n  termctl [OPTIONS] promote ID\n\nPromotes terminal ID to the master pane.",
        ),
        "zoom" => Some(
            "TERMCTL-ZOOM\n\nUSAGE\n  termctl [OPTIONS] zoom [--on|--off]\n\nToggles zoom without an option. Use status to read zoom state; --on and --off explicitly set it.",
        ),
        "input" => Some(
            "TERMCTL-INPUT\n\nUSAGE\n  termctl [OPTIONS] input ID --text VALUE [--force]\n  termctl [OPTIONS] input ID --paste VALUE [--force]\n  termctl [OPTIONS] input ID --keys VALUE... [--force]\n\nSends text, bracketed paste, or keys to terminal ID. --keys accepts C-, M-, and S- modifiers; Enter, Tab, Backspace, Up, Down, Left, Right, PageUp, PageDown, Home, End, Escape, and F1 through F12; other bare words are literal text. For raw compatibility, existing control bytes such as $'\\x03' pass through; prefix Raw: to force a named-looking token literal (Raw:Up).\n\nEXAMPLE\n  termctl input backend --keys 'C-c Enter Up' 'text'",
        ),
        "version" => Some(
            "TERMCTL-VERSION\n\nUSAGE\n  termctl [OPTIONS] version\n\nShows the supported ctl.v1 schema version.",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use termdeck::ctl::{Response, SCHEMA};

    use super::{command_help, help, help_for, notify_help, parse, print_response, run};

    #[test]
    fn notify_help_cross_references_automatic_shell_notifications() {
        assert!(notify_help().contains("README.md#shell-notifications"));
    }

    #[test]
    fn exit_codes_cover_ok_runtime_usage_and_refusal() {
        assert_eq!(
            print_response(
                Response {
                    schema: SCHEMA.to_owned(),
                    ok: true,
                    data: Some(serde_json::json!({ "delivered": true })),
                    error: None,
                },
                false,
            ),
            0
        );
        for code in 1..=3 {
            assert_eq!(
                print_response(Response::error(code, "expected"), false),
                i32::from(code)
            );
        }
        let usage = run(Vec::new());
        let refusal = print_response(Response::error(3, "declined"), false);
        assert_eq!(usage, 2);
        assert_eq!(refusal, 3);
        assert_ne!(usage, refusal, "usage and session refusal stay distinct");
        assert_eq!(
            run(vec![
                "--socket".to_owned(),
                PathBuf::from("/definitely/not/a/termdeck.sock")
                    .display()
                    .to_string(),
                "version".to_owned(),
            ]),
            1
        );
    }

    #[test]
    fn general_and_every_command_help_return_success() {
        assert!(help().contains("TERMDECK_ALLOW_INPUT"));
        assert!(help().contains("Toggle or set zoom mode; status reads it."));
        assert!(
            help_for(&["--help".to_owned()])
                .unwrap()
                .unwrap()
                .contains("USAGE")
        );
        for verb in [
            "status", "list", "peek", "notify", "open", "close", "promote", "zoom", "input",
            "version",
        ] {
            assert_eq!(run(vec![verb.to_owned(), "--help".to_owned()]), 0);
            assert!(
                help_for(&["help".to_owned(), verb.to_owned()])
                    .unwrap()
                    .unwrap()
                    .contains("USAGE")
            );
        }
        assert!(
            help_for(&[
                "--socket".to_owned(),
                "/tmp/termdeck.sock".to_owned(),
                "status".to_owned(),
                "--help".to_owned(),
            ])
            .unwrap()
            .unwrap()
            .contains("TERMCTL-STATUS")
        );
        assert_eq!(run(vec!["unknown".to_owned()]), 2);
        assert_eq!(run(vec!["--unknown".to_owned()]), 2);
    }

    #[test]
    fn help_prepass_keeps_help_literals_as_input_values() {
        for input_kind in ["--text", "--paste", "--keys"] {
            for literal_help in ["--help", "-h"] {
                let arguments = [
                    "input".to_owned(),
                    "one".to_owned(),
                    input_kind.to_owned(),
                    literal_help.to_owned(),
                ];
                assert_eq!(help_for(&arguments).unwrap(), None);
            }
        }
    }

    #[test]
    fn zoom_help_describes_toggle_and_status_read() {
        let zoom_help = command_help("zoom").unwrap();
        assert!(zoom_help.contains("Toggles zoom without an option."));
        assert!(zoom_help.contains("Use status to read zoom state"));
    }

    #[test]
    fn parse_peek_and_notify() {
        let (_, _, peek) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "peek".to_owned(),
            "one".to_owned(),
            "5".to_owned(),
        ])
        .unwrap();
        assert_eq!(peek.id.as_deref(), Some("one"));
        assert_eq!(peek.lines, Some(5));

        let (_, _, peek) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "peek".to_owned(),
            "one".to_owned(),
            "--lines".to_owned(),
            "3".to_owned(),
        ])
        .unwrap();
        assert_eq!(peek.lines, Some(3));

        let (_, _, notify) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "notify".to_owned(),
            "build".to_owned(),
            "done".to_owned(),
        ])
        .unwrap();
        assert_eq!(notify.msg.as_deref(), Some("build done"));

        let (_, _, literal_notify) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "--".to_owned(),
            "notify".to_owned(),
            "--help".to_owned(),
        ])
        .unwrap();
        assert_eq!(literal_notify.msg.as_deref(), Some("--help"));
    }

    #[test]
    fn parse_control_verbs() {
        let (_, _, open) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "open".to_owned(),
            "/work/api".to_owned(),
        ])
        .unwrap();
        assert_eq!(open.path.as_deref(), Some("/work/api"));

        let (_, _, input) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "input".to_owned(),
            "two".to_owned(),
            "--paste".to_owned(),
            "hello".to_owned(),
            "--force".to_owned(),
        ])
        .unwrap();
        assert_eq!(input.id.as_deref(), Some("two"));
        assert_eq!(input.paste.as_deref(), Some("hello"));
        assert!(input.force);

        let (_, _, zoom) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "zoom".to_owned(),
            "--on".to_owned(),
        ])
        .unwrap();
        assert_eq!(zoom.on, Some(true));
        assert!(
            parse(vec![
                "--socket".to_owned(),
                "/tmp/ctl.sock".to_owned(),
                "zoom".to_owned(),
                "--on".to_owned(),
                "--off".to_owned(),
            ])
            .is_err()
        );
    }

    #[test]
    fn parse_encodes_named_keys_before_building_the_request() {
        let (_, _, request) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "input".to_owned(),
            "backend".to_owned(),
            "--keys".to_owned(),
            "C-c Enter Up".to_owned(),
            "text".to_owned(),
            "--force".to_owned(),
        ])
        .unwrap();

        assert_eq!(request.id.as_deref(), Some("backend"));
        assert_eq!(request.keys.as_deref(), Some("\u{3}\r\u{1b}[Atext"));
        assert!(request.force);
    }

    #[test]
    fn parse_rejects_unknown_named_key_before_connecting() {
        let arguments = vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "input".to_owned(),
            "backend".to_owned(),
            "--keys".to_owned(),
            "C-not-a-key".to_owned(),
        ];
        let error = parse(arguments.clone()).unwrap_err();
        assert!(error.contains("C-not-a-key"));
        assert_eq!(run(arguments), 2);
    }

    #[test]
    fn input_help_documents_named_keys_and_raw_compatibility() {
        let input_help = command_help("input").unwrap();
        assert!(input_help.contains("C-c Enter Up"));
        assert!(input_help.contains("Raw:Up"));
        assert!(help().contains("--keys 'C-c Enter Up' 'text'"));
    }
}
