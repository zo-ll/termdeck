//! Human-friendly ctl.v1 client.  It is intentionally a thin one-call socket
//! wrapper, so agents and shell users exercise exactly the session protocol.

use std::{env, path::PathBuf, process};

use termdeck::ctl::{self, Request, Response, SCHEMA};

fn main() {
    process::exit(run(env::args().skip(1).collect()));
}

fn run(arguments: Vec<String>) -> i32 {
    let (json, socket, request) = match parse(arguments) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("termctl: {message}");
            return 2;
        }
    };
    match ctl::call(&socket, &request) {
        Ok(response) => print_response(response, json),
        Err(message) => {
            eprintln!("termctl: {message}");
            1
        }
    }
}

fn parse(arguments: Vec<String>) -> Result<(bool, PathBuf, Request), String> {
    let mut json = false;
    let mut socket = None;
    let mut requested_lines = None;
    let mut force = false;
    let mut on = None;
    let mut input = None;
    let mut positional = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
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
        return Err(usage().to_owned());
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
        "zoom" if positional.len() == 1 => (None, None, None, None),
        "status" | "list" | "peek" | "notify" | "version" | "open" | "close" | "promote"
        | "zoom" | "input" => return Err(usage().to_owned()),
        _ => return Err(format!("unknown verb: {verb}")),
    };
    if (force && !matches!(verb.as_str(), "close" | "input"))
        || (on.is_some() && verb != "zoom")
        || (input.is_some() && verb != "input")
    {
        return Err(usage().to_owned());
    }
    if verb == "input" && input.is_none() {
        return Err(usage().to_owned());
    }
    let (text, paste, keys) = match input {
        Some((kind, value)) if kind == "--text" => (Some(value), None, None),
        Some((kind, value)) if kind == "--paste" => (None, Some(value), None),
        Some((_, value)) => (None, None, Some(value)),
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
    "usage: termctl [--json] [--socket PATH] status|list|peek ID [--lines N|N]|notify MSG|open PATH|close ID [--force]|promote ID|zoom [--on|--off]|input ID (--text|--paste|--keys) VALUE [--force]|version"
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use termdeck::ctl::{Response, SCHEMA};

    use super::{parse, print_response, run};

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
        assert_eq!(run(Vec::new()), 2);
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
}
